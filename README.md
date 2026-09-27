# Shortcut settings evidence for issue 190

Production change: `016d0dd88753f3fcdf9e47d0b9d5fba3f50eb835`.
Comparison base: `0b23982f46eb00c64a606db924b5385f108918cc`.
Captures were taken from the working diff subsequently published as that commit.
`source-files.json` records the final six source/document file hashes.

## Before and after

- `before-1440.png`, `before-390.png`: native Setup preset at the comparison base,
  1440x1000 and 390x844 logical points, scale 1. Descriptions and names run under buttons.
- `native-1440x1000.png`, `native-1288x820.png`, `native-600x800.png`,
  `native-390x844.png`, `native-320x568.png`, `native-320x320.png`: shared production
  settings layout in the native Setup preset, scale 1. Bounded text columns on
  wide layouts, stacked compact rows, grouped footer, whole-modal scrolling when short.
- `native-aliases-390.png`: Next guide finds Confirm / submit; expanded button names,
  390x844, scale 1.
- `native-recording-390.png`: selected recording state and Escape instruction,
  390x844, scale 1.
- `native-conflict-390.png`: peer name/context, conflict filter and disabled Save,
  390x844, scale 1.

The final schema-specific keypoint-name search addition followed native capture;
it does not affect the depicted Setup states. The final production release WASM
captures below include that addition. Native evidence proves shared rendering
and AccessKit semantics, not browser behavior.

## Chromium

Chromium 149.0.7827.55, locked production release WASM build. Transport responses
were synthetic and intercepted in the browser; Setup has no dataset or image.
The 13-case matrix covers 320x320, 320x568, 390x844, 600x800, 1288x820 and
1440x1000 at DPR 1 and 2, plus 390x844 at DPR 3. Reports retain all case outcomes.
Published representative captures:

- `browser-1440x1000-dpr2-zoom1.png`: desktop columns, DPR 2, 100% zoom.
- `browser-390x844-dpr3-zoom1.png`: compact rows/footer, DPR 3, 100% zoom.
- `browser-600x800-dpr2-zoom1.png`: medium-width stacked rows, DPR 2, 100% zoom.
- `browser-320x568-dpr2-zoom1.png`: narrow settings, DPR 2, 100% zoom.
- `browser-1288x820-dpr1-zoom2.png`: actual Chrome tab zoom 200%, DPR 1;
  chrome.tabs.setZoom/getZoom confirmed 2. The short effective viewport scrolls.
- `browser-search.png`, `browser-recording.png`, `browser-modified.png`:
  Next guide search, Tab/Enter recording, then F9 captured with visible focus;
  1440x1000, DPR 1, 100% zoom.
- `browser-saving.png`, `browser-save-error.png`, `browser-discard.png`:
  pending save, synthetic HTTP 503 retains F9, then Escape opens discard;
  1440x1000, DPR 1, 100% zoom. The intercepted save payload contained F9.

The browser matrix checks startup and Escape dismissal; the separate interaction
case checks real keyboard recording and save-failure presentation. Per-control
browser accessibility is not exposed by the current canvas adapter; CDP reports
Canvas/textbox roles. Native AccessKit and deterministic tests cover control names
and disabled/selected semantics. OS-level text enlargement and platform screen
readers were not exercised. This does not validate live backend authentication or
persistence. No credentials, runtime datasets, image content, annotation geometry,
request bodies or browser traces are included.

## Checks

- `./scripts/verify.sh changed origin/main`: passed on final change; selected UI
  and browser profiles, complete locked baseline and release Trunk build.
- UI suite: 615 passed, one existing ignored test.
- Focused tests cover aliases (including actual schema keypoint names), category
  continuity, text/control non-intersection with long chords, peer conflicts and
  filtering, final-row/footer scroll reachability, existing save/reset/cancel/error
  and input-blocking behavior.
- `python3 scripts/docs.py check`: 62 Markdown files and 29 wiki pages passed.
- `git diff --check`: passed.
