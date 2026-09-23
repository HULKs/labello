# Visual evidence for Labello PR #150

Captured from implementation commit 75a62f4e957da35105ef66c1790687027e31e479 on 2026-09-23. All screenshots use synthetic data.

- desktop.png: shared native review UI at 1440 x 1000; compact title/explanation spacing, author initials fallback, collapsed audit details and historical messages.
- additional-info.png: native review UI with audit details expanded.
- browser-narrow.png: release WASM in Chromium 153 at 390 x 844 CSS pixels and DPR 2. Penalty spots is the active workflow, so its synthetic feedback belongs here. The turquoise avatar is a deterministic test image.
- browser-zoom-200.png: actual Chrome 200% tab zoom, 720 x 456 CSS viewport. The bounded detail window keeps audit details and Close feedback reachable.

Browser author metadata and avatar bytes are synthetic fixtures. Workflow requests and saved reasons use the disposable real server. The native captures use the deterministic workflow-reasons inspector preset. See https://github.com/HULKs/labello/pull/150 for the acceptance evidence and limitations.
