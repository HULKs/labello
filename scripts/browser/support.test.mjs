import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PNG } from "pngjs";
import {
  capture,
  comparePng,
  diagnostics,
  matrix,
  fixture,
  cleanup,
  until,
} from "./support.mjs";

test("artifacts refuse an arbitrary environment before accessing the page", async () => {
  let touched = false;
  await assert.rejects(
    capture(
      {},
      {
        screenshot: () => {
          touched = true;
        },
      },
      "login-wide",
    ),
    /artifact.non_fixture/,
  );
  assert.equal(touched, false);
});

test("admitted fixtures reject unknown states and foreign origins before capture", async () => {
  const environment = await fixture("boxes");
  let captured = false;
  const page = {
    url: () => "https://example.invalid",
    screenshot: () => {
      captured = true;
    },
  };
  try {
    await assert.rejects(
      capture(environment, page, "../private"),
      /artifact.state/,
    );
    await assert.rejects(
      capture(environment, page, "annotation-wide"),
      /artifact.origin/,
    );
    assert.equal(captured, false);
  } finally {
    assert.deepEqual(await cleanup(environment), {
      directoryRemoved: true,
      serverExited: true,
      webStopped: true,
    });
  }
  await assert.rejects(
    capture(environment, page, "annotation-wide"),
    /artifact.non_fixture/,
  );
});

test("visual comparison detects a paint-only mismatch and dimension changes", () => {
  const baseline = new PNG({ width: 20, height: 20 });
  baseline.data.fill(255);
  const actual = PNG.sync.read(PNG.sync.write(baseline));
  actual.data.fill(0, 0, 20 * 10 * 4);
  assert.ok(
    comparePng(PNG.sync.write(actual), PNG.sync.write(baseline)).ratio > 0.4,
  );
  assert.equal(
    comparePng(PNG.sync.write(baseline), PNG.sync.write(baseline)).ratio,
    0,
  );
  assert.throws(
    () =>
      comparePng(
        PNG.sync.write(baseline),
        PNG.sync.write(new PNG({ width: 1, height: 1 })),
      ),
    /visual.dimensions/,
  );
});

test("browser diagnostics discard raw content and allow only counted expected failures", () => {
  const page = new EventEmitter();
  const monitor = diagnostics(page);
  monitor.expect("http:/auth/session:401");
  page.emit("response", {
    status: () => 401,
    url: () => "http://127.0.0.1/auth/session?private=must-not-appear",
  });
  monitor.check();
  monitor.assertConsumed();
  page.emit("response", {
    status: () => 401,
    url: () => "http://127.0.0.1/auth/session?private=must-not-appear",
  });
  assert.throws(() => monitor.check(), /browser.unexpected_failure/);
  assert.equal(monitor.counts.http, 1);
  assert.ok(!JSON.stringify(monitor.counts).includes("must-not-appear"));
  page.emit("pageerror", new Error("must-not-appear"));
  page.emit("console", { type: () => "error", text: () => "must-not-appear" });
  assert.deepEqual(monitor.counts, {
    http: 1,
    console: 1,
    startup: 1,
    network: 0,
  });
});

test("missing expected failures cannot masquerade as exercised failure coverage", () => {
  const monitor = diagnostics(new EventEmitter());
  monitor.expect("network:/fixture");
  assert.throws(
    () => monitor.assertConsumed(),
    /browser.expected_failure_missing/,
  );
});

test("readiness is bounded and fixture selection is closed", async () => {
  await assert.rejects(
    until(() => false, "readiness.timeout", 1),
    /readiness.timeout/,
  );
  await assert.rejects(fixture("../../existing-data"), /fixture.scenario/);
});

test("the maintained matrix includes short, adjacent layouts, high DPR, and real zoom", () => {
  assert.deepEqual(
    matrix.viewports.map((v) => [v.width, v.height]),
    [
      [320, 568],
      [390, 844],
      [600, 800],
      [1288, 820],
      [1440, 1000],
      [320, 320],
    ],
  );
  assert.ok(
    matrix.viewports.every((v) => v.dpr.includes(1) && v.dpr.includes(2)),
  );
  assert.ok(matrix.viewports.find((v) => v.name === "mobile").dpr.includes(3));
  assert.ok(matrix.browserZoom.includes(2));
});
