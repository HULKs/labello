import {
  capture,
  ensure,
  until,
  comparePng,
  waitForSettingsPaint,
} from "./support.mjs";
import {
  frames,
  click,
  key,
  drag,
  start,
  enter,
  imageState,
  read,
  accessibility,
} from "./driver.mjs";

export async function annotation(page, environment, options) {
  const { monitor, observation } = await start(page, environment);
  const assignment = await enter(page, observation);
  const state = () => imageState(page, environment, assignment);
  await drag(page, [430, 400], [650, 600]);
  await key(page, "Control+s");
  await until(
    async () => Object.keys((await state()).annotations).length === 1,
    "annotation.save",
  );
  const saved = Object.values((await state()).annotations)[0][0];
  await drag(page, [540, 500], [580, 530]);
  await until(
    () =>
      page.evaluate(async () => {
        const databases = await indexedDB.databases();
        if (!databases.some((db) => db.name === "labello-workspace-v2"))
          return false;
        return new Promise((resolve) => {
          const opening = indexedDB.open("labello-workspace-v2");
          opening.onsuccess = () => {
            const db = opening.result;
            const request = db
              .transaction("drafts")
              .objectStore("drafts")
              .count();
            request.onsuccess = () => {
              db.close();
              resolve(request.result > 0);
            };
            request.onerror = () => {
              db.close();
              resolve(false);
            };
          };
          opening.onerror = () => resolve(false);
        });
      }),
    "draft.persisted",
  );
  await page.reload();
  await until(
    async () => (await page.locator("#startup-status").count()) === 0,
    "draft.restart",
  );
  await frames(page, 30);
  await key(page, "Control+s");
  await until(
    async () => Object.values((await state()).annotations)[0].length === 2,
    "draft.recovered_save",
  );
  const edited = Object.values((await state()).annotations)[0][1];
  ensure(
    JSON.stringify(saved.geometry) !== JSON.stringify(edited.geometry),
    "annotation.edit",
  );
  const screenshots = [
    await capture(environment, page, "annotation-wide", options),
  ];
  await page.setViewportSize({ width: 390, height: 844 });
  await frames(page, 15);
  screenshots.push(
    await capture(environment, page, "annotation-compact", options),
  );
  await key(page, "Control+,");
  screenshots.push(
    await capture(environment, page, "settings-compact", options),
  );
  await key(page, "Escape");
  await page.setViewportSize({ width: 1440, height: 1000 });
  await frames(page, 15);
  await key(page, "Control+,");
  await frames(page, 10);
  screenshots.push(await capture(environment, page, "settings-wide", options));
  await click(page, 921, 327);
  await waitForSettingsPaint(environment, page, "recording");
  await key(page, "Enter");
  await waitForSettingsPaint(environment, page, "save-enabled");
  await click(page, 1016, 817);
  await until(
    async () =>
      (await read(page, environment, "/datasets/browser-fixture/keybindings"))
        .bindings.next_image.key === "Enter",
    "keybindings.persisted",
  );
  await key(page, "Escape");
  // Keyboard-only modal reopening, tab traversal, dismissal, and configured
  // submission are backed by the durable state transition below.
  await key(page, "Control+,");
  await key(page, "Tab");
  await key(page, "Escape");
  await key(page, "Enter");
  await until(
    async () =>
      (await state()).taskStates[assignment.taskId].status === "submitted",
    "annotation.submit",
  );
  await frames(page, 20);
  const releasesBefore = observation.counters.releases;
  await key(page, "x");
  await until(
    () => observation.counters.releases > releasesBefore,
    "annotation.skip_request",
  );
  const states = await Promise.all(
    observation.assignments.map((item) => imageState(page, environment, item)),
  );
  ensure(
    states.some((state) =>
      state.assignments.some((item) => item.status === "cancelled"),
    ),
    "annotation.skip",
  );
  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "annotation",
    states: [
      "create",
      "edit",
      "save",
      "draft-recovery",
      "keybindings",
      "submit",
      "skip",
      "keyboard-modal",
    ],
    screenshots,
    accessibility: await accessibility(page),
    counts: monitor.counts,
  };
}

