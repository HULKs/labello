#!/usr/bin/env python3
"""Check the built browser decoder's async boundary, pixels and bitmap cleanup."""
import argparse
import asyncio
import json
from pathlib import Path

from playwright.async_api import async_playwright
from stylus_input import application, require


async def run(browser_name, server_binary):
    root = Path(__file__).resolve().parents[3]
    dist = root / "apps/labello-wasm/dist"
    modules = [path for path in (dist / "snippets").rglob("*.js")
               if "export async function decodeWorkingPreview" in path.read_text()]
    require(len(modules) == 1, "built-preview-decoder-module")
    fixture = list((root / "crates/labello-client/src/demo/fixtures/data-saver.webp").read_bytes())
    with application(server_binary=server_binary) as (origin, _, _):
        async with async_playwright() as playwright:
            if browser_name == "firefox":
                browser = await playwright.firefox.launch(headless=False, firefox_user_prefs={
                    "webgl.force-enabled": True, "webgl.disabled": False,
                    "webgl.out-of-process": False,
                })
            else:
                browser = await playwright.chromium.launch(channel="chromium", args=["--enable-unsafe-swiftshader"])
            try:
                page = await browser.new_page()
                await page.goto(origin)
                await page.locator("#startup-status").wait_for(state="detached", timeout=30000)
                result = await page.evaluate("""async ({module, fixture}) => {
                    const {decodeWorkingPreview} = await import(module);
                    const original = globalThis.createImageBitmap;
                    let closed = 0, entered = false, release;
                    const held = new Promise(resolve => release = resolve);
                    globalThis.createImageBitmap = async (...args) => {
                        entered = true;
                        await held;
                        const bitmap = await original(...args);
                        const close = bitmap.close.bind(bitmap);
                        bitmap.close = () => { closed++; close(); };
                        return bitmap;
                    };
                    try {
                        let completed = false;
                        const pending = decodeWorkingPreview(new Uint8Array(fixture), 1, 1)
                            .then(pixels => { completed = true; return pixels; });
                        await new Promise(resolve => setTimeout(resolve, 0));
                        if (!entered || completed) throw new Error('decode did not yield');
                        release();
                        const pixels = await pending;
                        if (pixels.length !== 4 || pixels[3] !== 255 || closed !== 1)
                            throw new Error('decoded pixels or cleanup');
                        let rejected = 0;
                        for (const [bytes, width] of [[fixture, 2], [[0, 1, 2], 1]]) {
                            try { await decodeWorkingPreview(new Uint8Array(bytes), width, 1); }
                            catch (_) { rejected++; }
                        }
                        if (rejected !== 2 || closed !== 2)
                            throw new Error('decode failure or bitmap cleanup');
                        return {asyncBoundary: true, pixels: pixels.length, rejected, closed};
                    } finally { globalThis.createImageBitmap = original; }
                }""", {"module": "/" + modules[0].relative_to(dist).as_posix(), "fixture": fixture})
                print(json.dumps({"result": "passed", "browser": browser_name, "version": browser.version, **result}))
            finally:
                await browser.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--browser", choices=["chromium", "firefox"], default="chromium")
    parser.add_argument("--server", type=Path)
    args = parser.parse_args()
    asyncio.run(asyncio.wait_for(run(args.browser, args.server), timeout=60))
