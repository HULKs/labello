# Issue 206 follow-up evidence

This change responds to the maintainer's report that bounding-box review still briefly greyed the UI and displayed a spinner after PR #212. The new regression assertion failed before the fix: `image navigation must not fade the retained canvas` in `delayed_previous_review_keeps_canvas_busy_state_and_replaces_on_success`. No before screenshot was retained; the failure was captured as a deterministic mesh-opacity assertion.

Production owners: the shared egui shell and workspace panels, workflow markers, Inspect's request/reducer state, and existing Admin/Export/About/Setup/Import owners. WASM and the native inspector use these same production owners. No transport or persisted data contract changes.

## Acceptance matrix

| Workflow/view | Observable behavior and negative cases | Evidence |
| --- | --- | --- |
| Bounding-box annotation | Retain canvas, bars and controls at normal opacity during next/previous; block duplicate mutations | Real production Chromium delayed next/previous matrix; shared loading-bar matrix and frozen release test |
| Bounding-box review and reviewer correction | Same retained display; do not show saving spinner for release/reopen | Real Chromium review matrix; fail-before previous-review regression; Review and ReviewCorrection shared states |
| Skeleton annotation, review and correction | Same loading policy across object overlays | Chromium annotation/review matrix; OverlayAnnotation, OverlayReview and OverlayCorrection shared states |
| Migration object, optional single, exclusion, pass and full-image | Stable controls and retained image while next target loads; no stale mutation | Shared 17-preset matrix includes each named state |
| Migration review, discovery, discovery review, annotated edit, guide deleted and companion annotation | Same policy through migration modes | Shared 17-preset matrix includes each named state; native migration-next-image inspected at all six sizes |
| Inspect | Keep displayed identity/pixels/annotations coherent until both replacement replies succeed; either response order; same-image refresh; failures/retry and obsolete replies | New Inspect reducer tests, existing page-boundary/thumbnail tests; production request, scheduling, gallery and mutation-guard audit |
| Admin, users and Export | Loaded refresh does not claim saving/loading; actual writes retain operation progress | Frozen Admin and Export kittest, responsive Admin status tests; owner audit |
| Setup catalog/schema sources | Empty successful result remains settled during refresh; initial loading and errors remain distinct | Dataset remote-state kittest includes empty loaded refresh; session-reset audit |
| About | Retain server identity during refresh; never reload from an unvalidated retained identity | Build-information and reload-coordinator tests; endpoint/stale-response coverage |
| Import | Loaded job status polls are silent; explicit commit/upload progress remains | Import status-poll test and existing import suites; operation priority audit |
| Statistics, streak, admin image/backup catalogs, Inspect gallery pagination | Keep loaded data through refresh and preserve failures | Existing statistics remote-state/coalescing tests and production owner audit; no new loader introduced |
| Initial entry, changed dataset/view/workflow/auth, empty and failed loads | Initial feedback stays available; scope invalidation prevents stale display or mutation; failures remain visible | Loading-bar first-load/scope/empty/failure tests and existing stale request/epoch tests |
| Explicit save, import, export, model check and confirmation dialogs | Keep actionable progress and error feedback | Existing operation tests, new import progress assertion, shared modal/saving tests |

The shared image-loading matrix covers 17 presets at 320x568, 390x844, 600x800, 1288x820, 1440x1000 and 320x320. It checks retained image mesh opacity, control identity/geometry, disabled AccessKit state, absent loading labels and blocked keyboard commands. Separate long-content/36-point-text and scale tests cover larger text and layout. Native review-next-image and migration-next-image were inspected through the MCP/Xvfb application at all six sizes, including disabled action semantics.

## Browser procedure

Run the committed `apps/labello-wasm/tests/silent_loading.py` using the isolated Playwright/Pillow environment described in `docs/stylus-input.md`, after building the production server and release WASM. `browser-matrix.jsonl` records the exact viewport, DPR, workflow and result of every case. Four combinations (bounding box/skeleton × annotation/review), six sizes, DPR 1/2, plus 390x844 at DPR 3. Each case holds four actual release/reopen requests and samples 20 frames in memory. It validates retained image opacity and browser canvas accessibility presence; widget semantics are checked by kittest.

`--zoom` uses an isolated local Chrome extension to set and read back actual tab zoom 2. Screenshots are cropped at capture time to application bars, excluding image content and annotation geometry. They show normal-opacity navigation context during a held previous-image request. Captures are Chromium 149.0.7827.55, DPR 1 except the skeleton mobile capture (DPR 3); normal zoom except the explicitly named zoom2 capture. The source was an uncommitted issue-scoped diff against base `2d7f350cde42e04216e56c04dbcbd9ae86439204`; the associated PR records the published commit. Final captures were refreshed after the final release build.

- `annotation-bounding_box-1288-pending.png`: annotation, 1288x820, DPR 1.
- `review-bounding_box-1288-pending.png`: review, 1288x820, DPR 1.
- `review-skeleton-390-pending.png`: skeleton review, 390x844, DPR 3.
- `review-bounding_box-1288-zoom2-pending.png`: review, 1288x820, DPR 1, actual 200% Chrome zoom.

## Verification limits

The live browser matrix exercises release/reopen navigation, not every migration transaction, Inspect response failure, or correction submission. Those paths have shared UI/reducer evidence; this is not a claim of exhaustive browser interaction coverage. Native rendering is not browser proof. Physical devices, operating-system text-size settings and a full screen-reader session were not exercised. Larger text uses egui settings; the browser AX check is limited to the canvas node. Synthetic images and all raw request data remain local and are not uploaded.

The first concurrent browser sweep had synchronization timeouts after transitions; it was stopped. The script now waits for API requests to settle rather than relying on a fixed delay. The reported sweep had 51 passes and one mobile DPR 3 startup timeout while the release build was being replaced. The unchanged case passed on a settled-build retry. Both the bounded failure category and retry result are included; no product assertion failure was observed. The startup-timeout cause was not conclusively established. Initial Clippy failure (duplicate Admin status branches) was fixed and canonical verification rerun. Independent acceptance remains pending.

Published source commit: `a35fceaab0cf72ecc132fbb1b96c027c4faed855`. `source-files.json` records the exact included files and SHA-256 hashes. Canonical changed-path verification passed; final UI suite: 676 passed, 2 existing ignored.
