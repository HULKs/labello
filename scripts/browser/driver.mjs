import { ensure, until, diagnostics } from "./support.mjs";

export async function frames(page, count = 3) {
  await page.evaluate(async (count) => {
    for (let i = 0; i < count; i++) await new Promise(requestAnimationFrame);
  }, count);
}

// egui consumes input per animation frame. Keep press/release in distinct
// frames; dispatching both in a single frame can miss a canvas interaction.
async function paintedFrame(page) {
  // Finish a compositor round trip before the next action. Animation-frame
  // callbacks alone can race canvas updates. Discard the static corner pixel.
  await page.screenshot({ clip: { x: 0, y: 0, width: 1, height: 1 } });
}

export async function click(page, x, y) {
  await page.mouse.move(x, y);
  await frames(page);
  await page.mouse.down();
  await frames(page);
  await page.mouse.up();
  await frames(page);
  await paintedFrame(page);
}

export async function key(page, value) {
  const keys = value.split("+");
  for (const part of keys) await page.keyboard.down(part);
  await frames(page);
  for (const part of keys.reverse()) await page.keyboard.up(part);
  await frames(page);
  await paintedFrame(page);
}

export async function drag(page, from, to, button = "left") {
  await page.mouse.move(...from);
  await frames(page);
  await page.mouse.down({ button });
  await frames(page);
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(
      from[0] + ((to[0] - from[0]) * i) / 8,
      from[1] + ((to[1] - from[1]) * i) / 8,
    );
    await frames(page, 2);
  }
  await page.mouse.up({ button });
  await frames(page);
}

export async function read(page, environment, path) {
  ensure(
    /^\/(me|datasets\/browser-fixture\/(images\/[a-zA-Z0-9_-]+|keybindings|admin))$/.test(
      path,
    ),
    "observation.path",
  );
  return page.evaluate(
    async ({ api, path }) => {
      const response = await fetch(api + path, { credentials: "include" });
      if (!response.ok) return null;
      return response.json();
    },
    { api: environment.api, path },
  );
}

export function observe(page) {
  const assignments = [];
  const counters = {
    assignments: 0,
    saves: 0,
    keybindings: 0,
    admin: 0,
    corrections: 0,
    reviews: 0,
    previews: 0,
    releases: 0,
    auth: 0,
    authOptions: 0,
    datasets: 0,
    session: 0,
  };
  const jobs = new Set();
  page.on("response", (response) => {
    const job = (async () => {
      await response.finished();
      const path = new URL(response.url()).pathname;
      if (path === "/me") counters.session++;
      if (!response.ok()) return;
      if (path === "/auth/options") counters.authOptions++;
      if (path === "/datasets") counters.datasets++;
      if (path.endsWith("/images/next")) {
        const assignment = await response.json();
        if (assignment) {
          assignments.push(assignment);
          counters.assignments++;
        }
      }
      if (path.endsWith("/preview")) counters.previews++;
      if (path === "/auth/local-admin") counters.auth++;
      if (response.request().method() === "GET") return;
      for (const [suffix, name] of [
        ["/annotation-batch", "saves"],
        ["/keybindings", "keybindings"],
        ["/admin", "admin"],
        ["/corrections", "corrections"],
        ["/reviews", "reviews"],
        ["/release", "releases"],
      ]) {
        if (path.endsWith(suffix)) counters[name]++;
      }
    })();
    jobs.add(job);
    job.finally(() => jobs.delete(job)).catch(() => {});
  });
  return {
    assignments,
    counters,
    async settled() {
      await Promise.all(jobs);
    },
  };
}

export async function start(page, environment) {
  const monitor = diagnostics(page);
  const observation = observe(page);
  monitor.expect("http:/me:401");
  monitor.expect("console");
  await page.goto(environment.url + "/?dataset=browser-fixture");
  await until(async () => {
    ensure(
      (await page.locator('#startup-status[data-error="true"]').count()) === 0,
      "wasm.startup_failure",
    );
    return (await page.locator("#startup-status").count()) === 0;
  }, "wasm.startup");
  await until(
    () =>
      observation.counters.authOptions > 0 && observation.counters.session > 0,
    "auth.discovery",
  );
  await frames(page, 15);
  const compact = page.viewportSize().width < 600;
  await click(
    page,
    compact ? 100 : page.viewportSize().width / 2 - 255,
    compact ? 452 : 334,
  );
  await until(() => {
    monitor.check();
    return observation.counters.auth === 1;
  }, "auth.local_login");
  ensure(await read(page, environment, "/me"), "auth.credentialed_session");
  await until(() => observation.counters.datasets > 0, "datasets.discovery");
  await frames(page, 15);
  return { monitor, observation };
}

export async function enter(page, observation, review = false) {
  ensure(
    page.viewportSize().width >= 600 || !review,
    "driver.compact_review_entry",
  );
  await click(
    page,
    page.viewportSize().width < 600
      ? 100
      : page.viewportSize().width / 2 - (review ? 216 : 195),
    page.viewportSize().width < 600 ? 420 : review ? 520 : 290,
  );
  await until(
    () =>
      observation.assignments.length > 0 && observation.counters.previews > 0,
    "workflow.assignment",
  );
  await frames(page, 15);
  return observation.assignments[0];
}

export function imageState(page, environment, assignment) {
  return read(
    page,
    environment,
    "/datasets/browser-fixture/images/" + assignment.imageId,
  );
}

export async function accessibility(page) {
  const session = await page.context().newCDPSession(page);
  try {
    const tree = await session.send("Accessibility.getFullAXTree");
    const roles = {};
    for (const node of tree.nodes.filter((node) => !node.ignored)) {
      const role = node.role?.value;
      if (
        [
          "RootWebArea",
          "Canvas",
          "textbox",
          "generic",
          "button",
          "dialog",
        ].includes(role)
      )
        roles[role] = (roles[role] || 0) + 1;
    }
    ensure(roles.Canvas === 1, "accessibility.canvas");
    return {
      roles,
      widgetNames: "unsupported: eframe web does not publish AccessKit updates",
    };
  } finally {
    await session.detach();
  }
}
