#!/usr/bin/env python3
"""Measure cold signed-in startup with disposable production API state.

Each run uses a fresh browser context with only the fixture's session and an
optional workspace preference. HTTP caches are isolated; the browser process is reused.
Credentials, payloads and screenshots stay in memory. Dataset readiness is the
first draw of the fixture's recommended-dataset button, followed by a real
selection check. Restored-image readiness is available as a separate endpoint.
"""

import argparse
import asyncio
import http.client
import json
from pathlib import Path
import statistics
import time
from urllib.parse import urlsplit

from playwright.async_api import async_playwright
from stylus_input import COLOR, QuietFiles, Scenario, application, require, until


PROBE = r"""(() => {
  const t = window.__startup = {fetches: []};
  const fetch = window.fetch;
  window.fetch = function(input, ...args) {
    const path = new URL(typeof input === 'string' || input instanceof URL ? input : input.url, location.href).pathname;
    const kind = path.endsWith('.wasm') ? 'wasm' : path.endsWith('labello.client.json') ? 'config'
      : path.endsWith('/auth/options') ? 'auth' : path.endsWith('/me') ? 'session'
      : path.endsWith('/datasets') ? 'datasets'
      : path.endsWith('/work-items/claim') ? 'claim'
      : path.endsWith('/work-items/display') ? 'display'
      : path.endsWith('/encoded-preview') || path.endsWith('/preview') ? 'preview'
      : /\/revalidate/.test(path) ? 'revalidate'
      : path.endsWith('/state') ? 'image-state'
      : path.endsWith('/reasons') ? 'image-reasons'
      : path.endsWith('/availability') ? 'availability'
      : path.endsWith('/stats') ? 'statistics'
      : path.endsWith('/preferences') ? 'preferences'
      : path.endsWith('/keybindings') ? 'keybindings'
      : path.endsWith('/presence') ? 'presence'
      : /^\/api\/datasets\/[^/]+$/.test(path) ? 'metadata'
      : /^\/api\/datasets\/[^/]+\/images\/[^/]+$/.test(path) ? 'image-record'
      : 'other';
    const entry = {kind, start: performance.now()}; t.fetches.push(entry);
    return fetch.call(this, input, ...args).then(response => {
      entry.end = performance.now(); entry.status = response.status; return response;
    });
  };
  for (const Class of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
    if (!Class) continue;
    const upload = Class.prototype.texImage2D;
    Class.prototype.texImage2D = function(...args) {
      const result = upload.apply(this, args);
      const bytes = args[8];
      if (args[3] > 100 && args[4] > 100 && bytes
          && Math.abs(bytes[0] - 187) < 8 && Math.abs(bytes[1] - 43) < 8
          && Math.abs(bytes[2] - 129) < 8)
        t.imageTexture = this.getParameter(this.TEXTURE_BINDING_2D);
      return result;
    };
    const draw = Class.prototype.drawElements;
    Class.prototype.drawElements = function(...args) {
      const result = draw.apply(this, args);
      if (!t.firstDraw) t.firstDraw = performance.now();
      if (window.__startupDatasetProbe && !t.datasetsReady
          && t.fetches.some(f => f.kind === 'datasets' && f.status === 200)) {
        // Inside the recommended-dataset button at the fixed 1440x1000/DPR1
        // benchmark viewport. Initial login/loading layouts have no button here.
        const pixel = new Uint8Array(4);
        this.readPixels(380, this.drawingBufferHeight - 201, 1, 1, this.RGBA, this.UNSIGNED_BYTE, pixel);
        if (Math.abs(pixel[0] - 45) < 8 && Math.abs(pixel[1] - 212) < 8
            && Math.abs(pixel[2] - 191) < 8)
          t.datasetsReady = performance.now();
      }
      if (!t.imageReady && t.imageTexture
          && this.getParameter(this.TEXTURE_BINDING_2D) === t.imageTexture)
        t.imageReady = performance.now();
      return result;
    };
  }
})();"""


