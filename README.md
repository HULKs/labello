# PR 149 visual evidence

Screenshots for https://github.com/HULKs/labello/pull/149 and issue #146.

After captures were made during verification of the source published as
344f95b319332f402d50305f7d79ccc9ee4ab595, using the release WASM build,
Chromium 153.0.8010.12, a disposable local server and synthetic presence data.
The Octocat photo was fetched from GitHub. No real annotation data is shown.

- before-handle.png: historical browser capture from PR #122 at
  8a2b785f8f49019ce4d625fed24005b399d01595. Illustrates the previous @handle
  presentation, not an exact capture of PR #149's comparison base.
- after-avatar.png: 1440x1000 viewport, DPR 1; header cropped to 56px high.
- identity-details.png: 390x844 viewport, DPR 1; activated details with handles
  and datasets. The same content appears on hover. Cropped to the top 250px.
- compact-overflow.png: 320x320 viewport, DPR 1; header with +11 hidden users.
- failed-photo-fallback.png: 390x844 viewport, DPR 1; initials remain after a
  failed avatar request. Header cropped to 56px high.

These are browser screenshots, not generated mockups. This evidence branch is
independent of the reviewed implementation branch.
