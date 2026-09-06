import { resolve } from "node:path";
import { PNG } from "pngjs";
import {
  matrix,
  capture,
  ensure,
  until,
  comparePng,
  repository,
} from "./support.mjs";
import {
  frames,
  key,
  drag,
  start,
  enter,
  imageState,
  accessibility,
} from "./driver.mjs";

function assertCanvas(png) {
  const pixels = PNG.sync.read(png).data;
  let light = 0,
    dark = 0;
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i] === 220 && pixels[i + 1] === 220 && pixels[i + 2] === 220)
      light++;
    if (pixels[i] === 35 && pixels[i + 1] === 35 && pixels[i + 2] === 35)
      dark++;
  }
  ensure(light > 100 && dark > 100, "display.synthetic_canvas");
}

export async function display(page, environment, options) {
  const dpr = options.dpr;
  const samples = [];
  const { monitor, observation } = await start(page, environment);
  await enter(page, observation);
  for (const viewport of matrix.viewports.filter((view) =>
    view.dpr.includes(dpr),
  )) {
    await page.setViewportSize({
      width: viewport.width,
      height: viewport.height,
    });
    await frames(page, 15);
    const sample = await capture(
      environment,
      page,
      `matrix-${viewport.name}-${dpr}`,
      options,
    );
    const backingStore = await page.evaluate(() => {
      const canvas = document.querySelector("canvas");
      const box = canvas.getBoundingClientRect();
      return (
        Math.abs(canvas.width - Math.round(box.width * devicePixelRatio)) <=
          1 &&
        Math.abs(canvas.height - Math.round(box.height * devicePixelRatio)) <= 1
      );
    });
    ensure(backingStore, "display.backing_store");
    const workspace = await page.screenshot();
    assertCanvas(workspace);
    ensure(
      sample.display.width === viewport.width &&
        sample.display.height === viewport.height &&
        sample.display.dpr === dpr,
      "display.effective_size",
    );
    await key(page, "Control+,");
    const overlay = await capture(
      environment,
      page,
      `matrix-${viewport.name}-${dpr}-settings`,
      options,
    );
    ensure(
      comparePng(workspace, await page.screenshot()).ratio > 0.01,
      "display.overlay_open",
    );
    if (viewport.height <= 568) {
      const before = await page.screenshot();
      await page.mouse.move(viewport.width / 2, viewport.height / 2);
      await page.mouse.wheel(0, 1200);
      await frames(page, 15);
      ensure(
        comparePng(before, await page.screenshot()).ratio > 0.001,
        "display.scroll_reachable",
      );
    }
    await key(page, "Tab");
    await key(page, "Escape");
    await frames(page, 10);
    assertCanvas(await page.screenshot());
    samples.push({
      workspace: sample,
      overlay,
      accessibility: await accessibility(page),
    });
  }
  monitor.check();
  monitor.assertConsumed();
  return { workflow: "display", dpr, samples };
}

export const extensionPath = resolve(
  repository,
  "scripts/browser/zoom-extension",
);

export async function zoom(page, environment, options) {
  const { monitor, observation } = await start(page, environment);
  const assignment = await enter(page, observation);
  await drag(page, [430, 400], [650, 600]);
  await key(page, "Control+s");
  await until(
    async () =>
      Object.keys((await imageState(page, environment, assignment)).annotations)
        .length === 1,
    "zoom.annotation_save",
  );
  const context = page.context();
  const worker =
    context.serviceWorkers()[0] ||
    (await context.waitForEvent("serviceworker"));
  const factor = await worker.evaluate(async (origin) => {
    const tab = (await chrome.tabs.query({})).find((tab) =>
      tab.url?.startsWith(origin),
    );
    await chrome.tabs.setZoom(tab.id, 2);
    return chrome.tabs.getZoom(tab.id);
  }, environment.url);
  await frames(page, 20);
  const screenshot = await capture(
    environment,
    page,
    "annotation-zoom",
    options,
  );
  ensure(
    factor === 2 &&
      screenshot.display.width === 720 &&
      screenshot.display.height === 500,
    "zoom.actual_browser_reflow",
  );
  await key(page, "Space");
  await until(
    async () =>
      (await imageState(page, environment, assignment)).taskStates[
        assignment.taskId
      ].status === "submitted",
    "zoom.submit",
  );
  const fontSettings = await worker.evaluate(async (values) => {
    await chrome.fontSettings.setDefaultFontSize({
      pixelSize: values.defaultFontSize,
    });
    await chrome.fontSettings.setMinimumFontSize({
      pixelSize: values.minimumFontSize,
    });
    return {
      default: await chrome.fontSettings.getDefaultFontSize({}),
      minimum: await chrome.fontSettings.getMinimumFontSize({}),
    };
  }, matrix.largerText);
  ensure(
    fontSettings.default.pixelSize === 24 &&
      fontSettings.minimum.pixelSize === 20,
    "text.browser_setting",
  );
  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "zoom",
    factor,
    screenshot,
    largerText: {
      setting: "applied",
      canvas:
        "unsupported: Chromium font preferences do not scale egui canvas text; use native scale evidence",
    },
    accessibility: await accessibility(page),
  };
}