class StartupFiles(QuietFiles):
    protocol_version = "HTTP/1.1"
    disable_nagle_algorithm = True
    backend = None
    options = None
    config_override = None
    fail_auth_options = False

    def do_GET(self):
        path = urlsplit(self.path).path
        if path.startswith("/api/"):
            return self.proxy()
        if path == "/labello.client.json":
            body = self.config_override or json.dumps({"apiBaseUrl": f"http://127.0.0.1:{self.server.server_port}/api/"}).encode()
            kind, encoding = "application/json", None
        else:
            asset = Path(self.translate_path(path if path != "/" else "/index.html"))
            if not asset.is_file():
                return self.send_error(404)
            kind, encoding = self.guess_type(str(asset)), None
            if asset.suffix in (".wasm", ".js") and self.options.encoding != "identity":
                encoding = self.options.encoding
                asset = Path(str(asset) + (".br" if encoding == "br" else ".gz"))
                require(asset.is_file(), "compressed-release-asset-missing")
            body = asset.read_bytes()
        self.respond(200, body, [("Content-Type", kind)], encoding, throttle=True)

    def do_POST(self):
        self.proxy()

    def do_PUT(self):
        self.proxy()

    def proxy(self):
        if self.fail_auth_options and urlsplit(self.path).path == "/api/auth/options":
            return self.respond(503, b'{"error":"fixture unavailable"}', [("Content-Type", "application/json")], None)
        backend = urlsplit(self.backend)
        connection = http.client.HTTPConnection(backend.hostname, backend.port, timeout=20)
        try:
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            headers = {key: value for key, value in self.headers.items()
                       if key.lower() not in ("host", "connection")}
            connection.request(self.command, self.path.removeprefix("/api"), body, headers)
            response = connection.getresponse()
            self.respond(response.status, response.read(), response.getheaders(), None)
        finally:
            connection.close()

    def respond(self, status, body, headers, encoding, throttle=False):
        time.sleep(self.options.latency_ms / 1000)
        self.send_response(status)
        for key, value in headers:
            if key.lower() not in ("content-length", "connection", "transfer-encoding", "cache-control"):
                self.send_header(key, value)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        if encoding:
            self.send_header("Content-Encoding", encoding)
        self.end_headers()
        try:
            for offset in range(0, len(body), 32768):
                chunk = body[offset:offset + 32768]
                if throttle and self.options.mbps:
                    time.sleep(len(chunk) * 8 / (self.options.mbps * 1_000_000))
                self.wfile.write(chunk)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", type=Path, default=Path("apps/labello-wasm/dist"))
    parser.add_argument("--browser", choices=["firefox", "chromium"], default="firefox")
    parser.add_argument("--headed", action="store_true")
    parser.add_argument("--encoding", choices=["br", "gzip", "identity"], default="br")
    parser.add_argument("--screen", choices=["datasets", "restored-image"], default="datasets")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--latency-ms", type=float, default=40)
    parser.add_argument("--mbps", type=float, default=50)
    parser.add_argument("--budget-ms", type=float, default=1000)
    args = parser.parse_args()
    probe = f"window.__startupDatasetProbe = {json.dumps(args.screen == 'datasets')};\n" + PROBE
    require(args.runs > 0 and args.mbps >= 0 and args.latency_ms >= 0, "invalid-benchmark-settings")
    StartupFiles.options = args
    with application(handler_type=StartupFiles, dist=args.dist.resolve()) as (origin, backend, server):
        StartupFiles.backend = backend
        api = origin + "/api/"
        async with async_playwright() as playwright:
            launch = {"headless": not args.headed}
            if args.browser == "firefox":
                launch["firefox_user_prefs"] = {"webgl.force-enabled": True,
                    "webgl.disabled": False, "webgl.out-of-process": False}
            else:
                launch["args"] = ["--no-sandbox", "--use-gl=angle", "--use-angle=swiftshader",
                    "--enable-unsafe-swiftshader"]
            browser = await getattr(playwright, args.browser).launch(**launch)
            try:
                context_options = {"viewport": {"width": 1440, "height": 1000}, "device_scale_factor": 1}
                seed_context = await browser.new_context(**context_options)
                await seed_context.add_init_script(probe)
                fixture = Scenario(seed_context, origin, api)

                async def ready():
                    require(server.poll() is None, "server-startup-failed")
                    try:
                        return (await seed_context.request.get(backend + "/health", timeout=1000)).ok
                    except Exception:
                        return False

                await until(ready, "server-readiness-timeout")
                await fixture.seed("bounding_box")
                if args.screen == "restored-image":
                    await fixture.open(1440, 1000, False, "cdp", args.browser)
                    await fixture.page.wait_for_timeout(300)
                state = await seed_context.storage_state()
                await seed_context.close()
                rows = []
                for _ in range(args.runs):
                    context = await browser.new_context(storage_state=state, **context_options)
                    await context.add_init_script(probe)
                    page = await context.new_page()
                    errors = []
                    page.on("pageerror", lambda _: errors.append("pageerror"))
                    await page.goto(origin + "/", wait_until="domcontentloaded")
                    try:
                        ready_key = "datasetsReady" if args.screen == "datasets" else "imageReady"
                        await page.wait_for_function(f"() => __startup.{ready_key}", timeout=30000)
                    except Exception:
                        print(json.dumps(await page.evaluate("() => ({fetches:__startup.fetches,first_draw_ms:__startup.firstDraw})")))
                        raise
                    scenario = Scenario(context, origin, api)
                    scenario.page = page
                    if args.screen == "datasets":
                        bounds = await scenario.color_bounds((45, 212, 191))
                        require(bounds and bounds[0] <= 380 < bounds[2] and bounds[1] <= 200 < bounds[3],
                                "dataset-button-not-visible")
                    else:
                        require(await scenario.color_bounds(COLOR), "restored-image-not-visible")
                    require(not errors, "browser-startup-error")
                    row = await page.evaluate("""() => ({
                      first_draw_ms: __startup.firstDraw, image_ready_ms: __startup.imageReady,
                      datasets_ready_ms: __startup.datasetsReady,
                      fetches: __startup.fetches,
                      wasm: performance.getEntriesByType('resource').filter(r => r.name.endsWith('.wasm'))
                        .map(r => ({start_ms:r.startTime, end_ms:r.responseEnd,
                          encoded_bytes:r.encodedBodySize, decoded_bytes:r.decodedBodySize}))
                    })""")
                    if args.screen == "datasets":
                        require(not row.get("image_ready_ms"), "dataset-list-restored-work")
                        await page.mouse.click(380, 200)
                        await page.wait_for_function(
                            "__startup.fetches.some(f => f.kind === 'metadata' && f.status === 200)",
                            timeout=10000)
                    rows.append(row)
                    await context.close()
                times = [row["datasets_ready_ms" if args.screen == "datasets" else "image_ready_ms"]
                         for row in rows]
                # An invalid config must retain the bounded startup error. A
                # failed auth-options request must reveal a retry screen even
                # when /me succeeds, without restoring account-scoped work.
                context = await browser.new_context(storage_state=state, **context_options)
                page = await context.new_page()
                StartupFiles.config_override = b'{"apiBaseUrl":"ftp://fixture.invalid/"}'
                await page.goto(origin + "/", wait_until="domcontentloaded")
                await page.locator('#startup-status[data-error="true"]').wait_for(timeout=10000)
                require("fixture.invalid" not in await page.locator("#startup-status").text_content(),
                        "config-error-discloses-url")
                StartupFiles.config_override = None
                StartupFiles.fail_auth_options = True
                await page.add_init_script(probe)
                await page.goto(origin + "/", wait_until="domcontentloaded")
                await page.locator("#startup-status").wait_for(state="detached", timeout=10000)
                await page.wait_for_timeout(100)
                require(not await page.evaluate("Boolean(__startup.imageReady || __startup.datasetsReady)"),
                        "failed-options-revealed-workspace")
                StartupFiles.fail_auth_options = False
                await context.close()
                signed_out = await browser.new_context(**context_options)
                page = await signed_out.new_page()
                await page.add_init_script(probe)
                await page.goto(origin + "/", wait_until="domcontentloaded")
                await page.locator("#startup-status").wait_for(state="detached", timeout=10000)
                await page.wait_for_function("__startup.fetches.some(f => f.kind === 'session' && f.status === 401)",
                                             timeout=10000)
                require(not await page.evaluate("Boolean(__startup.imageReady || __startup.datasetsReady)"),
                        "signed-out-revealed-workspace")
                await signed_out.close()
                report = {"browser": args.browser, "version": browser.version, "viewport": [1440, 1000],
                    "screen": args.screen,
                    "dpr": 1, "zoom": 1, "cold_contexts": args.runs, "encoding": args.encoding,
                    "browser_process_reused": True,
                    "mbps_per_response": args.mbps, "latency_ms": args.latency_ms,
                    "median_ms": round(statistics.median(times), 1), "max_ms": round(max(times), 1),
                    "budget_ms": args.budget_ms, "pass": statistics.median(times) < args.budget_ms,
                    "failure_cases": ["invalid-config", "auth-options-unavailable", "signed-out"],
                    "runs": rows}
                print(json.dumps(report))
                require(report["pass"], "signed-in-startup-budget-exceeded")
            finally:
                await browser.close()


if __name__ == "__main__":
    asyncio.run(asyncio.wait_for(main(), timeout=180))
