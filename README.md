# Documentation rebuild captures

These public documentation captures support the Labello documentation rebuild.
They are hosted separately from the source change and are not wiki pages.

All captures use Chromium 153.0.8010.12 at 1440×1000 CSS pixels, DPR 1 and 100% zoom.
README captures are cropped to rendered Markdown. No uncommitted source changes
were present for the final after capture.

- [README before](readme-before.png): GitHub renders the README at
  `82891d4d0f0c77f8b63751736dec2baf128aa1a0`, including the old metadata header.
- [README after](readme-after.png): GitHub renders the README at
  `5a50292fb09cf67a317f481ec945b65281993dcd`, with concise introduction and navigation.
- [Wiki Home](wiki-home.png): the new 27-page documentation index and sidebar at
  wiki commit `8aff3987039b56da10a43d49672b3cff968d45a3`. Home/sidebar match the
  documentation PR sources. The previous wiki was only an initialization page.

`capture.json` records the browser environment and source URLs.
