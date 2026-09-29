#!/usr/bin/env python3
"""Exercise delayed same-view navigation in the production browser client.

Uses disposable data and the private Playwright environment documented in
stylus-input.md. Screenshots used for pixel assertions stay in memory. Optional
captures contain only application bars, never image content or annotation geometry.
"""
import argparse
import asyncio
import io
import json
import tempfile
from pathlib import Path
from urllib.parse import urlsplit

from PIL import Image
from playwright.async_api import async_playwright
from stylus_input import COLOR, Scenario, application, png, require, until


async def run(args):
    with application() as (origin, api, server):
        async with async_playwright() as playwright:
            profile = tempfile.TemporaryDirectory(prefix="labello-silent-loading-")
            launch_args = ["--enable-unsafe-swiftshader", f"--force-device-scale-factor={args.dpr}"]
            browser = None
            context = None
            try:
                if args.zoom:
                    extension = Path(profile.name) / "extension"
                    extension.mkdir()
                    (extension / "manifest.json").write_text(json.dumps({"manifest_version": 3,
                        "name": "Local verification zoom", "version": "1.0", "permissions": ["tabs"],
                        "background": {"service_worker": "background.js"}}))
                    (extension / "background.js").write_text("chrome.runtime.onInstalled.addListener(() => {});")
                    context = await playwright.chromium.launch_persistent_context(str(Path(profile.name) / "profile"),
                        executable_path=playwright.chromium.executable_path, headless=True,
                        ignore_default_args=["--disable-extensions"],
                        args=launch_args + [f"--disable-extensions-except={extension}", f"--load-extension={extension}"],
                        viewport={"width": 1288, "height": 820}, device_scale_factor=args.dpr)
                else:
                    browser = await playwright.chromium.launch(args=launch_args)
                    context = await browser.new_context(viewport={"width": 1288, "height": 820}, device_scale_factor=args.dpr)
                scenario = Scenario(context, origin, api)

                async def ready():
                    require(server.poll() is None, "server-startup-failed")
                    try:
                        return (await context.request.get(api + "/health", timeout=1000)).ok
                    except Exception:
                        return False

                await until(ready, "server-readiness-timeout")
                await scenario.seed(args.kind)
                for index in range(3):
                    with Image.open(io.BytesIO(png())) as fixture:
                        fixture.putpixel((0, 0), (index, 0, 0))
                        data = io.BytesIO()
                        fixture.save(data, format="PNG")
                    await scenario.request("POST", "/datasets/stylus/uploads?root=uploads/stylus&ingest=true",
                        multipart={"files": {"name": f"fixture-{index}.png", "mimeType": "image/png", "buffer": data.getvalue()}})
                if args.review:
                    metadata = await scenario.request("GET", "/datasets/stylus/admin")
                    metadata["tasks"][0]["review"] = {"workflow": "approval", "allowReviewerCorrections": True}
                    await scenario.request("PUT", "/datasets/stylus/admin", data={key: metadata[key] for key in
                        ["name", "imageRoots", "labelClasses", "tasks", "roleAssignments", "imbalance", "prelabelConfigs"]})
                    for _ in range(4):
                        assignment = await scenario.request("POST", "/datasets/stylus/images/next",
                            data={"taskId": f"{args.kind}:fixture", "kind": "annotation"})
                        await scenario.request("POST", "/datasets/stylus/assignments/complete",
                            data={key: assignment[key] for key in ["assignmentId", "imageId", "taskId", "kind"]})
                    scenario.page = await context.new_page()
                    scenario.errors = []
                    scenario.page.on("pageerror", lambda _: scenario.errors.append("pageerror"))
                    await scenario.page.goto(origin + "/?api=" + api + "&dataset=stylus")
                    await scenario.page.locator("#startup-status").wait_for(state="detached", timeout=30000)
                    async def open_review():
                        if await scenario.color_bounds(COLOR):
                            return True
                        await scenario.page.mouse.click(150, 28)
                        await scenario.page.wait_for_timeout(250)
                        return False
                    await until(open_review, "review-image-not-rendered")
                    await scenario.page.set_viewport_size({"width": args.width, "height": args.height})
                else:
                    await scenario.open(args.width, args.height, False, "cdp", "chromium")
                if args.zoom:
                    worker = context.service_workers[0] if context.service_workers else await context.wait_for_event("serviceworker")
                    factor = await worker.evaluate("""async () => {
                        const tabs = await chrome.tabs.query({});
                        const tab = tabs.find(tab => tab.url && tab.url.startsWith('http://127.0.0.1:'));
                        await chrome.tabs.setZoom(tab.id, 2);
                        return await chrome.tabs.getZoom(tab.id);
                    }""")
                    require(factor == 2, "browser-zoom-not-applied")
                await scenario.page.wait_for_timeout(500)
                scenario.bounds = await until(lambda: scenario.color_bounds(COLOR), "image-not-rendered")
                cdp = await context.new_cdp_session(scenario.page)
                version = (await cdp.send("Browser.getVersion"))["product"]
                tree = await cdp.send("Accessibility.getFullAXTree")
                require(any(node.get("role", {}).get("value") == "Canvas" for node in tree["nodes"]), "canvas-accessibility-node")
                inflight = set()
                scenario.page.on("request", lambda request: inflight.add(request) if request.url.startswith(api) else None)
                scenario.page.on("requestfinished", lambda request: inflight.discard(request))
                scenario.page.on("requestfailed", lambda request: inflight.discard(request))

                async def requests_settled():
                    if inflight:
                        return False
                    await scenario.page.wait_for_timeout(350)
                    return not inflight

                held = asyncio.Event()
                release = asyncio.Event()
                armed = None

                async def delayed(route):
                    nonlocal armed
                    path = urlsplit(route.request.url).path
                    if armed and path.endswith(armed):
                        armed = None
                        held.set()
                        await release.wait()
                    await route.continue_()

                await scenario.page.route(api + "/**", delayed)
                samples = 0
                for index, (key, suffix) in enumerate([("x", "/assignments/release"), ("ArrowLeft", "/assignments/reopen"),
                                                       ("x", "/assignments/release"), ("ArrowLeft", "/assignments/reopen")]):
                    await until(requests_settled, "previous-transition-not-settled")
                    held.clear()
                    release.clear()
                    armed = suffix
                    bounds = await until(lambda: scenario.color_bounds(COLOR), "ready-image-not-rendered")
                    # A flat-color interior point detects blank or opacity-faded frames.
                    point = (int((bounds[0] + bounds[2]) / 2), int((bounds[1] + bounds[3]) / 2))
                    await scenario.page.keyboard.press(key)
                    try:
                        await asyncio.wait_for(held.wait(), timeout=15)
                    except TimeoutError:
                        raise AssertionError(f"transition-request-not-started-{index}") from None
                    try:
                        for _ in range(5):
                            await scenario.page.wait_for_timeout(80)
                            with Image.open(io.BytesIO(await scenario.page.screenshot(scale="css"))) as capture:
                                pixel = capture.convert("RGB").getpixel(point)
                            require(max(abs(a - b) for a, b in zip(pixel, COLOR)) < 10, "retained-image-faded-or-blank")
                            samples += 1
                        if args.artifacts and index == 1:
                            folder = Path(args.artifacts)
                            folder.mkdir(parents=True, exist_ok=True)
                            await scenario.page.screenshot(path=str(folder / f"{'review' if args.review else 'annotation'}-{args.kind}-{args.width}-{'zoom2-' if args.zoom else ''}pending.png"),
                                clip={"x": 0, "y": 0, "width": args.width, "height": 114 if args.width >= 1288 else 140}, scale="css")
                    finally:
                        release.set()
                    await until(requests_settled, "transition-did-not-settle")
                require(not scenario.errors, "browser-page-error")
                print(json.dumps({"result": "passed", "browser": version, "kind": args.kind,
                    "workflow": "review" if args.review else "annotation", "viewport": [args.width, args.height],
                    "dpr": args.dpr, "zoom": 2 if args.zoom else 1, "delayed_transitions": 4, "pixel_samples": samples}))
            finally:
                if context:
                    await context.close()
                if browser:
                    await browser.close()
                profile.cleanup()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=["bounding_box", "skeleton"], default="bounding_box")
    parser.add_argument("--review", action="store_true")
    parser.add_argument("--width", type=int, default=1288)
    parser.add_argument("--height", type=int, default=820)
    parser.add_argument("--dpr", type=float, default=1)
    parser.add_argument("--artifacts")
    parser.add_argument("--zoom", action="store_true", help="Apply actual 200% Chrome tab zoom")
    asyncio.run(asyncio.wait_for(run(parser.parse_args()), timeout=150))
