#!/usr/bin/env python3
"""Exercise compiled release A/B against a disposable identity/static server.

Build each artifact with LABELLO_RELEASE_TAG=v0.0.179-a / v0.0.179-b and
LABELLO_SOURCE_COMMIT=40 a's / 40 b's. Uses the Playwright environment documented
in docs/stylus-input.md. The server simulates deployment, not workflow/API policy.
"""
import argparse
import asyncio
import json
import mimetypes
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from playwright.async_api import async_playwright


class Deployment:
    def __init__(self, a, b):
        self.roots = {"a": a, "b": b}
        self.assets = "a"
        self.identity = "a"
        self.documents = 0
        self.build_checks = 0
        self.wasm_served = set()
        self.fail_assets = False
        self.refresh_requests = 0


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        state = self.server.deployment
        path = urlsplit(self.path).path
        status, kind, cache = 200, "application/json", "no-store"
        if path == "/api/build-information":
            state.build_checks += 1
            body = json.dumps({"releaseTag": f"v0.0.179-{state.identity}",
                               "sourceCommit": state.identity * 40}).encode()
        elif path == "/api/auth/options":
            body = b'{"githubOauth":false,"localAdminLogin":false}'
        elif path == "/api/me":
            status, body = 401, b'{"error":"unauthorized"}'
        elif path == "/labello.client.json":
            body = json.dumps({"apiBaseUrl": f"http://127.0.0.1:{self.server.server_port}/api/"}).encode()
        else:
            root = state.roots[state.assets]
            relative = path.lstrip("/") or "index.html"
            candidate = (root / relative).resolve()
            if self.headers.get("Sec-Fetch-Dest") == "document":
                state.documents += 1
            if self.headers.get("Cache-Control") in ("no-cache", "max-age=0"):
                state.refresh_requests += 1
            if not candidate.is_relative_to(root) or not candidate.is_file():
                status, body = 404, b"missing"
            elif state.fail_assets and candidate.suffix in (".js", ".wasm"):
                status, body = 503, b"unavailable"
            else:
                body = candidate.read_bytes()
                kind = mimetypes.guess_type(candidate.name)[0] or "application/octet-stream"
                cache = "public, max-age=3600"
                if candidate.suffix == ".wasm":
                    state.wasm_served.add(state.assets)
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Cache-Control", cache)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


async def until(predicate):
    for _ in range(300):
        if predicate():
            return
        await asyncio.sleep(.1)
    raise AssertionError("browser recovery timed out")