export async function review(page, environment, options) {
  const { monitor, observation } = await start(page, environment);
  const assignment = await enter(page, observation, true);
  const screenshots = [
    await capture(environment, page, "review-wide", options),
  ];
  await page.setViewportSize({ width: 390, height: 844 });
  await frames(page, 15);
  screenshots.push(await capture(environment, page, "review-compact", options));
  await key(page, "y");
  await until(
    async () =>
      (await imageState(page, environment, assignment)).reviews.length === 1,
    "review.object_approval",
  );
  await key(page, "y");
  await until(
    async () =>
      (await imageState(page, environment, assignment)).taskStates[
        assignment.taskId
      ].status === "completed",
    "review.full_image_approval",
  );
  await frames(page, 20);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await frames(page, 15);
  await click(page, 1211, 368);
  await frames(page, 10);
  await drag(page, [665, 500], [700, 520]);
  await click(page, 1222, 667);
  await until(
    () => observation.counters.corrections === 1,
    "review.correction",
  );
  const changed = await Promise.all(
    observation.assignments.map((item) => imageState(page, environment, item)),
  );
  ensure(
    changed.some(
      (state) =>
        state.reviewerCorrections.length === 1 &&
        Object.values(state.annotations).some(
          (versions) => versions.length === 2,
        ),
    ),
    "review.correction_revision",
  );

  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "review",
    screenshots,
    assignment: !!assignment,
    accessibility: await accessibility(page),
  };
}

export async function migration(page, environment, options) {
  const { monitor, observation } = await start(page, environment);
  const assignment = await enter(page, observation);
  const screenshots = [
    await capture(environment, page, "migration-wide", options),
  ];
  await gestures(page);
  await click(page, 670, 360);
  await drag(page, [670, 360], [690, 380]);
  await key(page, "Control+,");
  await key(page, "Tab");
  await click(page, 921, 327);
  await waitForSettingsPaint(environment, page, "recording");
  await key(page, "Enter");
  await waitForSettingsPaint(environment, page, "save-enabled");
  await click(page, 1016, 817);
  await until(
    async () =>
      (await read(page, environment, "/datasets/browser-fixture/keybindings"))
        .bindings.next_image.key === "Enter",
    "migration.configured_shortcut",
  );
  await key(page, "Escape");
  await key(page, "h");
  await click(page, 685, 500);
  await key(page, "n");
  await key(page, "Enter");
  await until(
    async () =>
      Object.values(
        (await imageState(page, environment, assignment)).annotations,
      ).some((versions) => versions.at(-1).type === "skeleton"),
    "migration.object_save",
  );
  const skeleton = Object.values(
    (await imageState(page, environment, assignment)).annotations,
  )
    .map((versions) => versions.at(-1))
    .find((value) => value.type === "skeleton");
  ensure(
    JSON.stringify(
      skeleton.geometry.geometry.keypoints.map((point) => point.state),
    ) === JSON.stringify(["visible", "hidden", "absent"]),
    "migration.keypoint_outcomes",
  );
  await frames(page, 15);
  await page.setViewportSize({ width: 390, height: 844 });
  await frames(page, 15);
  screenshots.push(
    await capture(environment, page, "migration-compact", options),
  );
  await key(page, "Enter");
  await until(
    async () =>
      (await imageState(page, environment, assignment)).taskStates[
        assignment.taskId
      ].status === "submitted",
    "migration.confirm_submit",
  );
  ensure(
    Object.keys(
      (await imageState(page, environment, assignment)).migrationConfirmations,
    ).length === 1,
    "migration.confirmation",
  );
  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "migration",
    states: [
      "pan",
      "zoom",
      "fit",
      "point-selection-edit",
      "modal-draft-preservation",
      "configured-shortcut",
      "visible",
      "occluded",
      "absent",
      "object-save",
      "confirm-submit",
    ],
    screenshots,
    assignment: !!assignment,
    accessibility: await accessibility(page),
  };
}

