#!/usr/bin/env python3
"""Exercise Objects, Overview and retained history through production WASM/API.

Uses the disposable server and private Playwright environment from stylus_input.
Reports aggregate results only; all credentials and annotation data stay in memory.
"""
import argparse
import asyncio
import io
import json
from pathlib import Path
from datetime import datetime, timezone
from urllib.parse import urlsplit

from PIL import Image
from playwright.async_api import async_playwright
from stylus_input import Scenario, application, png, require, until


async def run(kind, artifacts=None):
    with application() as (origin, api, server):
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(args=["--enable-unsafe-swiftshader"])
            try:
                context = await browser.new_context(viewport={"width": 1440, "height": 1000})
                scenario = Scenario(context, origin, api)

                async def ready():
                    require(server.poll() is None, "server-startup-failed")
                    try:
                        return (await context.request.get(api + "/health", timeout=1000)).ok
                    except Exception:
                        return False

                await until(ready, "server-readiness")
                # Publish the complete source fixture before background preparation starts.
                await scenario.seed(kind, enabled=False)
                with Image.open(io.BytesIO(png())) as fixture:
                    fixture.putpixel((0, 0), (1, 0, 0))
                    data = io.BytesIO()
                    fixture.save(data, format="PNG")
                await scenario.request("POST", "/datasets/stylus/uploads?root=uploads/stylus&ingest=true",
                                       multipart={"files": {"name": "second.png", "mimeType": "image/png", "buffer": data.getvalue()}})
                metadata = await scenario.request("GET", "/datasets/stylus/admin")
                task = metadata["tasks"][0]
                task["enabled"] = True
                task["review"] = {"workflow": "approval", "allowReviewerCorrections": True}
                listing = await scenario.request("GET", "/datasets/stylus/images")
                images = [entry["image"]["imageId"] for entry in listing["items"]]
                for image in images:
                    for index in range(2):
                        geometry = ({"type": "bounding_box", "geometry": {"x": 0.15 + index * 0.4, "y": 0.2, "width": 0.2, "height": 0.3}}
                                    if kind == "bounding_box" else {"type": "skeleton", "geometry": {"keypoints": [
                                        {"name": "center", "state": "visible", "point": {"x": 0.25 + index * 0.4, "y": 0.4}}]}})
                        now = datetime.now(timezone.utc).isoformat()
                        await scenario.request("POST", f"/datasets/stylus/images/{image}/admin/events", data={"schemaVersion": 3, "payload": {
                            "kind": "annotation_version_created", "annotation": {"annotationId": f"object-{index}", "version": 1,
                            "objectGroupId": None, "origin": {"origin": "native", "legacyV2": False}, "taskId": task["taskId"],
                            "classId": "fixture", "type": kind, "revisionSource": {"source": "human", "action": "authored"},
                            "geometry": geometry, "authorUserId": "admin", "createdAt": now, "updatedAt": now, "deleted": False},
                            "previous_version": None, "reason": None}})

                await scenario.request("PUT", "/datasets/stylus/admin", data={key: metadata[key] for key in
                    ["name", "imageRoots", "labelClasses", "tasks", "roleAssignments", "imbalance", "prelabelConfigs"]})

                displayed = []
                histories = {}
                history_requests = {}
                def record(request):
                    path = urlsplit(request.url).path
                    if path.endswith("/work-items/display"):
                        displayed.append(request.post_data_json)
                    elif path.endswith("/work-items/history"):
                        history_requests[request] = len(displayed)
                async def response_ready(response):
                    if response.request in history_requests and response.ok:
                        histories[history_requests.pop(response.request)] = await response.json()
                context.on("request", record)
                context.on("response", response_ready)
                await scenario.open(1440, 1000, False, "cdp", "chromium")
                page = scenario.page

                async def current():
                    if not displayed or len(displayed) not in histories:
                        return None
                    action = displayed[-1]
                    state = await scenario.request("GET", f'/datasets/stylus/images/{action["imageId"]}')
                    if action["assignmentId"] not in state.get("workflowSeen", {}):
                        return None
                    return action, state["workflowAssignments"][action["assignmentId"]], state

                async def next_display(previous_count):
                    if len(displayed) <= previous_count:
                        return None
                    result = await current()
                    if result:
                        await page.wait_for_timeout(250)
                    return result

                first = await until(current, "initial-item-not-displayed")
                require(first[1]["item"]["kind"] == "object", "initial-work-not-focused")
                if artifacts:
                    folder = Path(artifacts)
                    folder.mkdir(parents=True, exist_ok=True)
                    await page.screenshot(path=str(folder / f"objects-{kind}.png"), clip={"x": 0, "y": 110, "width": 335, "height": 330}, scale="css")
                visits = [first]
                for _ in range(2):
                    before = len(displayed)
                    await page.keyboard.press("Space")
                    visits.append(await until(lambda: next_display(before), "object-did-not-advance"))
                require(all(visit[1]["item"]["kind"] == "object" for visit in visits), "object-queue-entered-overview")
                require(len({visit[0]["imageId"] for visit in visits}) == 2, "objects-did-not-cross-images")
                for previous in [visits[1], visits[0]]:
                    before = len(displayed)
                    await page.keyboard.press("ArrowLeft")
                    reopened = await until(lambda: next_display(before), "previous-item-not-displayed")
                    require(reopened[0]["imageId"] == previous[0]["imageId"] and reopened[1]["item"] == previous[1]["item"], "history-order-changed")
                for forward in [visits[1], visits[2]]:
                    before = len(displayed)
                    await page.keyboard.press("Space")
                    reopened = await until(lambda: next_display(before), "forward-item-not-displayed")
                    require(reopened[0]["imageId"] == forward[0]["imageId"] and reopened[1]["item"] == forward[1]["item"], "forward-history-not-retained")

                # Finish the two remaining objects, leaving both image overviews pending.
                before = len(displayed)
                await page.keyboard.press("Space")
                await until(lambda: next_display(before), "last-object-not-displayed")
                await page.keyboard.press("Space")
                async def objects_finished():
                    states = [await scenario.request("GET", f"/datasets/stylus/images/{image}") for image in images]
                    confirmed = set()
                    for state in states:
                        for assignment, receipt in state.get("workflowConfirmations", {}).items():
                            item = state["workflowAssignments"][assignment]["item"]
                            if receipt["review"] is None and item["kind"] == "object":
                                confirmed.add((state["imageId"], json.dumps(item, sort_keys=True)))
                        require(all(state["workflowAssignments"][assignment]["item"]["kind"] == "object" for assignment in state.get("workflowSeen", {})), "automatic-overview-entry")
                        require(state["taskStates"][task["taskId"]]["status"] != "submitted", "objects-submitted-image")
                    return len(confirmed) == 4
                await until(objects_finished, "objects-not-confirmed")
                await page.wait_for_timeout(800)
                before = len(displayed)
                await page.mouse.click(170, 300)  # Overview in the single-class selector.
                overview = await until(lambda: next_display(before), "overview-not-selected")
                require(overview[1]["item"]["kind"] == "overview", "overview-selected-object")
                if artifacts:
                    await page.screenshot(path=str(Path(artifacts) / f"overview-{kind}.png"), clip={"x": 0, "y": 110, "width": 335, "height": 330}, scale="css")
                for index in range(2):
                    before = len(displayed)
                    await page.keyboard.press("Space")
                    if index == 0:
                        await until(lambda: next_display(before), "next-overview-not-displayed")
                async def submitted():
                    states = [await scenario.request("GET", f"/datasets/stylus/images/{image}") for image in images]
                    return all(state["taskStates"][task["taskId"]]["status"] == "submitted" for state in states)
                await until(submitted, "annotation-overviews-not-submitted")
                await page.wait_for_load_state("networkidle")
                await page.wait_for_timeout(500)
                before = len(displayed)
                await page.mouse.click(150, 28)  # Review in desktop navigation.
                review = await until(lambda: next_display(before), "review-object-not-displayed")
                require(review[0]["kind"] == "review" and review[1]["item"]["kind"] == "object", "review-not-focused")
                require(review[1]["reviewException"], "self-review-fallback-not-recorded")
                if artifacts:
                    await page.screenshot(path=str(Path(artifacts) / f"review-{kind}.png"), clip={"x": 0, "y": 110, "width": 335, "height": 330}, scale="css")
                before = len(displayed)
                await page.keyboard.press("Space")
                review = await until(lambda: next_display(before), "review-did-not-advance")
                require(review[1]["item"]["kind"] == "object", "review-entered-overview")
                require(not scenario.errors, "browser-page-error")
                print(json.dumps({"result": "passed", "browser": browser.version, "kind": kind,
                                  "images": 2, "objects": 4, "history": "C-B-A-B-C", "annotation_overviews": 2,
                                  "review": "focused advance and recorded fallback", "viewport": [1440, 1000]}))
            finally:
                if artifacts and hasattr(scenario, "page"):
                    await scenario.page.screenshot(path=str(Path(artifacts) / f"navigation-{kind}.png"), clip={"x": 0, "y": 0, "width": 1440, "height": 108}, scale="css")
                    await scenario.page.screenshot(path=str(Path(artifacts) / f"final-selector-{kind}.png"), clip={"x": 0, "y": 110, "width": 335, "height": 330}, scale="css")
                await browser.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=["bounding_box", "skeleton"], default="bounding_box")
    parser.add_argument("--artifacts", help="Capture only the workflow selector, excluding image content")
    args = parser.parse_args()
    asyncio.run(asyncio.wait_for(run(args.kind, args.artifacts), timeout=180))
