# Migration missing-object scan cue, issue 188

These are actual captured canvas frames, cropped to the canvas region. The interior is masked to remove all image content and annotation geometry. The image-identity bars are excluded. The masks are evidence redaction, not product UI. Full unredacted captures were inspected locally with synthetic repository fixtures.

Before revision: `0b23982f46eb00c64a606db924b5385f108918cc`.
After revision: PR #187 at `f5b0c319969faba590cb5c2371dcb8d31c8a90ed` plus the five-file issue-188 diff published in the linked pull request. All after captures were refreshed after rebasing onto that stack. The subsequent rebase onto #187 at `644fa7b3ee826a9d1c635b81ef0ccd9daa06200c` produced commit `88c079e9d502f3e41c3c0a19bb0b771213f01baa`. The migration renderer and its frame output are unchanged across that rebase, confirmed by source hash and range-diff. Browser captures were refreshed on the final release build. The production renderer SHA-256 is recorded below; documentation and test-only adjustments after capture do not alter the depicted UI.

| Capture | Scenario and viewport | Surface |
| --- | --- | --- |
| before-wide.png | Full-image migration, 1440×1000, no scan frame | Native MCP inspector, scale 1 |
| after-wide.png | Same overview, prominent double frame | Native MCP inspector, 1440×1000, scale 1 |
| before-compact.png | Full-image migration, 390×844, no scan frame | Native MCP inspector, scale 1 |
| after-compact.png | Same overview with the double frame | Native MCP inspector, 390×844, scale 1 |
| after-320x320.png | Full-image migration in a short viewport | Native MCP inspector, 320×320, scale 1 |
| after-focused-wide.png | Pending migration object, cue absent | Native MCP inspector, 1440×1000, scale 1 |
| after-focused-fit.png | Fit clicked while an object is pending, cue remains absent | Native MCP inspector, 1440×1000, scale 1 |
| after-refocus.png | Keyboard R returns to object focus, cue absent | Native MCP inspector, 1440×1000, scale 1 |
| after-discovery.png | No imported guides, full-image discovery scan | Native MCP inspector, 1440×1000, scale 1 |
| after-discovery-zoom.png | Wheel zoom within discovery scan retains frame | Native MCP inspector, 1440×1000, scale 1 |

Native inspection also covered 320×568, 600×800, and 1288×820. Geometry regression tests cover all six sizes and ensure the cue never covers the image viewport. The frame uses a fixed gutter in focused and overview phases so activation does not resize the canvas. Blue contrast against the panel is 7.02:1; the light inner frame is 14.48:1.

Browser evidence and exact published revision are recorded in the pull request. Native inspection does not prove browser behavior. Independent acceptance remains pending.
b679693d0e7f9d3a3d4bb672c102a8f865bba1267159a637eab4a61865d2d9ad  crates/labello-ui/src/manual_migration.rs

Browser captures show the production release WASM on this rebased change, using synthetic intercepted API responses. `browser-mobile-dpr2.png` shows the 390×844/DPR 2 full-image scan. `browser-zoom-200.png` shows actual 200% tab zoom in a 1288×820 outer viewport with base DPR 1. Both interiors are masked; crop dimensions preserve the rendered frame thickness. The browser matrix checks six viewport sizes at DPR 1/2, 390×844 at DPR 3, and the 200% zoom case separately. It verifies rendered frame presence, unchanged frame bounds during wheel zoom, browser startup without page errors, Tab input, and the browser Canvas accessibility node. This does not claim backend integration or complete screen-reader semantics.
