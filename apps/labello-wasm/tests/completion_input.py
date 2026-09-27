#!/usr/bin/env python3
"""Check held completion keys against disposable production WASM/API work.

Uses the isolated server, synthetic fixture, and pinned Playwright dependencies
from stylus_input.py. Reports only browser settings and aggregate counts.
"""

import argparse
import asyncio
import io
import json

from PIL import Image
from playwright.async_api import async_playwright
from stylus_input import Scenario, application, png, require, until


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=["bounding_box", "skeleton"], default="bounding_box")
    parser.add_argument("--width", type=int, default=1288)
    parser.add_argument("--height", type=int, default=820)
    parser.add_argument("--dpr", type=float, default=1)
    args = parser.parse_args()
    with application() as (origin, api, server):
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(args=["--enable-unsafe-swiftshader", f"--force-device-scale-factor={args.dpr}"])
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
                await scenario.seed(args.kind)
                for index in range(3):
                    with Image.open(io.BytesIO(png())) as image:
                        image.putpixel((0, 0), (index, 0, 0))
                        data = io.BytesIO()
                        image.save(data, format="PNG")
                    await scenario.request(
                        "POST", "/datasets/stylus/uploads?root=uploads/stylus&ingest=true",
                        multipart={"files": {"name": f"fixture-{index}.png",
                                             "mimeType": "image/png", "buffer": data.getvalue()}},
                    )
                listing = await scenario.request("GET", "/datasets/stylus/images")
                paths = ["/datasets/stylus/images/" + item["image"]["imageId"]
                         for item in listing["items"]]
                require(len(paths) == 4, "fixture-image-count")
                await scenario.open(args.width, args.height, False, "cdp", "chromium")
                await scenario.page.evaluate("""() => {
                    window.completionKeys = {fresh: 0, repeats: 0, untrusted: 0};
                    document.addEventListener('keydown', event => {
                        if (event.code !== 'Space') return;
                        window.completionKeys[event.repeat ? 'repeats' : 'fresh']++;
                        if (!event.isTrusted) window.completionKeys.untrusted++;
                    }, true);
                }""")

                async def completed():
                    states = [await scenario.request("GET", path) for path in paths]
                    return sum(state["taskStates"].get(f"{args.kind}:fixture", {}).get("status")
                               == "completed" for state in states)

                async def count_is(expected):
                    return await completed() == expected

                require(await completed() == 0, "unexpected-initial-completion")
                await scenario.page.keyboard.down("Space")
                await until(lambda: count_is(1), "first-press-did-not-complete")
                # Playwright sends trusted CDP keydowns with autoRepeat while the
                # key remains held. Keep sending across the next image load.
                for _ in range(12):
                    await scenario.page.keyboard.down("Space")
                    await scenario.page.wait_for_timeout(100)
                    require(await completed() == 1, "held-key-completed-successive-image")
                await scenario.page.keyboard.up("Space")
                await scenario.page.keyboard.down("Space")
                await until(lambda: count_is(2), "release-repress-did-not-complete")
                for _ in range(4):
                    await scenario.page.keyboard.down("Space")
                    await scenario.page.wait_for_timeout(100)
                    require(await completed() == 2, "second-hold-completed-successive-image")
                await scenario.page.keyboard.up("Space")
                keys = await scenario.page.evaluate("window.completionKeys")
                require(keys == {"fresh": 2, "repeats": 16, "untrusted": 0}, "key-event-contract")
                require(not scenario.errors, "browser-page-error")
                print(json.dumps({"result": "passed", "browser": browser.version,
                                  "kind": args.kind, "viewport": [args.width, args.height],
                                  "dpr": args.dpr, "completed": 2, "keys": keys}))
            finally:
                await browser.close()


if __name__ == "__main__":
    asyncio.run(asyncio.wait_for(main(), timeout=120))
