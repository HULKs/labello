# Visual evidence for Labello PR #150

Captured from implementation commit 0166322666245f2c1244772f6a30c4dae15c444c on 2026-09-23. All data is synthetic.

The revised Additional info control shares the author row and aligns right. Expanded details use the full width below that row.

- desktop.png: native shared review UI at 1440 x 1000.
- additional-info.png: expanded details in the native review UI.
- browser-narrow.png: Chromium 153 release WASM, 390 x 844 CSS viewport, DPR 2. The screenshot is captured at CSS resolution. Penalty spots is the active workflow, so the synthetic penalty feedback belongs here.
- browser-zoom-200.png: actual 200% Chrome tab zoom, 720 x 456 CSS viewport, expanded details below the full author row.

Browser author metadata and avatar bytes are synthetic fixtures. The turquoise circle is the deterministic avatar image. Workflow requests and saved reasons use the disposable real server. Native captures use the workflow-reasons preset. See https://github.com/HULKs/labello/pull/150 for the acceptance evidence and limitations.
