# Workflow change acknowledgment evidence

Issue: https://github.com/HULKs/labello/issues/204

After captures: implementation `52e3897d5919f85905dd8c0307a463d0f83abf7b`, captured from the uncommitted diff whose SHA-256 is `0110453e589488b2d679815373d15337e9af6b90d4d5ddd92a1a7f215f3cadc9`; the exact diff was committed without changes. Base/before: `20a1dfcc6d6647045c0fe8f32935b90c86f62ad3`.

Synthetic fixtures only. No image content or review comments are included.

| Capture | Scenario and settings |
| --- | --- |
| [before-notice.png](before-notice.png) | Native inspector before: cropped former dismissible notice, 1440 × 1000 viewport, scale 1; old workflow-reasons preset. |
| [native-1440x1000.png](native-1440x1000.png) | Native inspector after: workflow-change preset, 1440 × 1000, scale 1; blocking explanation and explicit action. |
| [native-320x320.png](native-320x320.png) | Native inspector after: same pending change, 320 × 320, scale 1; bounded action. |
| [browser-dpr1-zoom1-1440x1000.png](browser-dpr1-zoom1-1440x1000.png) | Chromium 149.0.7827.55, production WASM/API fallback, 1440 × 1000 CSS pixels, DPR 1, 100% zoom. |
| [browser-dpr2-zoom1-390x844.png](browser-dpr2-zoom1-390x844.png) | Chromium after: pending fallback, 390 × 844 CSS pixels, DPR 2, 100% zoom. |
| [browser-dpr2-zoom1-320x320.png](browser-dpr2-zoom1-320x320.png) | Chromium after: pending fallback, 320 × 320 CSS pixels, DPR 2, 100% zoom. |
| [browser-dpr3-zoom1-390x844.png](browser-dpr3-zoom1-390x844.png) | Chromium after: pending fallback, 390 × 844 CSS pixels, DPR 3, 100% zoom. |
| [browser-dpr1-zoom2-1440x1000.png](browser-dpr1-zoom2-1440x1000.png) | Chromium after: pending fallback, 1440 × 1000 window, DPR 1 baseline, actual Chrome tab zoom 200%. |

Native AccessKit and egui_kittest verify modal/dialog semantics and keyboard focus. Chromium exposes the canvas without internal decision nodes in the captured accessibility tree; these captures do not establish browser screen-reader support.
