# Issue 182 UI evidence

Native captures show only the workspace action bar. Source images, annotation geometry, and identifying image text are excluded.

The native inspector was built from `fix/migration-missing-object-actions`, based on `0b23982f46eb00c64a606db924b5385f108918cc`, with the task diff present. The resulting code commit is `95c002d07d25fa14573443e77b72d88909927273`; these captures show its exact production source. `source-files.json` records SHA-256 fingerprints of the reviewed files. No production rendering change was made after these captures.

- `overview-1440x1000.png`: full-image Submit at the bottom right, native scale 1.
- `editing-1440x1000.png`: a partial multi-keypoint addition, Remove added object available and Save missing object disabled, native scale 1.
- `editing-320x320.png`: the same action set condensed to accessible icon buttons at the short compact viewport, native scale 1.
- `deleted-390x844.png`: after Delete, the editor closes and Submit returns; no discard step, native scale 1.

`native-report.json` records all six inspected sizes: 320x320, 320x568, 390x844, 600x800, 1288x820, and 1440x1000. Checks cover create, Delete, action availability, and final Submit. Native output scale is not browser DPR.

Before captures are unavailable: the initial baseline verification ran out of disk space before visual evidence was collected. The regression record identifies the original label and shared Delete/Undo handler directly in the base revision.

Browser checks used Chromium 149.0.7827.55 and the locked production release WASM build with synthetic API responses. All 26 schema/viewport/DPR cases passed: one and two points, each at 320x320, 320x568, 390x844, 600x800, 1288x820, and 1440x1000 with DPR 1 and 2, plus 390x844 at DPR 3. Both schemas also passed at 1288x820 with actual Chrome tab zoom 200%, set and read back through chrome.tabs.setZoom/getZoom. Browser screenshot/input coordinates were normalized for that zoom.

- `browser-confirm-required-2point-1288x820-dpr1-zoom2.png`: completed two-point draft remains in object confirmation after clicking another empty location; full browser viewport 1288x820, DPR 1, actual zoom 200%.
- `browser-deleted-1point-1288x820-dpr1-zoom2.png`: persisted object deletion returns to image Submit at the same viewport and zoom.
- `browser-deleted-1point-390x844-dpr3-zoom1.png`: mobile image Submit after deletion, DPR 3, zoom 100%.

Captures show only controls. `browser-report.json` records all 28 cases. This validates actual WASM input/rendering and request selection with mocked transport; it does not validate live server persistence or authentication. CPython 3.11 was used after the older Python 3.13 Playwright environment crashed. OS-level text enlargement, physical stylus hardware, and platform screen readers were not exercised.
