# Visual evidence for Labello PR #150

Implementation commit d5db3205712199d040fede3ed6e512a327e2bc9d, captured on 2026-09-23 from the resolved merge worktree before the merge commit, without subsequent source edits. All data is synthetic.

Additional info shares the author row and aligns right. Expanded details use the full width below that row.

- desktop.png: shared native review UI, 1440 x 1000, native scale 1.
- additional-info.png: native review UI, 1440 x 1000, scale 1, expanded audit details.
- browser-narrow.png: Chromium 153.0.8010.12 release WASM, 390 x 844 CSS viewport and DPR 2. Screenshot captured at CSS resolution. Penalty spots is selected, so its synthetic feedback belongs here.
- browser-zoom-200.png: Chromium release WASM, actual 200% tab zoom and 720 x 456 CSS viewport, reported DPR 2. Expanded details use the full width below the author row.

The browser author metadata and turquoise avatar image are deterministic fixtures. Workflow requests and reasons use a disposable real server. Native screenshots use the workflow-reasons inspector preset. See https://github.com/HULKs/labello/pull/150 for the before/after comparison, verification and limitations.
