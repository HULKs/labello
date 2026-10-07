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
from navigation_latency import NavigationLatency, PROBE
from stylus_input import COLOR, Scenario, application, png, require, until


async def run(kind, artifacts=None, latency_budget_ms=None, server_binary=None):
    with application(server_binary=server_binary) as (origin, api, server):
        async with async_playwright() as playwright:
            browser = await playwright.chromium.launch(channel="chromium" if latency_budget_ms else None, args=["--enable-unsafe-swiftshader", "--use-gl=angle", "--use-angle=swiftshader"])
            try:
                context = await browser.new_context(viewport={"width": 1440, "height": 1000})
                if latency_budget_ms:
                    await context.add_init_script(PROBE)
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
                meter = NavigationLatency(page, latency_budget_ms) if latency_budget_ms else None
                phase = "annotation_objects"

                async def action(key, next_item=True):
                    if meter and next_item:
                        await meter.click(phase, "previous" if key == "ArrowLeft" else "submit")
                    else:
                        await page.keyboard.press(key)


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
                    await page.screenshot(path=str(folder / f"objects-{kind}.png"), clip={"x": 0, "y": 110, "width": 360, "height": 330}, scale="css")
                visits = [first]
                for _ in range(2):
                    before = len(displayed)
                    await action("Space")
                    visits.append(await until(lambda: next_display(before), "object-did-not-advance"))
                require(all(visit[1]["item"]["kind"] == "object" for visit in visits), "object-queue-entered-overview")
                require(len({visit[0]["imageId"] for visit in visits}) == 2, "objects-did-not-cross-images")
                for previous in [visits[1], visits[0]]:
                    before = len(displayed)
                    await action("ArrowLeft")
                    reopened = await until(lambda: next_display(before), "previous-item-not-displayed")
                    require(reopened[0]["imageId"] == previous[0]["imageId"] and reopened[1]["item"] == previous[1]["item"], "history-order-changed")
                for forward in [visits[1], visits[2]]:
                    before = len(displayed)
                    await action("Space")
                    reopened = await until(lambda: next_display(before), "forward-item-not-displayed")
                    require(reopened[0]["imageId"] == forward[0]["imageId"] and reopened[1]["item"] == forward[1]["item"], "forward-history-not-retained")

                # Finish the two remaining objects, leaving both image overviews pending.
                before = len(displayed)
                await action("Space")
                await until(lambda: next_display(before), "last-object-not-displayed")
                await action("Space", next_item=False)
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
                await page.mouse.click(206, 200)  # Overview beside Objects in the single-class selector.
                overview = await until(lambda: next_display(before), "overview-not-selected")
                require(overview[1]["item"]["kind"] == "overview", "overview-selected-object")
                if artifacts:
                    await page.screenshot(path=str(Path(artifacts) / f"overview-{kind}.png"), clip={"x": 0, "y": 110, "width": 360, "height": 330}, scale="css")
                phase = "annotation_overview"
                # Revisions made in Overview must survive autosave without reopening Objects.
                await page.wait_for_timeout(600)
                scenario.bounds = await until(lambda: scenario.color_bounds(COLOR), "overview-image-not-rendered")
                original = overview[2]["annotations"]["object-0"][-1]
                await scenario.gesture((0.25, 0.35) if kind == "bounding_box" else (0.25, 0.4),
                                       (0.30, 0.40) if kind == "bounding_box" else (0.30, 0.45), pointer="mouse")
                async def overview_edit_saved():
                    state = await scenario.request("GET", f'/datasets/stylus/images/{overview[0]["imageId"]}')
                    draft = state.get("workflowEditDrafts", {}).get(overview[0]["assignmentId"], {})
                    return any(change.get("annotation_id") == "object-0" for change in draft.get("edits", {}).get("changes", []))
                await until(overview_edit_saved, "overview-edit-not-autosaved")
                for index in range(2):
                    before = len(displayed)
                    await action("Space", next_item=index == 0)
                    if index == 0:
                        await until(lambda: next_display(before), "next-overview-not-displayed")
                        if meter:
                            for key in ["ArrowLeft", "Space"]:
                                before = len(displayed)
                                await action(key)
                                await until(lambda: next_display(before), "annotation-overview-return-did-not-advance")
                async def submitted():
                    states = [await scenario.request("GET", f"/datasets/stylus/images/{image}") for image in images]
                    return all(state["taskStates"][task["taskId"]]["status"] == "submitted" for state in states)
                await until(submitted, "annotation-overviews-not-submitted")
                final_state = await scenario.request("GET", f'/datasets/stylus/images/{overview[0]["imageId"]}')
                revised = final_state["annotations"]["object-0"][-1]
                require(revised["version"] == original["version"] + 1 and revised["geometry"] != original["geometry"],
                        "overview-edit-not-published-once")
                await page.wait_for_load_state("networkidle")
                await page.wait_for_timeout(500)
                before = len(displayed)
                await page.mouse.click(150, 28)  # Review in desktop navigation.
                review = await until(lambda: next_display(before), "review-object-not-displayed")
                require(review[0]["kind"] == "review" and review[1]["item"]["kind"] == "object", "review-not-focused")
                require(review[1]["reviewException"], "self-review-fallback-not-recorded")
                phase = "review_objects"
                if artifacts:
                    await page.screenshot(path=str(Path(artifacts) / f"review-{kind}.png"), clip={"x": 0, "y": 110, "width": 360, "height": 330}, scale="css")
                before = len(displayed)
                await action("Space")
                review = await until(lambda: next_display(before), "review-did-not-advance")
                require(review[1]["item"]["kind"] == "object", "review-entered-overview")
                if meter:
                    for key in ["ArrowLeft", "Space"]:
                        before = len(displayed)
                        await action(key)
                        await until(lambda: next_display(before), "review-object-return-did-not-advance")
                for index in range(3):
                    before = len(displayed)
                    await action("Space", next_item=index < 2)
                    if index < 2:
                        await until(lambda: next_display(before), "next-review-object-not-displayed")
                async def objects_reviewed():
                    states = [await scenario.request("GET", f"/datasets/stylus/images/{image}") for image in images]
                    return sum(1 for state in states for receipt in state.get("workflowConfirmations", {}).values()
                               if receipt.get("review")) == 4
                await until(objects_reviewed, "object-reviews-not-confirmed")
                await page.wait_for_timeout(800)
                before = len(displayed)
                await page.mouse.click(206, 200)
                review = await until(lambda: next_display(before), "review-overview-not-selected")
                require(review[1]["item"]["kind"] == "overview", "review-overview-selected-object")
                phase = "review_overview"
                for index in range(2):
                    before = len(displayed)
                    if index == 0:
                        await action("Space")
                        await until(lambda: next_display(before), "next-review-overview-not-displayed")
                        if meter:
                            for key in ["ArrowLeft", "Space"]:
                                before = len(displayed)
                                await action(key)
                                await until(lambda: next_display(before), "review-overview-return-did-not-advance")
                    else:
                        await page.mouse.click(1380, 968)  # Visible Approve action in the bottom bar.
                async def completed():
                    states = [await scenario.request("GET", f"/datasets/stylus/images/{image}") for image in images]
                    return all(state["taskStates"][task["taskId"]]["status"] == "completed" for state in states)
                await until(completed, "review-overviews-not-completed")
                stats = await scenario.request("GET", "/datasets/stylus/stats")
                days = [day for person in stats["contributors"].values() for day in person["history"]]
                require(sum(day["labeled"] for day in days) == 6, "annotation-item-streak-count")
                require(sum(day["reviewed"] for day in days) == 6, "review-item-streak-count")
                require(not scenario.errors, "browser-page-error")
                if meter:
                    print(json.dumps({"kind": kind, "latencySamples": meter.samples}), flush=True)
                    require(meter.within_budget(), "navigation-exceeds-latency-budget")
                print(json.dumps({"result": "passed", "browser": browser.version, "kind": kind,
                                  "images": 2, "objects": 4, "history": "C-B-A-B-C", "annotation_overviews": 2, "overview_edit_autosave": True,
                                  "review": "4 Objects and 2 Overview, shortcut and button", "streak_units": {"annotation": 6, "review": 6}, "viewport": [1440, 1000]}))
            finally:
                if artifacts and hasattr(scenario, "page"):
                    await scenario.page.screenshot(path=str(Path(artifacts) / f"navigation-{kind}.png"), clip={"x": 0, "y": 0, "width": 1440, "height": 108}, scale="css")
                    await scenario.page.screenshot(path=str(Path(artifacts) / f"final-selector-{kind}.png"), clip={"x": 0, "y": 110, "width": 360, "height": 330}, scale="css")
                await browser.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=["bounding_box", "skeleton"], default="bounding_box")
    parser.add_argument("--artifacts", help="Capture only the workflow selector, excluding image content")
    parser.add_argument("--latency-budget-ms", type=float, help="Measure visible buttons through next-item rendering and interaction readiness")
    parser.add_argument("--server", type=Path, help="Server binary; use a release build for latency measurements")
    args = parser.parse_args()
    if args.latency_budget_ms is not None and args.latency_budget_ms <= 0:
        parser.error("--latency-budget-ms must be positive")
    asyncio.run(asyncio.wait_for(run(args.kind, args.artifacts, args.latency_budget_ms, args.server), timeout=180))
