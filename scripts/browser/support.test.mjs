import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter, once } from "node:events";
import { rm } from "node:fs/promises";
import { PNG } from "pngjs";
import {
  capture,
  comparePng,
  diagnostics,
  matrix,
  fixture,
  cleanup,
  until,
  waitForSettingsPaint,
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

test("settings waits observe painted state without repeating input actions", async () => {
  const environment = await fixture("boxes");
  let captures = 0;
  const page = {
    url: () => environment.url,
    screenshot: async () => {
      const png = new PNG({ width: 4, height: 4 });
      const color = ++captures === 1 ? [29, 43, 68] : [17, 94, 89];
      for (let offset = 0; offset < png.data.length; offset += 4) {
        png.data.set([...color, 255], offset);
      }
      return PNG.sync.write(png);
    },
  };
  try {
    await waitForSettingsPaint(environment, page, "recording");
    assert.equal(captures, 2);
    for (const invalid of ["unknown", "__proto__", "toString"]) {
      await assert.rejects(
        waitForSettingsPaint(environment, page, invalid),
        /artifact.state/,
      );
    }
    await assert.rejects(
      waitForSettingsPaint({}, page, "recording"),
      /artifact.non_fixture/,
    );
    await assert.rejects(
      waitForSettingsPaint(
        environment,
        { url: () => "https://example.invalid" },
        "recording",
      ),
      /artifact.origin/,
    );
  } finally {
    await cleanup(environment);
  }
});

test("cleanup handles a fixture process that already exited from a signal", async () => {
  const environment = await fixture("boxes");
  const stopped = once(environment.process, "exit");
  environment.process.kill("SIGKILL");
  await stopped;
  assert.equal(environment.process.exitCode, null);
  assert.equal(environment.process.signalCode, "SIGKILL");
  const cleaned = cleanup(environment);
  try {
    assert.deepEqual(
      await Promise.race([
        cleaned,
        new Promise((_, reject) => {
          const timer = setTimeout(
            () => reject(new Error("signalled cleanup did not finish")),
            1000,
          );
          timer.unref();
        }),
      ]),
      { directoryRemoved: true, serverExited: true, webStopped: true },
    );
  } finally {
    // A regression must not strand the test fixture's listener.
    if (environment.web.listening) {
      environment.web.closeAllConnections();
      await new Promise((resolve) => environment.web.close(resolve));
    }
    await rm(environment.directory, { recursive: true, force: true });
  }
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
