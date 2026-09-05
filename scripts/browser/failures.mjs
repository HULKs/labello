import { ensure, until } from "./support.mjs";
import { frames, click, key, drag, start, imageState } from "./driver.mjs";

export async function recovery(page, environment) {
  const { monitor, observation } = await start(page, environment);
  let adminRoute,
    previewRoute,
    failed = false;
  await page.route("**/datasets/browser-fixture/admin", async (route) => {
    if (!adminRoute && route.request().method() === "GET") adminRoute = route;
    else await route.continue();
  });
  await page.route("**/preview?*", async (route) => {
    if (!failed && !previewRoute) previewRoute = route;
    else await route.continue();
  });
  await click(page, 589, 520);
  await until(() => !!adminRoute, "recovery.admin_loading");
  const oldAdmin = await adminRoute.fetch();
  await click(page, 60, 28);
  await until(() => !!previewRoute, "recovery.image_loading");
  await frames(page, 15);
  const path = new URL(previewRoute.request().url()).pathname;
  monitor.expect(`http:${path}:503`);
  monitor.expect("console");
  failed = true;
  await previewRoute.fulfill({
    status: 503,
    headers: {
      "content-type": "application/json",
      "access-control-allow-origin": environment.url,
      "access-control-allow-credentials": "true",
    },
    body: '{"error":"synthetic temporary failure"}',
  });
  await frames(page, 20);
  await key(page, "r");
  await until(() => observation.counters.previews > 0, "recovery.retry");
  await frames(page, 15);
  await adminRoute.fulfill({ response: oldAdmin });
  await frames(page, 15);
  const assignment = observation.assignments[0];
  ensure(assignment, "recovery.assignment");
  await drag(page, [430, 400], [650, 600]);
  await key(page, "Control+s");
  await until(
    async () =>
      Object.keys((await imageState(page, environment, assignment)).annotations)
        .length === 1,
    "recovery.stale_response_preserves_workspace",
  );
  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "recovery",
    states: [
      "held-admin-response",
      "loading",
      "declared-http-503",
      "keyboard-retry",
      "stale-response",
      "continued-edit-save",
    ],
    counts: monitor.counts,
  };
}
