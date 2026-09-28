# Issue 206: silent loading evidence

Tested source: base `c8079b500d157aee903d697e3908a8a814fa8ef8` with rebased source commit `98593cb8c3c9360b9a76927611dfdcc3d07b0ea9` and diff SHA-256 `1148002330fba34d235545473105711630de77a4e646de55b673a8a41f7efbd5`. `source-files.json` contains every changed file's hash. These captures replace the earlier evidence after rebasing onto the new context-bar layout and glossary changes. All captures use synthetic demo images, users and annotations.

The original inspector defect was reproduced by a failing `subsequent_image_load_keeps_inspector_context_visible` test before the production change. A screenshot of the original build was not captured before editing. The browser before/pending/after sequence below compares states of the final implementation, not different source revisions.

## Native inspector

Native scale 1; final worktree build, Xvfb and isolated MCP server. `review-initial-load`, `review-next-image` and `migration-next-image` inspected at 1440x1000, 1288x820, 600x800, 390x844, 320x568 and 320x320. No assignment is held by the retained next-image presets.

- `native-review-initial-load-390x844.png`: initial entry may show a loader, compact viewport.
- `native-review-next-image-1440x1000.png`: delayed replacement retains the image, workflow and complete inspector; controls are disabled.
- `native-review-next-image-320x320.png`: short viewport retains the canvas and action-bar geometry.
- `native-migration-next-image-1440x1000.png`: migration image and inspector remain present during replacement.

## Chromium

Chromium 149.0.7827.55, release production WASM with intercepted synthetic API responses, software ANGLE/SwiftShader. These are browser-rendering and interaction checks, not a live Axum/OAuth integration test. Viewports 320x320, 320x568, 390x844, 600x800, 1288x820 and 1440x1000 at DPR 1 and 2; 390x844 at DPR 3. Normal screenshots use 100% zoom and CSS-pixel capture dimensions.

- `browser-before-1440x1000-dpr1.png`: loaded first image, inspector open, before Skip.
- `browser-pending-1440x1000-dpr1.png`: second claim deliberately delayed; same image and inspector remain visible.
- `browser-after-1440x1000-dpr1.png`: successful response replaces image and details together.
- `browser-pending-390x844-dpr3.png`: compact workspace retains image during a delayed response, DPR 3.
- `browser-pending-320x320-dpr2.png`: smallest/shortest viewport, DPR 2.
- `browser-after-390x844-dpr1-failure.png`: failed replacement clears the old preview and offers recovery.
- `browser-pending-1288x820-dpr1-zoom2.png`: actual Chrome tab zoom 200%, confirmed through the extension API; retained canvas at the resulting compact layout.

The browser checks compare preview-color pixels while pending and after replacement, and attempt Skip/Space/Previous while waiting. There are exactly two claims and one release per case, with no extra mutation requests or page errors. The two failure cases cover wide and phone layouts. `browser-report.json` and `browser-zoom-report.json` contain results.

Shared egui tests cover disabled AccessKit controls, drawer Escape behavior, stable rectangles, all workflow presets, empty/failure outcomes, stale scope rejection, initial versus subsequent feedback, and long content/large text. Chromium's accessibility tree exposes a canvas rather than equivalent widget semantics; no screen-reader certification is claimed. OS-level text scaling and physical devices were not exercised.
