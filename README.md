# Documentation rebuild captures

These public documentation captures support the Labello documentation rebuild.
They are hosted separately from the source change and are not wiki pages.

All captures use Chromium 153.0.8010.12 at 1440×1000 CSS pixels, DPR 1 and 100% zoom.
README and guide captures are cropped to rendered Markdown. The source checkout
was clean during capture.

- [README before](readme-before.png): GitHub renders the README at
  `40398e8d3b43fdae70f9596d4909d00ace8a7219`, including the old metadata header.
- [README after](readme-after.png): GitHub renders the README at
  `66c7a3f2c30ba83aece77b7863083f06ff5c7d8b`, with concise introduction and navigation.
- [Inspector guide before](inspection-guide-before.png): the inspection section
  of `docs/administration.md` at previous draft head
  `fc607344f38016c35c83d5f3291951bf71ee69fa`.
- [Inspector guide after](inspection-guide-after.png): the same section at
  `66c7a3f2c30ba83aece77b7863083f06ff5c7d8b`, with current gallery loading, filter,
  label and return-to-review behavior.
- [Wiki Home](wiki-home.png): the new 27-page documentation index and sidebar at
  wiki commit `8aff3987039b56da10a43d49672b3cff968d45a3`. Home/sidebar match the
  documentation PR sources. The previous wiki was only an initialization page.

`capture.json` records the browser environment and source URLs/revisions.
