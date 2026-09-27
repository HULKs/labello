#!/usr/bin/env python3
"""Check canvas mouse bindings in Chromium against disposable production data.

Build labello-server and the release WASM distribution first. Uses the same
Playwright/Pillow environment as stylus_input.py. Reports contain no payloads,
credentials, screenshots, or annotation geometry.
"""

import argparse
import asyncio
import json

from playwright.async_api import async_playwright
from stylus_input import Scenario, application, require, until


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dpr", type=float, default=1)
    parser.add_argument("--width", type=int, default=1288)
    parser.add_argument("--height", type=int, default=820)
    args = parser.parse_args()
    with application() as (origin, api, server):
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(args=[
                "--enable-unsafe-swiftshader", f"--force-device-scale-factor={args.dpr}",
            ])
            try:
                context = await browser.new_context(
                    viewport={"width": 1288, "height": 820}, device_scale_factor=args.dpr,
                )
                scenario = Scenario(context, origin, api)

                async def ready():
                    require(server.poll() is None, "server-startup-failed")
                    try:
                        return (await context.request.get(api + "/health", timeout=1000)).ok
                    except Exception:
                        return False

                await until(ready, "server-readiness-timeout")
                await scenario.seed("bounding_box")
                bindings = await scenario.request("GET", "/datasets/stylus/keybindings")
                bindings["bindings"]["delete_annotation"] = {
                    "key": "MouseRight", "ctrl": False, "shift": True,
                    "alt": False, "command": False,
                }
                saved = await scenario.request("PUT", "/datasets/stylus/keybindings", data=bindings)
                require(saved == bindings, "mouse-binding-api-round-trip")
                await scenario.open(args.width, args.height, False, "cdp", "chromium")
                page = scenario.page
                await page.evaluate("""() => {
                    window.mouseContextMenuPrevented = false;
                    document.addEventListener('contextmenu', event => {
                        setTimeout(() => { window.mouseContextMenuPrevented = event.defaultPrevented; }, 0);
                    }, true);
                }""")
                await scenario.gesture((0.25, 0.25), (0.6, 0.6), pointer="mouse")
                await scenario.annotation(1)
                x, y = scenario.point((0.4, 0.4))
                await page.mouse.click(x, y, button="right")
                await page.wait_for_timeout(1000)
                state = await scenario.state()
                require(not next(iter(state["annotations"].values()))[-1]["deleted"],
                        "modifier-required-for-mouse-binding")
                require(await page.evaluate("window.mouseContextMenuPrevented"),
                        "browser-context-menu-not-suppressed")
                await page.keyboard.down("Shift")
                await page.mouse.click(4, 4, button="right")
                await page.wait_for_timeout(1000)
                state = await scenario.state()
                require(not next(iter(state["annotations"].values()))[-1]["deleted"],
                        "mouse-binding-escaped-canvas")
                await page.mouse.click(x, y, button="right")
                await page.keyboard.up("Shift")

                async def deleted():
                    state = await scenario.state()
                    return next(iter(state["annotations"].values()))[-1]["deleted"]

                await until(deleted, "right-click-delete-not-saved")
                state = await scenario.state()
                versions = len(next(iter(state["annotations"].values())))
                await page.wait_for_timeout(1000)
                state = await scenario.state()
                require(len(next(iter(state["annotations"].values()))) == versions,
                        "mouse-release-or-hold-repeated-delete")
                require(not scenario.errors, "browser-pageerror")
                print(json.dumps({
                    "result": "passed", "browser": browser.version,
                    "viewport": [args.width, args.height], "dpr": args.dpr,
                    "checks": ["api-binding-round-trip", "modifier-match", "canvas-only",
                               "right-click-delete", "context-menu-suppressed", "single-delete"],
                }))
            finally:
                await browser.close()


if __name__ == "__main__":
    asyncio.run(asyncio.wait_for(main(), timeout=120))
