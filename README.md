# Documentation rebuild captures

These public documentation captures support the Labello documentation rebuild.
They are hosted separately from the source change and are not wiki pages.

All captures use Chromium 153.0.8010.12 at 1440×1000 CSS pixels, DPR 1 and 100% zoom.
README captures are cropped to rendered Markdown. No uncommitted source changes
were present for the final after capture.

- [README before](readme-before.png): GitHub renders the README at
  `49c2b2f26186138cae84fc6aa9829e51b5033044`, including the old metadata header.
- [README after](readme-after.png): GitHub renders the README at
  `fc607344f38016c35c83d5f3291951bf71ee69fa`, with concise introduction and navigation.
- [Wiki Home](wiki-home.png): the new 27-page documentation index and sidebar at
  wiki commit `8aff3987039b56da10a43d49672b3cff968d45a3`. Home/sidebar match the
  documentation PR sources. The previous wiki was only an initialization page.

`capture.json` records the browser environment and source URLs.
