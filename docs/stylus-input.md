# Stylus input

Labello handles a primary pen tip as an annotation pointer. Drag to create a box,
drag a selected box to move it, and drag its handles to resize it. Tap to place a
keypoint and drag a placed keypoint to move it. Pan mode and the configured pan
modifier still take priority. Pressure and tilt do not change annotation geometry.

## Support and conformance

The browser adapter accepts `PointerEvent` streams with `pointerType = "pen"`,
`isPrimary = true`, and primary-tip contact. It reads `pointermove` directly;
compatibility mouse movement is not required. Pen compatibility mouse events
and touch events whose `touchType` is `stylus` do not enter the canvas twice.
The browser canvas disables native page pan/zoom during direct manipulation.

| OS, browser, and event source | Evidence and support boundary |
| --- | --- |
| Ubuntu 24.04, Chromium 149.0.7827.55, CDP-emulated primary pen | Browser-generated pen events reach the real WASM client and API. Box create/move/resize, keypoint place/move, and sequential pen/mouse/touch use are checked. This is an emulated event contract, not a physical pen certification. |
| Ubuntu 24.04, Playwright WebKit 26.5, scripted primary pen | Checks the same production client with explicit DOM events, including movement without compatibility mouse events. This is desktop WebKit, not iPadOS Safari. |
| iPadOS Safari with Apple Pencil or another pen reporting `pointerType = "pen"` | Priority physical target. Device model, iPadOS/browser version, rapid repeated contact, and mixed-input results must be recorded with the procedure below before claiming verified support. Currently unverified. |
| iPadOS Firefox with the same pen | Priority physical target and a separate application test. Firefox for iOS uses WebKit, but passing desktop WebKit or Safari does not establish Firefox application behavior. Currently unverified. |
| Linux with a physical pen tablet; Android Chrome or Firefox with an active stylus | Candidate combinations. Chromium emulation does not establish OS/driver behavior. Currently unverified. |
| Passive capacitive stylus reported only as a finger, or legacy touch-only pen delivery | No distinct pen support. These follow ordinary touch behavior and cannot obtain pen-specific conflict handling. |

