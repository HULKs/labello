# Review summary and submitter evidence

Issue #193. Product commit `a22c02d13b0419a66bcac37634a743119404fd03`, based on PR #192 at `120f9cdef1a74c39217f9cc7da2f57147a322538`.

Before captures use the exact base inspector binary. After captures use the final shared renderer and production release WASM built from the working diff published in this commit. `source-files.json` records SHA-256 hashes of the included files. Only the UI context is published for loaded work; the crops exclude image pixels, annotation geometry and filenames. The retained-loading capture has no image or annotation content. All identities and profile pixels are synthetic.

## Captures

- `before-1440-context.png`, `before-390-context.png`: native inspector at 1440x1000 and 390x844, scale 1. The information element is also the Inspector button.
- `native-<width>x<height>-context.png`: ordinary review at 1440x1000, 1288x820, 600x800, 390x844, 320x568 and 320x320, scale 1. Passive avatar summary and separate Inspector control.
- `native-migration-390-context.png`: migration review at 390x844, scale 1; current migration-confirmation author.
- `native-correction-320-context.png`: correction state at 320x320, scale 1; summary remains aligned while the decision becomes Submit correction.
- `native-loading-390.png`: retained next-image loading at 390x844, scale 1; summary and controls are disabled together.
- `browser-1440x1000-dpr2-zoom1-context.png`, `browser-390x844-dpr3-zoom1-context.png`, `browser-320x320-dpr1-zoom1-context.png`: Chromium 149.0.7827.55, production release WASM, 100% zoom, responsive context and loaded public profile texture.
- `browser-390x844-dpr1-zoom1-fallback-context.png`: portrait HTTP 503, initials fallback, same submitter attribution and one cached request.
- `browser-1288x820-dpr1-zoom2-context.png`: actual Chrome tab zoom 200%, DPR 1, viewport 1288x820; responsive reflow retains the controls.

## Results and limits

Canonical changed-path verification passed against the recorded base. UI: 641 passed, one pre-existing ignored. Native Inspector click, focused Close, Tab/Escape focus return, and passive summary click were observed. Native processes were disconnected and stopped.

Chromium exercised 13 viewport/DPR cases, an avatar-failure case, and actual 200% zoom. Each loaded submitter metadata and one portrait request without cookies, with no page errors. Browser transport uses intercepted synthetic data; production-router tests cover authorization/public field boundaries. Real OAuth/backend persistence, OS larger-text settings and physical touch devices were not exercised. Chromium exposes its canvas wrapper rather than individual egui controls; native AccessKit/tests cover semantics, without screen-reader certification.

The reports contain aggregate checks and UI bounds only. They contain no authentication material or request/response bodies.