async function gestures(page) {
  await key(page, "0");
  await frames(page, 10);
  await page.mouse.move(1, 999);
  await frames(page, 10);
  const fitted = await page.screenshot();
  await page.mouse.move(650, 500);
  await page.mouse.wheel(0, -400);
  await frames(page, 15);
  await page.mouse.move(1, 999);
  await frames(page, 10);
  const zoomed = await page.screenshot();
  ensure(comparePng(fitted, zoomed).ratio > 0.01, "canvas.zoom");
  await drag(page, [600, 500], [660, 520], "middle");
  await page.mouse.move(1, 999);
  await frames(page, 10);
  ensure(
    comparePng(zoomed, await page.screenshot()).ratio > 0.01,
    "canvas.pan",
  );
  await key(page, "0");
  await page.mouse.move(1, 999);
  await frames(page, 15);
  ensure(
    comparePng(fitted, await page.screenshot()).ratio < 0.003,
    "canvas.fit",
  );
}

export async function skeletons(page, environment) {
  const { monitor, observation } = await start(page, environment);
  const assignment = await enter(page, observation);
  await gestures(page);
  await click(page, 430, 400);
  // Force the save boundary that previously dropped later keypoint edits.
  await until(
    async () =>
      Object.keys((await imageState(page, environment, assignment)).annotations)
        .length === 1,
    "skeleton.partial_autosave",
  );
  await key(page, "h");
  await click(page, 500, 500);
  await key(page, "n");
  // Opening/dismissing support must preserve the in-progress keypoint draft.
  await key(page, "Control+,");
  await key(page, "Tab");
  await key(page, "Escape");
  await key(page, "Control+s");
  const latest = async () =>
    Object.values(
      (await imageState(page, environment, assignment)).annotations,
    )[0]?.at(-1);
  // Autosave may already have persisted an earlier partial skeleton. Wait for
  // the completed outcomes instead of treating its first revision as this save.
  await until(
    async () =>
      JSON.stringify(
        (await latest())?.geometry.geometry.keypoints.map(
          (point) => point.state,
        ),
      ) === JSON.stringify(["visible", "hidden", "absent"]),
    "skeleton.keypoint_outcomes",
  );
  const initial = await latest();
  await click(page, 430, 400);
  await drag(page, [430, 400], [460, 430]);
  await key(page, "Control+s");
  await until(
    async () =>
      (await latest()).version > initial.version &&
      JSON.stringify((await latest()).geometry) !==
        JSON.stringify(initial.geometry),
    "skeleton.point_edit",
  );
  await key(page, "Space");
  await until(
    async () =>
      (await imageState(page, environment, assignment)).taskStates[
        assignment.taskId
      ].status === "submitted",
    "skeleton.submit",
  );
  monitor.check();
  monitor.assertConsumed();
  return {
    workflow: "skeletons",
    states: [
      "pan",
      "zoom",
      "fit",
      "visible",
      "occluded",
      "absent",
      "modal-draft-preservation",
      "point-selection-edit",
      "save",
      "submit",
    ],
    counts: monitor.counts,
  };
}

export async function admin(page, environment, options) {
  const { monitor } = await start(page, environment);
  await click(page, 589, 520);
  await frames(page, 20);
  await click(page, 250, 378);
  await frames(page, 15);
  await click(page, 618, 789);
  await frames(page, 15);
  await page.mouse.move(1030, 800);
  await page.mouse.wheel(0, 450);
  await frames(page, 15);
  await click(page, 600, 635);
  await key(page, "Control+a");
  await page.keyboard.insertText("Synthetic renamed workflow");
  await frames(page, 15);
  await page.mouse.move(1030, 300);
  await page.mouse.wheel(0, -1400);
  await frames(page, 15);
  await click(page, 1206, 118);
  await until(
    async () =>
      (await read(page, environment, "/datasets/browser-fixture/admin"))
        .tasks[0].name === "Synthetic renamed workflow",
    "admin.task_save",
  );
  await page.reload();
  await until(
    async () => (await page.locator("#startup-status").count()) === 0,
    "admin.restart",
  );
  await frames(page, 30);
  ensure(
    (await read(page, environment, "/datasets/browser-fixture/admin")).tasks[0]
      .name === "Synthetic renamed workflow",
    "admin.task_reload",
  );
  monitor.check();
  monitor.assertConsumed();
  return { workflow: "admin" };
}
