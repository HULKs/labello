# Build mismatch recovery evidence

Issue #179. Chromium 149.0.7827.55 and native egui inspector. All captures use synthetic release identities and contain no dataset, annotation, or credential content.

Browser captures use the production WASM from base `20a1dfcc6d6647045c0fe8f32935b90c86f62ad3` plus the issue diff published by the linked implementation PR. Final publication scope SHA-256: `52eb7a720dcfc31d20ec1f063c24730a3bf87c83767ba70f3a2c66cebf3a77a4`.

Native captures use the same shared rendering before the final nonvisual safety guards were finished; no subsequent edit changed their depicted layout. Native scale is 1. Browser filename DPR values specify device scale, with the Chromium native scale set to match. Browser zoom is 100% except `browser-200percent.png`, captured at real Chromium 200% zoom with X11 keyboard input. The 200% capture is scrolled to the retry control.

The original implementation had no update progress/failure controls; those are new states, so there is no equivalent before capture. `update-before.png` and `update-after.png` show the A-to-B state transition using the new implementation.

| Capture | Pixel dimensions | Scenario |
| --- | --- | --- |
| [browser-1288x820-dpr1.png](browser-1288x820-dpr1.png) | 1288×820 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-1288x820-dpr2.png](browser-1288x820-dpr2.png) | 2576×1640 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-1440x1000-dpr1.png](browser-1440x1000-dpr1.png) | 1440×1000 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-1440x1000-dpr2.png](browser-1440x1000-dpr2.png) | 2880×2000 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-200percent.png](browser-200percent.png) | 1288×913 | Chromium 200% zoom, 644×456 CSS viewport, DPR 2, retry visible after scrolling. |
| [browser-320x320-dpr1.png](browser-320x320-dpr1.png) | 320×320 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-320x320-dpr2.png](browser-320x320-dpr2.png) | 640×640 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-320x568-dpr1.png](browser-320x568-dpr1.png) | 320×568 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-320x568-dpr2.png](browser-320x568-dpr2.png) | 640×1136 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-390x844-dpr1.png](browser-390x844-dpr1.png) | 390×844 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-390x844-dpr2.png](browser-390x844-dpr2.png) | 780×1688 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-390x844-dpr3.png](browser-390x844-dpr3.png) | 1170×2532 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-600x800-dpr1.png](browser-600x800-dpr1.png) | 600×800 | Chromium persistent mismatch; viewport and DPR in filename. |
| [browser-600x800-dpr2.png](browser-600x800-dpr2.png) | 1200×1600 | Chromium persistent mismatch; viewport and DPR in filename. |
| [native-1288x820.png](native-1288x820.png) | 1288×820 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [native-320x568.png](native-320x568.png) | 320×568 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [native-600x800.png](native-600x800.png) | 600×800 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [native-compact.png](native-compact.png) | 390×844 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [native-short-retry.png](native-short-retry.png) | 320×320 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [native-wide.png](native-wide.png) | 1440×1000 | Native failed-update preset; short-retry shows Tab focus scrolling Retry into view. |
| [network-failure-after.png](network-failure-after.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [network-failure-before.png](network-failure-before.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [persistent-after.png](persistent-after.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [persistent-before.png](persistent-before.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [storage-denied-after.png](storage-denied-after.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [storage-denied-before.png](storage-denied-before.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [update-after.png](update-after.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |
| [update-before.png](update-before.png) | 1288×820 | Chromium 1288×820 DPR 1, scenario/state named in filename. |

The browser test uses a disposable identity/auth/static server. The storage sentinel confirms cookies/local storage/IndexedDB survive; shared tests separately cover actual draft write readiness. No real deployment or authenticated dataset was used. Native AccessKit identifies Retry and the polite status; browser accessibility exposes the canvas. Platform larger-text settings were unavailable.

Machine-readable browser results: [report.json](report.json). Actual zoom measurements: [zoom.json](zoom.json).
