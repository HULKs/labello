# Overview cue shortened to 100 ms

Fresh Chromium 149.0.7827.55 recordings of the release WASM build at 1440×1000, DPR 1, 100% zoom. Scenario: entering full-image migration discovery using disposable synthetic intercepted API responses.

- `motion-no-preference-1440x1000.gif`: 100 ms contraction/rebound/settle with system motion enabled. The sampling captured a 4.3% contraction; it does not measure the exact duration. The deterministic timing regression test establishes the 100 ms timeline.
- `motion-reduce-1440x1000.gif`: the same transition with reduced motion, with constant image width.

Recordings loop for review. The app transition runs once. Crop excludes identity bars. All image content and canvas overlays are masked; the neutral-gray rectangle retains only the displayed image bounds, and the actual outer frame remains visible. This masking is evidence redaction, not product UI.

Captured in `/home/alexschmander/.t3/worktrees/labello/t3code-b89c826d` at base `bef2b1f1e920f3c74a756c312ad509243051ef41` plus exactly `change.patch`. `source-files.json` records SHA-256 hashes of the three changed files. The PR records the resulting published commit.

Validation: focused migration timing test passed; `./scripts/verify.sh changed origin/main` passed, including all 644 UI tests, locked baseline and release Trunk build. Native inspector at 1440×1000 and 390×844, scale 1, showed the static no-adapter fallback. Browser smoke checks also exercised wheel zoom, Tab input, startup without page errors, and Canvas accessibility presence. Layout/scaling matrices were not repeated for this duration-only change. No physical-device or platform larger-text claim.

Capture tooling: the system Python 3.12 crashes even on a minimal asyncio invocation. The successful run used a disposable Python 3.13.12 environment, Pillow 12.3.0, Playwright 1.63.0, and the installed Chromium binary. An initial sampling run missed most of the 100 ms contraction; capture validation checks observable motion while exact timing remains covered by the deterministic test.