Mozilla documents Firefox's iOS engine in its
[Firefox for iOS architecture](https://firefox-source-docs.mozilla.org/overview/ios.html).
WebKit added generic mouse, touch, and stylus Pointer Events in
[Safari 13](https://webkit.org/blog/9674/new-webkit-features-in-safari-13/).
Neither source establishes Labello's physical-device compatibility.

The original broad US-03 stylus requirement remains **partially satisfied**.
The input adapter and emulated annotation contract have regression coverage;
iPadOS Safari/Firefox physical acceptance remains pending. No physical-device
combination is certified by the automated check.

Unsupported features include pressure-sensitive drawing, tilt-dependent tools,
eraser/barrel-button tools, Pencil hover previews beyond ordinary pointer hover,
Scribble, and OS palm-rejection guarantees. Do not use simultaneous pen and mouse
gestures. An active mouse or touch gesture blocks a new pen gesture. Touches
beginning during an active pen gesture are ignored through their final lift;
this is event arbitration, not device-level palm rejection. Lift all contacts
before switching input methods. Cancelled or lost pen capture discards the edit
preview. `Escape` also cancels an annotation drag.

## Automated browser procedure

The focused check runs the production release WASM distribution against a new
loopback API server. It creates its own synthetic image and dataset through the
API, authenticates through local development login, and verifies saved annotation
versions and geometry. It never connects to an existing server or browser profile.
Each invocation uses fresh temporary data and cleans up its server and browser
on success, failure, timeout, or interruption.

Prerequisites are CPython 3.11 or newer, the repository's locked build tools,
and Playwright's browser dependencies. Install into a private environment:

```sh
python3.11 -m venv /tmp/labello-stylus-env
/tmp/labello-stylus-env/bin/pip install -r apps/labello-wasm/tests/stylus-requirements.txt
/tmp/labello-stylus-env/bin/playwright install chromium webkit
cargo build --locked -p labello-server
cd apps/labello-wasm
trunk build --release --locked
```

Return to the repository root. Run both annotation types through Chromium's
trusted emulated pen stream, then through the explicit DOM regression streams:

```sh
for kind in bounding_box skeleton; do
  /tmp/labello-stylus-env/bin/python apps/labello-wasm/tests/stylus_input.py --kind "$kind"
  /tmp/labello-stylus-env/bin/python apps/labello-wasm/tests/stylus_input.py --kind "$kind" --events pointer-only
  /tmp/labello-stylus-env/bin/python apps/labello-wasm/tests/stylus_input.py --kind "$kind" --events dom-compat
  /tmp/labello-stylus-env/bin/python apps/labello-wasm/tests/stylus_input.py --kind "$kind" --browser webkit --events pointer-only
done
```

`pointer-only` supplies no compatibility mouse movement. `dom-compat` adds
redundant stylus touch and mouse events. These explicit DOM streams are synthetic
and alternate touch-before-pointer and pointer-before-touch delivery. They are
untrusted, unlike Chromium's CDP-generated events. They test adapter behavior,
not the events an iPad actually produces. Trusted touch injection and Chromium's
canvas accessibility node are checked only in Chromium; the report says whether
the touch check ran. WebKit does not provide the CDP injection used by that check.
The WebKit check accepts only `pointer-only`: this desktop build also lacks a
usable `Touch` constructor for the scripted compatibility stream.
Pointer-cancellation and capture-loss probes use scripted events in every mode.

Repeat relevant cases with `--width 600 --height 800 --dpr 2`,
`--width 390 --height 844 --dpr 3`, or the other applicable sizes in the
[UI matrix](ui-design-guidelines.md#verification). DPR and viewport emulation
are not actual browser zoom or a physical tablet. The automated procedure runs
at 100% zoom; use the physical procedure for actual zoom and OS behavior.

The check fails within two minutes and prints a bounded JSON outcome with browser
version, viewport, DPR, event source, and checks actually performed. It retains
no screenshots, geometry, HTTP payloads, cookies, or browser storage. It uses
screenshots only in memory to locate the generated flat-color fixture and check
drag previews. Do not extend it to arbitrary datasets or broad browser traces.
`LABELLO_TEST_WEBKIT_EXECUTABLE` can select a locally provisioned Playwright
WebKit executable when browser libraries are installed outside system paths.

Use `--negative-control --events pointer-only` to suppress pen presses. The
command must exit nonzero. The original missing movement path is reproduced by
running `--events pointer-only` against the pre-adapter distribution: the final
release position can be correct, but the moving preview assertion fails.

This focused manual browser check supplements
[canonical verification](verification.md#canonical-entry-point). It is not the
general browser workflow suite and is not automatically run by `Testing`.

## Physical iPad procedure

Use a disposable dataset on an authenticated test deployment of the exact
candidate revision. Do not enable loopback-only local administrator login on a
network-facing deployment to make it reachable from the iPad.

Record the candidate commit, iPad model, iPadOS version, stylus model, Safari
version, Firefox application version, orientation, viewport, DPR, browser zoom,
and relevant Pencil/Scribble settings. Test Safari and Firefox independently.
Use an artificial image and separate box and skeleton workflows.

1. At default zoom, create one box with the pen. Confirm its preview follows
   the tip throughout the drag, then wait for Saved. Move it and resize from
   each handle. Reload and confirm one object with the intended geometry.
2. Place every keypoint of a skeleton, then move placed keypoints. Include rapid
   successive contacts and contacts without hover. Confirm exactly one placement
   or edit per intended action, no accidental Fit, and no image pan or zoom.
3. Lift the pen, use a mouse or trackpad if available, then use the pen again.
   Separately, lift the pen, perform a two-finger pan/pinch, lift both fingers,
   Fit, and annotate again. Check saved object and keypoint counts after each.
4. While a pen drag is active, touch with a finger or palm. Confirm the pen
   preview remains correct or cancellation discards it, with no touch pan/zoom
   or unintended annotation. Keep the finger down after lifting the pen, then
   lift it and start a new gesture. Record the actual device outcome; do not
   infer palm rejection from the scripted test.
5. Start a drag and interrupt it by switching apps or changing orientation.
   Confirm no partial edit is saved and the next gesture works. Where a keyboard
   is attached, verify `Escape` cancellation and the configured pan modifier.
6. Test the surrounding controls with the pen, including opening/dismissing
   drawers, changing the workflow, Fit, Undo/Redo, and opening a browser file
   picker in the disposable admin workflow. Browser user activation must survive
   the pen adapter.
7. Repeat in landscape, portrait, Split View, and actual 200% browser zoom.
   Verify complete control reachability and that the annotation follows the tip.

Record each step as passed, failed, or unavailable with a bounded symptom.
Follow the [artifact redaction rules](operations.md#redaction). A table of
outcomes is sufficient; do not publish image content, annotation geometry,
credentials, request bodies, or broad recordings of the runtime dataset.
Promote only combinations that pass all applicable steps, and retain explicit
limitations for unavailable peripherals or untested settings.
