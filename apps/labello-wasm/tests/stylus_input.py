#!/usr/bin/env python3
"""Exercise browser pen events through the production WASM client and API.

Run after building labello-server and the release Trunk distribution. All data
and browser state are disposable. No screenshots, payloads, or credentials are
written to reports. See docs/stylus-input.md for the contract and procedure.
"""

import argparse
import asyncio
import contextlib
import functools
import http.server
import io
import json
import os
import platform
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

from PIL import Image, ImageChops
from playwright.async_api import async_playwright


ROOT = Path(__file__).resolve().parents[3]
COLOR = (187, 43, 129)
REFERENCE_COLOR = (53, 126, 190)


def require(condition, category):
    if not condition:
        raise AssertionError(category)


async def until(check, category, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = await check()
        if result:
            return result
        await asyncio.sleep(0.1)
    raise AssertionError(category)


def png():
    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data)))

    rows = []
    for y in range(600):
        row = bytearray(bytes(COLOR) * 800)
        for x, center_y in [(200, 150), (600, 450)]:
            if center_y - 8 <= y < center_y + 8:
                row[(x - 8) * 3:(x + 8) * 3] = bytes(REFERENCE_COLOR) * 16
        rows.append(b"\0" + row)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", 800, 600, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(b"".join(rows)))
            + chunk(b"IEND", b""))


