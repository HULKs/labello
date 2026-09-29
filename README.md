# Issue 218 visual and browser evidence

Implementation: HULKs/labello#218. Published source revision: `62b67751574bd04700c367ec3831bf274b1f883a`.
Verified comparison base: `958b654ed5b2f8b00ce0d51777ace3c13df38afc`.

All screenshots contain synthetic accounts, empty disposable datasets or deterministic statistics presets. No annotation images, geometry, request data, credentials or private paths are included.

## Native comparison

Before captures use `2d7f350cde42e04216e56c04dbcbd9ae86439204` from an isolated checkout. After captures were taken with the feature diff on that base. The feature then rebased cleanly onto `958b654`; the later change only corrects failed-overview retry scheduling. Neither changes the populated statistics or migration-editor presentation shown here. Those captures therefore depict the renderer published at `62b67751574bd04700c367ec3831bf274b1f883a`.

| Capture | State | Viewport | Scale |
| --- | --- | --- | --- |
| [Before desktop](before-statistics-desktop.png) | Existing dataset statistics | 1440×1000 | 1 |
| [After desktop](after-statistics-desktop.png) | Populated global statistics, combined contributors | 1440×1000 | 1 |
| [Before mobile](before-statistics-mobile.png) | Existing dataset statistics | 390×844 | 1 |
| [After mobile](after-statistics-mobile.png) | Populated global statistics | 390×844 | 1 |
| [Short window](after-statistics-320x320.png) | Global scope and close controls remain reachable | 320×320 | 1 |
| [Shortcut migration desktop](after-shortcuts-desktop.png) | Explicit preserved-dataset source choice | 1440×1000 | 1 |
| [Shortcut migration mobile](after-shortcuts-mobile.png) | Global settings and source selector | 390×844 | 1 |

The native inspector also covered 320×568, 600×800 and 1288×820. Named AccessKit controls and deterministic geometry/focus checks are covered by `egui_kittest`. This does not establish browser screen-reader behavior.

## Chromium workflow checks

Chromium 149.0.7827.55, Linux, release WASM, disposable loopback server. Both headless-shell and full Chromium were used; software WebGL was explicitly enabled. Captures use the feature diff, with the final failure/recovery, keyboard and DPR 3 captures including the scheduling correction published at `62b67751574bd04700c367ec3831bf274b1f883a`. The server/storage routes are unchanged by the rebase and scheduling correction.

- Open Statistics from Setup without selecting a work dataset. Switch between All accessible datasets and Training; the underlying Setup/work dataset remains unchanged. [Individual view](browser-statistics-individual.png).
- Change Confirm/submit to F9 and save. Reload the page and confirm F9 remains. Switch through Training and Validation and confirm F9 remains. [Reload](browser-shortcuts-reloaded.png), [Validation dataset](browser-shortcuts-validation.png).
- Restore all defaults, save, reload and confirm Space. The browser driver also reads the persisted own-user API value to assert F9 and then Space. [Reset after reload](browser-shortcuts-reset.png).
- Inject a 500 response on the overview read. Confirm Statistics unavailable and Retry statistics remain visible. Restore the real endpoint and confirm recovery. [Failure](browser-statistics-failure.png), [Recovery](browser-statistics-recovered.png).
- Keyboard pass: open Statistics, initial focus Close statistics, Tab to Statistics for, Space opens the menu, Tab through aggregate and dataset options, Enter selects Training. Escape dismisses the menu and modal and restores focus to the app-bar statistics button. [Keyboard selection](browser-statistics-keyboard-selected.png), [Restored focus](browser-statistics-keyboard-restored.png).
- Browser accessibility-tree inspection exposes Canvas plus generic/textbox nodes, not the native named-control tree. No browser screen-reader claim is made.

## Browser scale matrix

All cases exercised the live statistics modal. Native canvas backing dimensions were checked against CSS dimensions multiplied by DPR. DPR 1/2 captures use the populated layout with empty contributor history from two accessible datasets. DPR 3 uses full Chromium at the target mobile size. Some Chromium screenshot modes produced invalid white captures during development; those artifacts were discarded and the captures below were inspected. The DPR 3 image retains device pixels; most others use CSS-pixel capture dimensions.

| CSS viewport | DPR 1 | DPR 2 |
| --- | --- | --- |
| 320×320 | [Capture](browser-statistics-320x320-dpr1.png) | [Capture](browser-statistics-320x320-dpr2.png) |
| 320×568 | [Capture](browser-statistics-320x568-dpr1.png) | [Capture](browser-statistics-320x568-dpr2.png) |
| 390×844 | [Capture](browser-statistics-390x844-dpr1.png) | [Capture](browser-statistics-390x844-dpr2.png) |
| 600×800 | [Capture](browser-statistics-600x800-dpr1.png) | [Capture](browser-statistics-600x800-dpr2.png) |
| 1288×820 | [Capture](browser-statistics-1288x820-dpr1.png) | [Capture](browser-statistics-1288x820-dpr2.png) |
| 1440×1000 | [Capture](browser-statistics-1440x1000-dpr1.png) | [Capture](browser-statistics-1440x1000-dpr2.png) |

[390×844 at DPR 3](browser-statistics-390x844-dpr3.png).

Actual browser tab zoom was set through `chrome.tabs.setZoom` and read back as 2. A 2880×2000 browser viewport yields 1440×1000 CSS points; resizing to 1440×1000 yields 720×500 CSS points. [Desktop at 200%](browser-statistics-zoom200-desktop.png), [Compact at 200%](browser-statistics-zoom200.png). Scope and close remain reachable, and content scrolls within the modal.

The headless Linux environment has no applicable platform larger-text setting. Physical phones/tablets, screen-reader output and large-server performance were not tested.

## Verification record

`./scripts/verify.sh changed origin/main` passed against the recorded base. It covers documentation links, formatting, Clippy, locked workspace and shared-UI tests, the standalone inspector, the WASM compiler check and the locked release Trunk build.

Focused aggregation, global API, global UI and failure-retry tests passed. Existing ingestion polling hit its short deadline once under concurrent build load; the focused retry and later full run passed without an ingestion change. An interrupted final build was rerun. All reported acceptance results use the successful final run.

The new failure-retry regression checks that the workspace timer cannot immediately hide a failed overview, that the overview retries after its own interval, and that the visible error/Retry controls remain present.

Independent acceptance is pending. This evidence is the implementer's report.
