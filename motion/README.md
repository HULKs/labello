# Full-image phase motion, issue 188

Captured from the production renderer at `3e570f25404866901624c0d4ac314d4bffa84916`. The recordings use Chromium 149.0.7827.55 and the locked release WASM, with synthetic intercepted API responses. The source worktree was based on `88c079e9d502f3e41c3c0a19bb0b771213f01baa` with only the motion revision diff pending at capture time. No renderer changes followed capture.

`motion-no-preference-1440x1000.gif` shows entry into the no-guide full-image scan at 1440×1000, DPR 1, 100% browser zoom. The image contracts, rebounds, then settles; the frame brightens and returns to persistent blue. The GIF repeats the captured sequence for review; the application runs it once per phase entry. `motion-reduce-1440x1000.gif` shows the same scenario with the system reduced-motion preference enabled. Its image dimensions remain constant.

Recordings are actual CDP screencast frames with captured timing. The source image pixels and any annotation geometry are removed. A neutral gray rectangle replaces the image interior at its actual rendered bounds so its size changes remain reviewable. This mask is evidence redaction, not product UI. Identity bars are cropped out. Capture sampling does not guarantee every application frame; deterministic tests check the 450 ms timing and rebound precisely.

The companion contracted/settled PNGs are frames from those recordings. `motion-report.json` records measured width ratios and capture counts. `browser-report.json` records the six supported viewport sizes at DPR 1/2 plus 390×844 at DPR 3. `browser-zoom-report.json` records actual 200% browser tab zoom. The mobile and zoom PNGs show the settled persistent frame with the interior masked.

The six `native-*.png` captures use the MCP inspector's migration-full-image preset at scale 1. They show the unchanged static fallback when no platform motion adapter is installed. Inspected sizes: 320×320, 320×568, 390×844, 600×800, 1288×820, 1440×1000. Keyboard controls and the Annotation canvas accessibility node remain present.

Before this revision, the frame was static with no image contraction. The parent directory contains those prior static-frame captures and their original provenance. No physical-device, live-backend, platform larger-text, or full screen-reader certification is claimed.