class QuietFiles(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


@contextlib.contextmanager
def application():
    require((ROOT / "target/debug/labello-server").is_file(), "server-build-missing")
    dist = ROOT / "apps/labello-wasm/dist"
    require((dist / "index.html").is_file(), "wasm-build-missing")
    with tempfile.TemporaryDirectory(prefix="labello-stylus-") as directory:
        handler = functools.partial(QuietFiles, directory=str(dist))
        with http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as frontend:
            thread = threading.Thread(target=frontend.serve_forever, daemon=True)
            thread.start()
            origin = f"http://127.0.0.1:{frontend.server_port}"
            with socket.socket() as reserved:
                reserved.bind(("127.0.0.1", 0))
                port = reserved.getsockname()[1]
            config = Path(directory) / "server.toml"
            config.write_text(
                f'bind = "127.0.0.1:{port}"\n'
                f'datasetsRoot = "{directory}/datasets"\n'
                'bootstrapAdmins = ["admin"]\n'
                f'browserOrigins = ["{origin}"]\n'
                'sessionCookieSecure = false\n'
                '[developmentAuth]\nlocalAdminLogin = true\n'
                f'[previews]\ncacheRoot = "{directory}/previews"\n'
            )
            env = {k: v for k, v in os.environ.items()
                   if not k.startswith(("LABELLO_", "GITHUB_"))}
            env["LABELLO_CONFIG"] = str(config)
            server = subprocess.Popen(
                [str(ROOT / "target/debug/labello-server")], cwd=directory,
                env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                yield origin, f"http://127.0.0.1:{port}", server
            finally:
                if server.poll() is None:
                    server.send_signal(signal.SIGINT)
                    try:
                        server.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        server.kill()
                        server.wait(timeout=5)
                frontend.shutdown()
                thread.join(timeout=5)


class Scenario:
    def __init__(self, context, origin, api):
        self.context, self.origin, self.api = context, origin, api
        self.headers = {"Origin": origin}

    async def request(self, method, path, **kwargs):
        response = await self.context.request.fetch(
            self.api + path, method=method, headers=self.headers, **kwargs,
        )
        require(response.ok, f"fixture-api-{response.status}")
        return await response.json()

    async def seed(self, kind):
        self.headers["x-csrf-token"] = (await self.request("POST", "/auth/local-admin"))["csrfToken"]
        metadata = await self.request("POST", "/datasets", data={
            "datasetId": "stylus", "name": "Stylus fixture", "adminUserId": "admin",
        })
        metadata["labelClasses"] = [{
            "classId": "fixture", "name": "Fixture", "color": "#5eead4", "description": None,
        }]
        metadata["tasks"] = [{
            "taskId": f"{kind}:fixture", "name": "Stylus fixture",
            "annotationType": kind, "classIds": ["fixture"],
            "instructions": {"title": "", "exampleText": "", "exampleImages": []},
            "skeleton": None if kind == "bounding_box" else {
                "keypoints": [{"name": "center", "required": True}],
                "edges": [], "allowHidden": True, "allowAbsent": False,
            },
            "review": {"workflow": "none", "allowReviewerCorrections": False},
            "prelabelConfigIds": [], "enabled": True,
        }]
        await self.request("PUT", "/datasets/stylus/admin", data={
            key: metadata[key] for key in [
                "name", "imageRoots", "labelClasses", "tasks", "roleAssignments",
                "imbalance", "prelabelConfigs",
            ]
        })
        await self.request("POST", "/datasets/stylus/uploads?root=uploads/stylus&ingest=true", multipart={
            "files": {"name": "fixture.png", "mimeType": "image/png", "buffer": png()},
        })
        listing = await self.request("GET", "/datasets/stylus/images")
        require(len(listing["items"]) == 1, "fixture-image-count")
        self.image_path = "/datasets/stylus/images/" + listing["items"][0]["image"]["imageId"]

    async def color_bounds(self, color):
        # Screenshots exist only in memory to locate this test's flat-color image.
        # Neither image content nor annotation coordinates enter the report.
        with Image.open(io.BytesIO(await self.page.screenshot(scale="css"))) as capture:
            rgb = capture.convert("RGB")
        channels = ImageChops.difference(rgb, Image.new("RGB", rgb.size, color)).split()
        masks = [channel.point(lambda value: 255 if value < 8 else 0) for channel in channels]
        return ImageChops.multiply(ImageChops.multiply(masks[0], masks[1]), masks[2]).getbbox()

    async def canvas_pixels(self):
        with Image.open(io.BytesIO(await self.page.screenshot(scale="css"))) as capture:
            return capture.convert("RGB").crop(self.bounds)

    async def open(self, width, height, negative_control, event_mode, browser_name):
        self.event_mode = event_mode
        self.page = await self.context.new_page()
        self.errors = []
        self.page.on("pageerror", lambda _: self.errors.append("pageerror"))
        await self.page.add_init_script("""(() => {
            window.stylusCounts = {};
            for (const type of ['pointerdown', 'pointermove', 'pointerup',
                                'mousedown', 'mousemove', 'mouseup',
                                'touchstart', 'touchmove', 'touchend']) {
                document.addEventListener(type, event => {
                    const key = type + ':' + (event.pointerType || 'compat');
                    window.stylusCounts[key] = (window.stylusCounts[key] || 0) + 1;
                    if (!event.isTrusted) window.stylusCounts.untrusted = true;
                    if (type === 'pointerdown' && event.pointerType === 'pen') {
                        window.stylusPointerId = event.pointerId;
                    }
                }, true);
            }
        })();""")
        if negative_control:
            await self.page.add_init_script("""document.addEventListener('pointerdown', e => {
                if (e.pointerType === 'pen') e.stopImmediatePropagation();
            }, true);""")
        await self.page.goto(self.origin + "/?api=" + self.api + "&dataset=stylus")
        await self.page.locator("#startup-status").wait_for(state="detached", timeout=30000)
        async def canvas_ready():
            return await self.page.evaluate("""() => {
                const canvas = document.getElementById('labello-canvas');
                return Math.abs(canvas.width - canvas.clientWidth * devicePixelRatio) <= 1
                    && Math.abs(canvas.height - canvas.clientHeight * devicePixelRatio) <= 1;
            }""")
        await until(canvas_ready, "canvas-backing-size-dpr-mismatch")
        # Start at a fixed desktop viewport, then resize the annotation workspace.
        # App navigation is available even when Setup has no recommendation yet.
        await self.page.wait_for_timeout(1000)
        await self.page.mouse.click(64, 28)  # Annotate in the desktop application bar.
        self.bounds = await until(lambda: self.color_bounds(COLOR), "annotation-image-not-rendered")
        self.cdp = None
        if browser_name == "chromium":
            self.cdp = await self.context.new_cdp_session(self.page)
            tree = await self.cdp.send("Accessibility.getFullAXTree")
            require(any(node.get("role", {}).get("value") == "Canvas" for node in tree["nodes"]),
                    "browser-canvas-accessibility-node")
        await self.page.set_viewport_size({"width": width, "height": height})
        await self.page.wait_for_timeout(500)
        self.bounds = await until(lambda: self.color_bounds(COLOR), "resized-image-not-rendered")

    async def state(self):
        return await self.request("GET", self.image_path)

    async def annotation(self, version):
        async def saved():
            state = await self.state()
            if len(state["annotations"]) != 1:
                return False
            versions = next(iter(state["annotations"].values()))
            if versions[-1]["version"] < version:
                return False
            require(len(versions) == version and versions[-1]["version"] == version,
                    "duplicate-annotation-revision")
            return versions[-1]

        annotation = await until(saved, "annotation-save-timeout")
        await self.page.wait_for_timeout(600)
        state = await self.state()
        require(len(state["annotations"]) == 1, "duplicate-annotation")
        require(len(next(iter(state["annotations"].values()))) == version,
                "delayed-duplicate-revision")
        async def viewport_settled():
            return await self.color_bounds(COLOR) == self.bounds
        await until(viewport_settled, "pen-changed-viewport")
        return annotation

    def point(self, normalized):
        left, top, right, bottom = self.bounds
        return (left + (right - left) * normalized[0], top + (bottom - top) * normalized[1])

    async def gesture(self, start, end=None, pointer="pen", cancel=False):
        self.touch_first = not getattr(self, "touch_first", False)
        await self.page.wait_for_timeout(600)  # Separate gestures from double-click Fit.
        await self.page.evaluate("window.stylusCounts = {}")
        x, y = self.point(start)

        async def event(kind, x, y, hover=False):
            down = kind != "mouseReleased" and not hover
            payload = {
                "type": kind, "x": x, "y": y, "pointerType": pointer,
                "button": "none" if hover else "left", "buttons": 1 if down else 0, "clickCount": 1,
                "force": 0.6 if down else 0, "tiltX": 15 if pointer == "pen" else 0,
            }
            if pointer == "pen" and self.event_mode != "cdp":
                payload["compat"] = self.event_mode == "dom-compat" and not hover
                payload["touchFirst"] = self.touch_first
                await self.page.evaluate("""p => {
                    const type = {mousePressed:'pointerdown', mouseMoved:'pointermove',
                                  mouseReleased:'pointerup'}[p.type];
                    const canvas = document.getElementById('labello-canvas');
                    const emitPointer = () => canvas.dispatchEvent(new PointerEvent(type, {
                        bubbles:true, cancelable:true, pointerType:'pen', pointerId:41,
                        isPrimary:true, button:type === 'pointermove' ? -1 : 0,
                        buttons:p.buttons, clientX:p.x, clientY:p.y, pressure:p.force
                    }));
                    if (!p.compat || !p.touchFirst) emitPointer();
                    if (p.compat) {
                        const touch = new Touch({identifier:41, target:canvas, clientX:p.x,
                            clientY:p.y, pageX:p.x, pageY:p.y, touchType:'stylus'});
                        // The nonstandard property is absent in Chromium's Touch constructor.
                        if (touch.touchType !== 'stylus') Object.defineProperty(touch, 'touchType', {value:'stylus'});
                        const active = type !== 'pointerup' ? [touch] : [];
                        canvas.dispatchEvent(new TouchEvent({pointerdown:'touchstart',
                            pointermove:'touchmove', pointerup:'touchend'}[type], {
                            bubbles:true, cancelable:true, touches:active, targetTouches:active,
                            changedTouches:[touch]
                        }));
                        if (p.touchFirst) emitPointer();
                        canvas.dispatchEvent(new MouseEvent({pointerdown:'mousedown',
                            pointermove:'mousemove', pointerup:'mouseup'}[type], {
                            bubbles:true, cancelable:true, button:0, buttons:p.buttons,
                            clientX:p.x, clientY:p.y
                        }));
                    }
                }""", payload)
            elif pointer == "mouse":
                await self.page.mouse.move(x, y)
                if kind == "mousePressed":
                    await self.page.mouse.down()
                elif kind == "mouseReleased":
                    await self.page.mouse.up()
            else:
                await self.cdp.send("Input.dispatchMouseEvent", payload)
            await self.page.wait_for_timeout(60)

        await event("mouseMoved", x, y, hover=True)
        await event("mousePressed", x, y)
        if end is not None:
            before = await self.canvas_pixels()
            end_x, end_y = self.point(end)
            for step in range(1, 7):
                await event("mouseMoved", x + (end_x - x) * step / 6, y + (end_y - y) * step / 6)
            x, y = end_x, end_y
            after = await self.canvas_pixels()
            require(ImageChops.difference(before, after).getbbox() is not None,
                    "drag-preview-did-not-move")
        if cancel == "escape":
            await self.page.keyboard.press("Escape")
            await self.page.wait_for_timeout(60)
        elif cancel:
            await self.page.evaluate("""kind => {
                document.getElementById('labello-canvas').dispatchEvent(new PointerEvent(kind, {
                    bubbles:true, pointerId:window.stylusPointerId, pointerType:'pen', isPrimary:true
                }));
            }""", cancel)
            await self.page.wait_for_timeout(100)
        await event("mouseReleased", x, y)
        counts = await self.page.evaluate("window.stylusCounts")
        if self.event_mode == "cdp" or pointer == "mouse":
            require(not counts.get("untrusted"), "untrusted-dom-input")
        for name in ["pointerdown", "pointerup"]:
            require(counts.get(name + ":" + pointer) == 1, "missing-pointer-event")
        if self.event_mode == "dom-compat" or pointer == "mouse":
            for name in ["mousedown", "mouseup"] + (["mousemove"] if end else []):
                require(counts.get(name + ":compat", 0) >= 1, "missing-compatibility-event")
        if self.event_mode == "dom-compat" and pointer == "pen":
            for name in ["touchstart", "touchend"] + (["touchmove"] if end else []):
                require(counts.get(name + ":compat", 0) >= 1, "missing-stylus-touch-event")
        if self.event_mode != "dom-compat":
            require(not any(key.startswith("touch") for key in counts), "pen-produced-touch-event")

    async def boxes(self):
        await self.gesture((0.2, 0.2), (0.5, 0.5))
        created = await self.annotation(1)
        check_box(created, (0.2, 0.2, 0.3, 0.3))
        await self.gesture((0.35, 0.35), (0.45, 0.45))
        moved = await self.annotation(2)
        check_box(moved, (0.3, 0.3, 0.3, 0.3))
        await self.gesture((0.6, 0.6), (0.75, 0.75))
        resized = await self.annotation(3)
        check_box(resized, (0.3, 0.3, 0.45, 0.45))
        await self.gesture((0.5, 0.5), (0.55, 0.45), pointer="mouse")
        check_box(await self.annotation(4), (0.35, 0.25, 0.45, 0.45))
        await self.gesture((0.55, 0.45), (0.5, 0.5))
        check_box(await self.annotation(5), (0.3, 0.3, 0.45, 0.45))
        for cancellation in ["escape", "pointercancel", "lostpointercapture"]:
            before = await self.state()
            await self.gesture((0.5, 0.5), (0.6, 0.6), cancel=cancellation)
            await self.page.wait_for_timeout(1000)
            require((await self.state())["annotations"] == before["annotations"], "cancel-saved-edit")
        await self.touch_then_fit()
        await self.gesture((0.5, 0.5), (0.55, 0.45))
        check_box(await self.annotation(6), (0.35, 0.25, 0.45, 0.45))

    async def keypoints(self):
        await self.gesture((0.4, 0.4))
        check_point(await self.annotation(1), (0.4, 0.4))
        await self.gesture((0.4, 0.4), (0.6, 0.6))
        check_point(await self.annotation(2), (0.6, 0.6))
        await self.gesture((0.6, 0.6), (0.5, 0.5), pointer="mouse")
        check_point(await self.annotation(3), (0.5, 0.5))
        await self.gesture((0.5, 0.5), (0.4, 0.4))
        check_point(await self.annotation(4), (0.4, 0.4))
        await self.touch_then_fit()
        await self.gesture((0.4, 0.4), (0.6, 0.6))
        check_point(await self.annotation(5), (0.6, 0.6))

    async def touch_only_annotation(self):
        require(self.cdp is not None, "touch-only-check-requires-chromium")
        for event, point in [("touchStart", (0.2, 0.2)), ("touchMove", (0.5, 0.5)), ("touchEnd", None)]:
            points = [] if point is None else [{"id": 1, "x": self.point(point)[0], "y": self.point(point)[1]}]
            await self.cdp.send("Input.dispatchTouchEvent", {"type": event, "touchPoints": points})
            await self.page.wait_for_timeout(200)
        check_box(await self.annotation(1), (0.2, 0.2, 0.3, 0.3))

    async def pen_controls_do_not_scroll(self):
        original_size = self.page.viewport_size
        await self.page.set_viewport_size({"width": 1288, "height": 400})
        async def controls():
            with Image.open(io.BytesIO(await self.page.screenshot(scale="css"))) as capture:
                return capture.convert("RGB").crop((1000, 125, 1288, 325))
        # Inspector visibility can change when leaving a compact layout.
        # Establish a genuinely scrollable panel before checking pen behavior.
        for _ in range(2):
            await self.page.keyboard.press("i")
            await self.page.wait_for_timeout(500)
            await self.page.mouse.move(5, 5)
            before = await controls()
            await self.page.mouse.move(1120, 250)
            await self.page.mouse.wheel(0, 300)
            await self.page.mouse.move(5, 5)
            await self.page.wait_for_timeout(500)
            if ImageChops.difference(before, await controls()).getbbox() is not None:
                break
        else:
            raise AssertionError("inspector-scroll-negative-control")
        await self.page.mouse.move(1120, 250)
        await self.page.mouse.wheel(0, -10000)
        await self.page.mouse.move(5, 5)
        await self.page.wait_for_timeout(500)
        before = await controls()
        for event, y in [("pointerdown", 280), ("pointermove", 210), ("pointercancel", 210), ("pointerup", 210)]:
            await self.page.evaluate("""p => document.getElementById('labello-canvas').dispatchEvent(new PointerEvent(p.event, {
                bubbles:true, cancelable:true, pointerType:'pen', pointerId:91, isPrimary:true,
                button:p.event === 'pointermove' ? -1 : 0,
                buttons:['pointerdown','pointermove'].includes(p.event) ? 1 : 0,
                clientX:1120, clientY:p.y
            }))""", {"event": event, "y": y})
            await self.page.wait_for_timeout(100)
        await self.page.mouse.move(5, 5)
        await self.page.wait_for_timeout(300)
        require(ImageChops.difference(before, await controls()).getbbox() is None, "pen-scrolled-inspector-controls")
        await self.page.keyboard.press("i")
        await self.page.set_viewport_size(original_size)

    async def concurrent_touch_pen(self, kind):
        if self.cdp is None:
            return
        # Exercise the adapter with separate browser pen and finger streams.
        # Each sequence finishes with Fit so the saved-geometry helpers retain
        # their original-image coordinate checks.
        for fingers_first in [False, True]:
            before = await self.state()
            annotation = next(iter(before["annotations"].values()))[-1]
            version = annotation["version"]
            geometry = annotation["geometry"]["geometry"]
            start = ((geometry["x"] + geometry["width"] / 2,
                      geometry["y"] + geometry["height"] / 2) if kind == "bounding_box"
                     else tuple(geometry["keypoints"][0]["point"][axis] for axis in ["x", "y"]))
            x, y = self.point(start)
            async def pen(event, px=x, py=y):
                await self.page.evaluate("""p => {
                    document.getElementById('labello-canvas').dispatchEvent(new PointerEvent(p.event, {
                        bubbles:true, cancelable:true, pointerType:'pen', pointerId:81,
                        isPrimary:true, button:p.event === 'pointermove' ? -1 : 0,
                        buttons:p.event === 'pointerup' ? 0 : 1, clientX:p.x, clientY:p.y
                    }));
                }""", {"event": event, "x": px, "y": py})
                await self.page.wait_for_timeout(80)
            async def fingers(event, spread=0.1):
                points = [] if event == "touchEnd" else [
                    {"id": index + 1, "x": self.point((u, 0.5))[0], "y": self.point((u, 0.5))[1]}
                    for index, u in enumerate([0.5 - spread, 0.5 + spread])]
                await self.cdp.send("Input.dispatchTouchEvent", {"type": event, "touchPoints": points})
                await self.page.wait_for_timeout(150)
            if fingers_first:
                await fingers("touchStart")
            await pen("pointerdown")
            if not fingers_first:
                await fingers("touchStart")
            # A held pen must not become egui's touch long-press when fingers
            # are also down; that would silently discard widget drag ownership.
            await self.page.wait_for_timeout(1000)
            await fingers("touchMove", 0.12)
            require(await self.color_bounds(COLOR) != self.bounds, "concurrent-touch-did-not-zoom")
            # Read the rendered transform from two fixture reference marks.
            # Touch coordinates are rounded by the browser adapter, and a
            # clipped image rectangle cannot reveal pan at narrow widths.
            reference = await self.color_bounds(REFERENCE_COLOR)
            require(reference is not None, "pinch-reference-marks-missing")
            image_width = (reference[2] - reference[0]) / (0.5 + 16 / 800)
            image_height = (reference[3] - reference[1]) / (0.5 + 16 / 600)
            target = (start[0] + 0.02, start[1] - 0.02)
            px = reference[0] + (target[0] - (0.25 - 8 / 800)) * image_width
            py = reference[1] + (target[1] - (0.25 - 8 / 600)) * image_height
            await pen("pointermove", px, py)
            await pen("pointerup", px, py)
            await fingers("touchEnd")
            await self.page.keyboard.press("0")
            await self.page.wait_for_timeout(300)
            saved = await self.annotation(version + 1)
            if kind == "bounding_box":
                check_box(saved, (geometry["x"] + 0.02, geometry["y"] - 0.02, geometry["width"], geometry["height"]))
            else:
                check_point(saved, target)
        before = await self.state()
        for event, point in [("touchStart", (0.1, 0.7)), ("touchMove", (0.25, 0.85)), ("touchEnd", None)]:
            points = [] if point is None else [{"id": 7, "x": self.point(point)[0], "y": self.point(point)[1]}]
            await self.cdp.send("Input.dispatchTouchEvent", {"type": event, "touchPoints": points})
            await self.page.wait_for_timeout(100)
        await self.page.wait_for_timeout(1000)
        require((await self.state())["annotations"] == before["annotations"], "finger-created-annotation-after-pen")

    async def touch_then_fit(self):
        if self.cdp is None:
            return  # Trusted touch injection is tested separately in Chromium.
        before = await self.state()
        await self.page.wait_for_timeout(600)
        for kind, spread in [("touchStart", 0.1), ("touchMove", 0.2), ("touchEnd", None)]:
            points = [] if spread is None else [
                {"id": index + 1, "x": self.point((x, 0.5))[0], "y": self.point((x, 0.5))[1]}
                for index, x in enumerate([0.5 - spread, 0.5 + spread])
            ]
            await self.cdp.send("Input.dispatchTouchEvent", {"type": kind, "touchPoints": points})
            await self.page.wait_for_timeout(150)
        require(await self.color_bounds(COLOR) != self.bounds, "touch-did-not-zoom")
        await self.page.keyboard.press("0")
        await self.page.wait_for_timeout(1000)
        require(await self.color_bounds(COLOR) == self.bounds, "fit-did-not-restore-viewport")
        require((await self.state())["annotations"] == before["annotations"], "touch-edited-annotation")


def check_box(annotation, expected):
    require(annotation["geometry"]["type"] == "bounding_box", "geometry-kind")
    box = annotation["geometry"]["geometry"]
    require(all(abs(box[name] - value) < 0.012 for name, value in
                zip(["x", "y", "width", "height"], expected)), "box-geometry-mismatch")


def check_point(annotation, expected):
    require(annotation["geometry"]["type"] == "skeleton", "geometry-kind")
    points = annotation["geometry"]["geometry"]["keypoints"]
    require(len(points) == 1 and points[0]["state"] == "visible", "keypoint-count-or-state")
    require(all(abs(points[0]["point"][axis] - value) < 0.012
                for axis, value in zip(["x", "y"], expected)), "keypoint-geometry-mismatch")


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--negative-control", action="store_true",
                        help="Suppress pen pointerdown; the check must fail, never use for acceptance.")
    parser.add_argument("--touch-only", action="store_true",
                        help="Check finger-only box creation before any pen event (Chromium).")
    parser.add_argument("--width", type=int, default=1288)
    parser.add_argument("--height", type=int, default=820)
    parser.add_argument("--dpr", type=float, default=1)
    parser.add_argument("--kind", choices=["bounding_box", "skeleton"], default="bounding_box")
    parser.add_argument("--events", choices=["cdp", "pointer-only", "dom-compat"], default="cdp")
    parser.add_argument("--browser", choices=["chromium", "webkit"], default="chromium")
    args = parser.parse_args()
    with application() as (origin, api, server):
        async with async_playwright() as playwright:
            require(args.browser == "chromium" or args.events == "pointer-only",
                    "webkit-check-requires-pointer-only-events")
            browser_type = getattr(playwright, args.browser)
            browser = await browser_type.launch(
                args=["--enable-unsafe-swiftshader", f"--force-device-scale-factor={args.dpr}"]
                if args.browser == "chromium" else [],
                executable_path=os.environ.get("LABELLO_TEST_WEBKIT_EXECUTABLE")
                if args.browser == "webkit" else None,
            )
            try:
                context = await browser.new_context(viewport={"width": 1288, "height": 820},
                                                    device_scale_factor=args.dpr, has_touch=True)
                scenario = Scenario(context, origin, api)

                async def ready():
                    require(server.poll() is None, "server-startup-failed")
                    try:
                        return (await context.request.get(api + "/health", timeout=1000)).ok
                    except Exception:
                        return False

                await until(ready, "server-readiness-timeout")
                await scenario.seed(args.kind)
                await scenario.open(args.width, args.height, args.negative_control, args.events, args.browser)
                if args.touch_only:
                    require(args.kind == "bounding_box", "touch-only-check-requires-boxes")
                    await scenario.touch_only_annotation()
                    print(json.dumps({"result": "passed", "browser": browser.version,
                                      "viewport": [args.width, args.height], "dpr": args.dpr,
                                      "checks": ["touch-only-box-create-and-save-before-pen"]}))
                    return
                await (scenario.boxes() if args.kind == "bounding_box" else scenario.keypoints())
                await scenario.concurrent_touch_pen(args.kind)
                if scenario.cdp is not None:
                    await scenario.pen_controls_do_not_scroll()
                require(not scenario.errors, "browser-pageerror")
                checks = ["geometry", "drag-preview", "single-revision-per-edit",
                          "pen-mouse-pen", "unchanged-pen-viewport"]
                if args.events == "dom-compat":
                    checks.append("duplicate-stylus-touch-and-mouse")
                if scenario.cdp is not None:
                    checks.extend(["pen-touch-pen", "concurrent-dom-pen-trusted-touch-both-orders", "finger-does-not-annotate-after-pen", "pen-controls-do-not-scroll"])
                if args.kind == "bounding_box":
                    checks.append("escape-pointercancel-lostcapture")
                print(json.dumps({"result": "passed", "browser": browser.version,
                                  "os": platform.system(), "kind": args.kind,
                                  "viewport": [args.width, args.height], "dpr": args.dpr,
                                  "zoom": "100%", "input": args.events,
                                  "engine": args.browser,
                                  "trustedTouchCheck": scenario.cdp is not None,
                                  "checks": checks}))
            finally:
                await browser.close()


if __name__ == "__main__":
    try:
        asyncio.run(asyncio.wait_for(main(), timeout=120))
    except AssertionError as error:
        print(json.dumps({"result": "failed", "category": str(error)}))
        sys.exit(1)
    except (Exception, KeyboardInterrupt):
        # Playwright exceptions can contain URLs and HTTP details.
        print(json.dumps({"result": "failed", "category": "tool-startup-timeout-or-interruption"}))
        sys.exit(1)