async def ready(page):
    await page.locator("#startup-status").wait_for(state="detached", timeout=60000)
    await page.wait_for_function("""() => {
        const canvas = document.getElementById('labello-canvas');
        return Math.abs(canvas.width - canvas.clientWidth * devicePixelRatio) <= 1
            && Math.abs(canvas.height - canvas.clientHeight * devicePixelRatio) <= 1;
    }""")
    await page.wait_for_timeout(800)


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    args = parser.parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True)
    state = Deployment(args.a.resolve(), args.b.resolve())
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.deployment = state
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f"http://127.0.0.1:{server.server_port}"
    results = []
    try:
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(headless=True, args=["--no-sandbox", "--enable-webgl", "--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
            for scenario in ("update", "persistent", "network-failure", "storage-denied"):
                state.assets, state.identity = "a", "a"
                state.documents, state.build_checks = 0, 0
                state.fail_assets = False
                state.wasm_served.clear()
                context = await browser.new_context(viewport={"width": 1288, "height": 820})
                if scenario == "storage-denied":
                    await context.add_init_script("""(() => {
                        const set = Storage.prototype.setItem;
                        Storage.prototype.setItem = function(key, value) {
                            if (key.startsWith('labello:build-reload:')) throw new Error('blocked');
                            return set.call(this, key, value);
                        };
                    })();""")
                page = await context.new_page()
                navigations = []
                page.on("framenavigated", lambda frame: navigations.append(True) if frame == page.main_frame else None)
                await page.goto(origin + "/?keep=fixture#retained")
                await ready(page)
                await until(lambda: state.build_checks >= 1)
                await page.evaluate("""async () => {
                    localStorage.setItem('fixture-preference', 'retained');
                    document.cookie = 'fixture-session=retained; SameSite=Lax';
                    await new Promise((resolve, reject) => {
                        const request = indexedDB.open('fixture-drafts', 1);
                        request.onupgradeneeded = () => request.result.createObjectStore('drafts');
                        request.onerror = reject;
                        request.onsuccess = () => {
                            const db = request.result;
                            const tx = db.transaction('drafts', 'readwrite');
                            tx.objectStore('drafts').put('retained', 'draft');
                            tx.oncomplete = () => {db.close(); resolve();};
                            tx.onerror = reject;
                        };
                    });
                }""")
                await page.mouse.click(610, 437)  # Pre-authentication About.
                await page.wait_for_timeout(400)
                await page.screenshot(path=args.artifacts / f"{scenario}-before.png")
                state.identity = "b"
                state.assets = "b" if scenario == "update" else "a"
                state.fail_assets = scenario == "network-failure"
                checks = state.build_checks
                await page.evaluate("window.dispatchEvent(new Event('focus'))")
                await until(lambda: state.build_checks > checks)
                if scenario in ("update", "persistent"):
                    try:
                        await until(lambda: len(navigations) == 2)
                    except AssertionError:
                        await page.mouse.click(610, 437)
                        await page.wait_for_timeout(1000)
                        await page.screenshot(path=args.artifacts / "failure-about.png")
                        print(json.dumps({"documents": state.documents, "checks": state.build_checks, "refreshes": state.refresh_requests}), flush=True)
                        raise
                    await ready(page)
                    assert await page.evaluate("location.search.includes('keep=fixture') && location.hash === '#retained'")
                    if scenario == "update":
                        assert "b" in state.wasm_served, "new WASM must execute after deployment"
                await page.wait_for_timeout(2000)
                assert len(navigations) == (2 if scenario in ("update", "persistent") else 1), "reload loop or unexpected navigation"
                preserved = await page.evaluate("""async () => {
                    const draft = await new Promise(resolve => {
                        const request = indexedDB.open('fixture-drafts', 1);
                        request.onsuccess = () => {
                            const db = request.result;
                            const read = db.transaction('drafts').objectStore('drafts').get('draft');
                            read.onsuccess = () => {db.close(); resolve(read.result);};
                        };
                    });
                    return localStorage.getItem('fixture-preference') === 'retained'
                        && document.cookie.includes('fixture-session=retained') && draft === 'retained';
                }""")
                assert preserved, "reload must preserve user storage"
                if scenario in ("update", "persistent"):
                    await page.mouse.click(610, 437)  # About after navigation.
                    await page.wait_for_timeout(400)
                await page.screenshot(path=args.artifacts / f"{scenario}-after.png")
                cdp = await context.new_cdp_session(page)
                tree = await cdp.send("Accessibility.getFullAXTree")
                assert any(node.get("role", {}).get("value") == "Canvas" for node in tree["nodes"])
                if scenario == "persistent":
                    matrix = [(w, h, d) for w, h in [(320, 568), (390, 844), (600, 800),
                              (1288, 820), (1440, 1000), (320, 320)] for d in (1, 2)] + [(390, 844, 3)]
                    for width, height, dpr in matrix:
                        visual_browser = await playwright.chromium.launch(headless=True, args=[
                            "--no-sandbox", "--enable-unsafe-swiftshader", f"--force-device-scale-factor={dpr}"])
                        visual = await visual_browser.new_context(viewport={"width": 1288, "height": 820}, device_scale_factor=dpr)
                        visual_page = await visual.new_page()
                        await visual_page.goto(origin)
                        await ready(visual_page)
                        await visual_page.wait_for_timeout(1500)
                        await ready(visual_page)
                        await visual_page.mouse.click(610, 437)
                        await visual_page.set_viewport_size({"width": width, "height": height})
                        await visual_page.wait_for_timeout(400)
                        assert await visual_page.evaluate("[innerWidth, innerHeight, devicePixelRatio]") == [width, height, dpr]
                        await visual_page.screenshot(path=args.artifacts / f"browser-{width}x{height}-dpr{dpr}.png")
                        await visual.close()
                        await visual_browser.close()
                    # Pointer activation of the production Retry app update button.
                    state.assets = "b"
                    await page.mouse.click(490, 567)
                    await until(lambda: len(navigations) == 3)
                    await ready(page)
                    assert "b" in state.wasm_served, "explicit retry must permit the completed deployment"
                results.append({"scenario": scenario, "navigations": len(navigations), "storage_preserved": preserved})
                await context.close()
            report = {"browser": browser.version, "viewport": [1288, 820], "dpr": 1, "zoom": 1,
                      "results": results, "cache_reload_requests": state.refresh_requests,
                      "limitations": "Synthetic identity API; native tests cover shared draft gating. Browser AX exposes canvas only."}
            (args.artifacts / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(report))
            await browser.close()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    asyncio.run(main())
