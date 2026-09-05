import { createHash } from "node:crypto";
import { createServer } from "node:http";
import {
  mkdtemp,
  readFile,
  rm,
  writeFile,
  mkdir,
  realpath,
  access,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve, extname, sep } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";
import pixelmatch from "pixelmatch";

export const repository = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../..",
);
export const matrix = JSON.parse(
  await readFile(new URL("./matrix.json", import.meta.url)),
);
const admitted = new WeakSet();
const states = new Set([
  "login-wide",
  "login-compact",
  "settings-wide",
  "settings-compact",
  "annotation-wide",
  "annotation-compact",
  "review-wide",
  "review-compact",
  "migration-wide",
  "migration-compact",
  "annotation-zoom",
  ...matrix.viewports.flatMap((view) =>
    view.dpr.flatMap((dpr) => [
      `matrix-${view.name}-${dpr}`,
      `matrix-${view.name}-${dpr}-settings`,
    ]),
  ),
]);

export class GateError extends Error {
  constructor(category) {
    super(category);
    this.category = category;
  }
}

export function ensure(condition, category) {
  if (!condition) throw new GateError(category);
}

export async function until(check, category, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new GateError(category);
}

export async function command(executable, args, options = {}) {
  const child = spawn(executable, args, {
    cwd: repository,
    stdio: "ignore",
    ...options,
  });
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new GateError("command.timeout"));
    }, 120_000);
    child.once("error", () => {
      clearTimeout(timer);
      reject(new GateError("command.start"));
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      code === 0 ? resolve() : reject(new GateError("command.failed"));
    });
  });
}

async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return server.address().port;
}

export async function fixture(scenario) {
  ensure(
    ["boxes", "skeletons", "review", "migration"].includes(scenario),
    "fixture.scenario",
  );
  const directory = await mkdtemp(resolve(tmpdir(), "labello-browser-"));
  const environment = { directory, scenario, process: null, web: null };
  try {
    const target = resolve(
      repository,
      process.env.CARGO_TARGET_DIR || "target",
    );
    await command(resolve(target, "debug/examples/browser_fixture"), [
      resolve(directory, "datasets"),
      scenario,
    ]);
    const provenance = JSON.parse(
      await readFile(resolve(directory, "datasets/synthetic-fixture.json")),
    );
    ensure(
      provenance.fixture === "labello-browser-v1" &&
        provenance.scenario === scenario &&
        provenance.images === 4 &&
        /^[a-f0-9]{64}$/.test(provenance.sourceDigest),
      "fixture.provenance",
    );
    const reserve = createServer();
    const apiPort = await listen(reserve);
    await new Promise((resolve) => reserve.close(resolve));
    environment.api = `http://127.0.0.1:${apiPort}`;
    const dist = await realpath(resolve(repository, "apps/labello-wasm/dist"));
    environment.web = createServer(async (request, response) => {
      try {
        const pathname = new URL(request.url, "http://127.0.0.1").pathname;
        if (pathname === "/labello.client.json") {
          response.writeHead(200, {
            "content-type": "application/json",
            "cache-control": "no-store",
          });
          response.end(JSON.stringify({ apiBaseUrl: environment.api }));
          return;
        }
        const file = await realpath(
          resolve(
            dist,
            `.${pathname === "/" ? "/index.html" : decodeURIComponent(pathname)}`,
          ),
        );
        ensure(file.startsWith(dist + sep), "assets.path");
        const types = {
          ".html": "text/html",
          ".js": "text/javascript",
          ".wasm": "application/wasm",
          ".svg": "image/svg+xml",
          ".ttf": "font/ttf",
          ".json": "application/json",
        };
        response.writeHead(200, {
          "content-type": types[extname(file)] || "application/octet-stream",
          "cache-control": "no-store",
        });
        response.end(await readFile(file));
      } catch {
        response.writeHead(404);
        response.end();
      }
    });
    environment.url = `http://127.0.0.1:${await listen(environment.web)}`;
    const config = resolve(directory, "server.toml");
    await writeFile(
      config,
      `bind = "127.0.0.1:${apiPort}"\ndatasetsRoot = ${JSON.stringify(resolve(directory, "datasets"))}\nbootstrapAdmins = ["synthetic-admin"]\nbrowserOrigins = [${JSON.stringify(environment.url)}]\nsessionCookieSecure = false\n[developmentAuth]\nlocalAdminLogin = true\n`,
    );
    // No inherited OAuth credentials, production configuration, or raw server logs.
    environment.process = spawn(resolve(target, "debug/labello-server"), [], {
      cwd: directory,
      stdio: "ignore",
      env: { PATH: process.env.PATH, LABELLO_CONFIG: config, RUST_LOG: "off" },
    });
    environment.process.once("error", () => {
      environment.startFailed = true;
    });
    await until(async () => {
      ensure(
        !environment.startFailed && environment.process.exitCode === null,
        "server.start",
      );
      try {
        return (
          await fetch(`${environment.api}/health`, {
            signal: AbortSignal.timeout(1000),
          })
        ).ok;
      } catch {
        return false;
      }
    }, "server.readiness");
    admitted.add(environment);
    return environment;
  } catch (error) {
    await cleanup(environment);
    throw error instanceof GateError ? error : new GateError("fixture.start");
  }
}

