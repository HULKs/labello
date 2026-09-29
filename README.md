# Workflow-change dialog evidence

Issue: https://github.com/HULKs/labello/issues/220

Before captures use production base `958b654ed5b2f8b00ce0d51777ace3c13df38afc` with only the new regression test added. After captures use that base plus the issue implementation, published as 2026961c2b26bdb7a1f529f81113fc1d4fed7a68. Final diff SHA-256: `9b9d375bd6af78e3969ba2554f815c55acd14a3d81c886c0c235d26899eac23c`.

## Native inspector

The `workflow-change` preset reaches the shared production renderer. Captures contain no image or annotation content. Native scale is 1. Screenshots at 1440×1000, 1288×820, 600×800, 390×844, 320×568 and 320×320 show the balancing reason, reused scales and type icons, wrapping labels and reachable acknowledgment. Before/after pairs are available at 1440×1000 and 390×844.

The dialog initially focuses its heading. Escape leaves it open. After allowing inspection events to render, Tab focuses Acknowledge and continue and Enter closes it; focus leaves the dialog. This is shared egui evidence, not browser evidence.

## Regression evidence

`automatic_workflow_change_explains_the_captured_availability_reason` failed before production edits with no node matching Other workflows need to catch up. It now checks specific and generic reasons in Annotation/Review, with later availability changes unable to rewrite the explanation. The same-name regression checks box/skeleton identities, migration configuration and frozen type icons across metadata changes. Existing acknowledgment, stale-response, scope and background-input guards remain covered. The responsive test exercises long names and 16/24-point body/button text, including a 320×320 viewport.

## Limitations

Native screenshots do not emulate browser DPR. No physical device, OS-level larger-text setting or full screen-reader session was tested. Deterministic AccessKit checks prove detailed widget semantics; browser accessibility evidence is limited to the canvas node. Independent acceptance remains pending.

## Production browser

Chromium 149.0.7827.55 with explicit ANGLE SwiftShader software rendering. Each of Annotation and Review passed 14 configurations: 320×568, 390×844, 600×800, 1288×820, 1440×1000 and 320×320 at DPR 1/2, plus 390×844 at DPR 3 and 1288×820 at actual 200% browser zoom. Filenames encode CSS viewport, DPR and zoom. JSONL reports record every passing case.

The disposable server reports annotation_finished or nothing_awaiting_review for the previous bounding-box workflow. The next skeleton workflow has the same task and class names. Captures show the checkmark or inbox reason icon and both task-type icons. There are no image claims before acknowledgment or after Escape. In Chromium, Tab enters the heading, a second Tab reaches acknowledgment, and Enter claims the next task. All final cases reported one claim after acknowledgment and no page errors. Canvas backing dimensions, observed DPR, actual Chrome zoom and the browser Canvas accessibility node were checked.

An earlier fixture used context DPR without the process-level scale flag, producing incorrect scale. Initial software fallback also lost its WebGL context and yielded blank captures despite working input. The final runs explicitly select ANGLE SwiftShader, validate nonblank screenshots and wait for a pointer-driven repaint. All 28 final captures passed on their first attempt. These fixture/environment failures did not require product changes.

Reproduction after the documented locked server/release builds: run `python -B browser.py annotation` and `python -B browser.py review` from the source checkout using the private Playwright environment in docs/stylus-input.md. The script writes to /tmp/labello-220-evidence and creates only disposable server and browser data.

## Local verification

`env -u NO_COLOR ./scripts/verify.sh changed origin/main` passed against the base above. The canonical checks include formatting, locked Clippy, workspace and UI tests, standalone-inspector checks/tests, WASM check and release Trunk build. UI: 678 passed and 2 existing ignored. All test groups: 1290 passing instances and 3 existing ignored. The six focused automatic_workflow cases passed. Documentation link/anchor checks passed for 63 Markdown files and 30 wiki pages. No lockfile changes or unrelated worktree changes.
