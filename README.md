# Issue #155 visual evidence

Implementation revision: 61a8757159bb2b98db06014d086cd1ffca06afae. Original implementation: 94e5858ef4e3f7b53ecdb3e2282caa649ae1d7ee. Base: 535775927541e904b2a3a6991006d6c1898ae846.
Captured on 2026-09-23. Initial-load native and all browser captures were refreshed from the uncommitted follow-up diff subsequently committed unchanged as 61a8757159bb2b98db06014d086cd1ffca06afae. Original native next-image and short captures remain from 94e5858ef4e3f7b53ecdb3e2282caa649ae1d7ee; the follow-up changes only header suppression during first/session/dataset/logout loads and leaves those depicted states unchanged. Separate evidence branch; no artifacts enter the implementation branch.

## Native inspector

All full-height captures are 390×844 logical points, native scale 1.0. The short capture is 320×320 at scale 1.0. The inspector reaches the shared production renderer. Captures have no loaded image or annotation geometry.

- `native-before-review-loading.png`: original renderer from the base, with the new synthetic next-image fixture only. Loading replaces the summary and removes actions; footer starts at y=732.
- `native-review-initial.png`: first load keeps the global header visible and reserves blank context and footer. Footer starts at y=730, matching loaded geometry including frame borders.
- `native-review-initial-before-header-fix.png`: first load at 94e5858ef, before the global-header clarification, for comparison.
- `native-review-initial-header-wide.png`: corrected first load at 1440×1000, native scale 1.0. Navigation and utilities remain visible.
- `native-review-next-final.png`: same-view next-image load retains summary and disabled controls; no loading-only reflow.
- `native-migration-next-final.png`: full-image migration load retains disabled Add, Confirm & finish, Previous and Skip actions.
- `native-review-short.png`: retained controls remain reachable in the short layout.

## Chromium

`review-*-header.png`, `review-*-context.png` and `review-*-actions.png` are direct clipped screenshots from Chromium 153.0.8010.12, viewport 390×844 CSS pixels, DPR 1, zoom 100%. Only bars are captured to exclude image content and filenames.

- `review-initial`: annotation-to-review view switch, with the first review claim held.
- `review-ready`: loaded empty-image review.
- `review-next-loading`: approval accepted, next claim held; the same bar controls remain and are disabled.
- `review-previous-loading`: previous-image reopen held after advancing; current controls remain disabled.

The run uses the locked release WASM build and actual disposable loopback API/dataset. Playwright holds real responses; it does not replace application code. Annotation first/next load, review view-switch/first/next/previous, and repeated Space/Skip/Previous/pointer attempts pass. The disposable runtime is removed after the run.

Responsive matrix: 320×568, 390×844, 600×800, 1288×820, 1440×1000 and 320×320 at DPR 1 and 2; 390×844 also at DPR 3. Chromium launch scale and canvas backing dimensions are verified. Real tab zoom 200% is checked separately. See `browser-report.json`.

Browser font preferences do not resize egui canvas text. Shared kittest covers 36-point buttons and long workflow labels. Chromium exposes the canvas/text input rather than full egui widget semantics; native AccessKit and kittest verify disabled controls and labels. Live browser migration requests are not separately exercised; all migration phases run through shared-UI regressions and representative native inspection.

## Verification

- Canonical changed-path verification passes against the pinned base above: formatting, warnings-as-errors clippy, locked tests, inspector, WASM check and locked release Trunk build.
- 513 UI tests pass, including 6 new loading-bar tests. The original 5 fail against the original production renderer. The added header regression fails against the first implementation revision 94e5858ef and passes with the follow-up.
- 14 workflow variants × 6 viewport sizes assert exact retained control labels, rectangles and disabled state. Additional cases cover initial geometry, five scope invalidations, empty/failure/retry, long labels and 36-point text.
- Existing delayed previous-review success/failure and stale-response tests pass.
- Changed documentation: content reviewed; 9 local links/anchors checked initially, and all 4 in the two follow-up documents rechecked; diff whitespace clean.

See `acceptance.md` for the requirement-to-owner matrix. These are implementation evidence, not independent acceptance.

The corrected Chromium header captures for first load, loaded review, and next-image loading have identical SHA-256 hashes: `a7ad9beae27d41a2d495fe40aefa45cc72c7f582b6f3f5ddaf5d96e342c724d8`.
