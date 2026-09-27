# Issue 201 feedback evidence

Source revision: `dab7462c53248d0baffa3d6a4cad8443a62a39be`, based on PR #199 at `dcc4f6c6984d7f21970c88c3545adeea298e34e9`.
Captures were taken from the task diff subsequently committed at that revision. Source file hashes are in `source-files.json`. No unrelated changes or lockfile updates were included.

These are new feedback views; no prior inbox or comparison view existed to capture. Native before/after images show the two versions of one synthetic correction, not two software revisions. All displayed users, annotation data and images are synthetic.

## Native inspector

- `inbox-wide.png`: anchored optional inbox with direct dismissal, 1440×1000, scale 1.
- `inbox-phone.png`: same inbox, 390×844, scale 1.
- `inbox-short.png`: bounded, scrollable inbox, 320×320, scale 1.
- `mandatory-wide.png` and `mandatory-before.png`: after/before correction, frozen required-feedback preset, 1440×1000, scale 1.
- `mandatory-phone.png`: required detail, 390×844, scale 1.
- `mandatory-short.png`: scrollable detail with reachable footer, 640×360, scale 1.

Inspector presets are frozen layout states. Functional network behavior was tested separately. Native AccessKit and egui_kittest cover semantic labels, focus and disabled controls.

## Chromium

Chromium 149.0.7827.55, release WASM built by Trunk. API responses are intercepted synthetic fixtures. Backend integration tests separately cover actual session/CSRF, storage, authorization and global admission; these captures do not claim a live OAuth/browser-to-server end-to-end test.

- `browser-progress-3.png`: third required entry acknowledged; still locked, 1440×1000, DPR 1, 100% zoom.
- `browser-progress-5.png`: all five entries acknowledged; close is available, same settings.
- `browser-cleared.png`: original review workspace restored, same settings.
- `browser-inbox-1440x1000-dpr1-zoom1-optional.png`: optional inbox, no dismissal from merely opening it.
- `browser-optional-viewed.png`: detail retained after optional acknowledgement, same settings.
- `browser-1440x1000-dpr1-zoom1-failure.png`: temporary detail failure; all five entries remain pending.
- `browser-retry-success.png`: explicit retry successfully loads and acknowledges one item, same settings.
- `browser-keyboard-next.png`: Tab/Enter advances to the next entry, same settings.
- `browser-390x844-dpr3-zoom1.png`: compact required view, DPR 3, 100% zoom.
- `browser-320x320-dpr2-zoom1.png`: short required view, DPR 2, 100% zoom; body scrolls, footer remains reachable.
- `browser-escape-1288x820-dpr1-zoom2.png`: required view after Escape/Space at actual 200% Chrome tab zoom, DPR 1.
- `browser-390x844-dpr1-zoom1-long.png` and `browser-long-explanation-scrolled.png`: a 1,728-character explanation at 390×844, DPR 1, 100% zoom. No acknowledgement until scrolling makes the image visible.

`browser-report.json` covers 320×320, 320×568, 390×844, 600×800, 1288×820 and 1440×1000 at DPR 1 and 2, plus 390×844 at DPR 3. Other reports cover five-item completion, optional direct dismissal, retries, keyboard advancement, long text and 200% zoom. No browser page errors or unintended work writes were observed. Browser accessibility exposes a canvas rather than shared widget semantics; no browser screen-reader claim is made. OS-level text scaling and physical mobile devices were not exercised.

## Machine checks

`./scripts/verify.sh changed dcc4f6c6984d7f21970c88c3545adeea298e34e9` passed: documentation audit/link checks, root and inspector formatting/Clippy, workspace tests, 654 UI tests (two pre-existing ignored tests), inspector tests, WASM check and release browser build. Five new API feedback tests cover historical attribution/comparison, idempotency, thresholds and restart latches, access/revocation, missing originals, multiple datasets, concurrent dismissal retries and fail-closed journal corruption. Added storage assertions cover additions/removals and repeated migration corrections.
