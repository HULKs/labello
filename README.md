# Mouse action bindings, issue 183

Native MCP inspector, synthetic Setup preset, built from the feature worktree.
No dataset images, credentials, or annotation content are present.

- settings-1440x1000.png: Delete annotation assigned to Right click, native scale 1.
- settings-390x844.png: compact labels above controls, native scale 1.
- settings-320x568.png: complete binding row and footer, native scale 1.
- settings-320x320-footer.png: scrolled short modal with accessible Save/Cancel, native scale 1.
- settings-long-mouse-binding.png: modifier-rich mouse button 5 binding wraps, 390x844, native scale 1.
- settings-390x844-scale2.png: 390x844 logical viewport, native scale 2; capture sampled at one pixel per point.
- settings-compact-before.png: 390x844, native scale 1, initial implementation using the previous compact row/footer layout. It shows the overlap discovered during native inspection. This is a pre-layout-fix capture, not a pristine-base capture. A pristine-base capture was unavailable during the initial disk-space build failures.

After captures correspond to tracked diff SHA256 285f8d3dae74369bc03f75affc82d432e1ae417ef3d75c42853bdeffdb4e59cc on base 0b23982f46eb00c64a606db924b5385f108918cc. The browser regression script is a separate new file and does not affect native rendering. The pull request identifies the resulting published code commit.

Native inspection covered 320x568, 390x844, 600x800, 1288x820, 1440x1000, and 320x320 at native scales 1 and 2, plus long bindings, recording, Escape cancellation, save/reset, and short-modal scrolling. Chromium 149.0.7827.55 separately passed the production input checks at those viewports/DPRs, 390x844/DPR 3, and actual 200% browser zoom. OS larger-text configuration was not exposed by the headless environment; it is not claimed.