export async function cleanup(environment) {
  admitted.delete(environment);
  if (
    environment.process &&
    environment.process.exitCode === null &&
    environment.process.signalCode === null
  ) {
    environment.process.kill("SIGINT");
    await new Promise((resolve) => {
      const timer = setTimeout(() => environment.process.kill("SIGKILL"), 3000);
      environment.process.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }
  if (environment.web) {
    environment.web.closeAllConnections();
    await new Promise((resolve) => environment.web.close(resolve));
  }
  await rm(environment.directory, { recursive: true, force: true });
  const removed = await access(environment.directory).then(
    () => false,
    () => true,
  );
  ensure(
    removed &&
      !environment.web?.listening &&
      (!environment.process ||
        environment.process.exitCode !== null ||
        environment.process.signalCode !== null),
    "cleanup.incomplete",
  );
  return { directoryRemoved: removed, serverExited: true, webStopped: true };
}

export function diagnostics(page) {
  const counts = { console: 0, startup: 0, network: 0, http: 0 };
  const expected = new Map();
  const accept = (key) => {
    const count = expected.get(key) || 0;
    if (count > 0) {
      expected.set(key, count - 1);
      return true;
    }
    return false;
  };
  page.on("console", (message) => {
    if (message.type() === "error" && !accept("console")) counts.console++;
  });
  page.on("pageerror", () => {
    if (!accept("startup")) counts.startup++;
  });
  page.on("requestfailed", (request) => {
    const path = new URL(request.url()).pathname;
    if (!accept(`network:${path}`)) counts.network++;
  });
  page.on("response", (response) => {
    const path = new URL(response.url()).pathname;
    if (
      response.status() >= 400 &&
      !accept(`http:${path}:${response.status()}`)
    )
      counts.http++;
  });
  return {
    counts,
    expect(key, count = 1) {
      expected.set(key, (expected.get(key) || 0) + count);
    },
    check() {
      ensure(
        Object.values(counts).every((value) => value === 0),
        "browser.unexpected_failure",
      );
    },
    assertConsumed() {
      ensure(
        [...expected.values()].every((value) => value === 0),
        "browser.expected_failure_missing",
      );
    },
  };
}

export function comparePng(actual, baseline) {
  const a = PNG.sync.read(actual);
  const b = PNG.sync.read(baseline);
  ensure(a.width === b.width && a.height === b.height, "visual.dimensions");
  const diff = new PNG({ width: a.width, height: a.height });
  const different = pixelmatch(a.data, b.data, diff.data, a.width, a.height, {
    threshold: 0.15,
  });
  return {
    ratio: different / (a.width * a.height),
    diff: PNG.sync.write(diff),
  };
}

// Canvas widgets have no browser AX names. Wait for the actual settings paint
// before the next action, rather than assuming a compositor/rAF round trip
// means egui has consumed the preceding input. These pixels are discarded.
export async function waitForSettingsPaint(environment, page, phase) {
  ensure(admitted.has(environment), "artifact.non_fixture");
  ensure(new URL(page.url()).origin === environment.url, "artifact.origin");
  const expected = new Map([
    ["recording", { x: 865, y: 325, color: [17, 94, 89] }],
    ["save-enabled", { x: 965, y: 792, color: [45, 212, 191] }],
  ]).get(phase);
  ensure(expected, "artifact.state");
  await until(async () => {
    ensure(new URL(page.url()).origin === environment.url, "artifact.origin");
    const png = PNG.sync.read(
      await page.screenshot({
        clip: { x: expected.x, y: expected.y, width: 4, height: 4 },
      }),
    );
    let matching = 0;
    for (let offset = 0; offset < png.data.length; offset += 4) {
      if (
        expected.color.every(
          (value, channel) => Math.abs(png.data[offset + channel] - value) <= 2,
        )
      ) matching++;
    }
    return matching >= png.width * png.height * 0.9;
  }, `keybindings.${phase}_paint`);
}

export async function capture(
  environment,
  page,
  state,
  { update = false, injectMismatch = false } = {},
) {
  ensure(admitted.has(environment), "artifact.non_fixture");
  ensure(states.has(state), "artifact.state");
  ensure(new URL(page.url()).origin === environment.url, "artifact.origin");
  await page.mouse.move(1, page.viewportSize().height - 1);
  await page.evaluate(async () => {
    for (let i = 0; i < 15; i++) await new Promise(requestAnimationFrame);
  });
  // Connection setup displays a runtime URL. Only its static title/action area
  // is admitted; workspace states contain exclusively synthetic fixture data.
  const clip = state.startsWith("login-")
    ? { x: 0, y: 0, width: page.viewportSize().width, height: 150 }
    : undefined;
  const png = await page.screenshot({ animations: "disabled", clip });
  const artifact = resolve(
    repository,
    "scripts/browser/artifacts",
    `${state}.png`,
  );
  await mkdir(dirname(artifact), { recursive: true });
  await writeFile(artifact, png);
  const display = await page.evaluate(() => ({
    width: innerWidth,
    height: innerHeight,
    dpr: devicePixelRatio,
  }));
  const result = {
    state,
    sha256: createHash("sha256").update(png).digest("hex"),
    display,
  };
  if (state.startsWith("matrix-") || state === "annotation-zoom") return result;
  const baseline = resolve(
    repository,
    "scripts/browser/baselines",
    `${state}.png`,
  );
  if (update) {
    ensure(!process.env.CI, "visual.ci_update_forbidden");
    await mkdir(dirname(baseline), { recursive: true });
    await writeFile(baseline, png);
  } else {
    let reference;
    try {
      reference = await readFile(baseline);
    } catch {
      throw new GateError("visual.baseline_missing");
    }
    const actual = PNG.sync.read(png);
    if (injectMismatch)
      actual.data.fill(0, 0, Math.floor(actual.data.length / 4));
    const comparison = comparePng(PNG.sync.write(actual), reference);
    if (comparison.ratio > 0.003) {
      await writeFile(
        resolve(repository, "scripts/browser/artifacts", `${state}-diff.png`),
        comparison.diff,
      );
      throw new GateError("visual.mismatch");
    }
  }
  return result;
}
