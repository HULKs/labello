# Mobile navigation refinement evidence

Production revision: f675a7527571eb7968a71e18852166dfbeffbb9d. Base: PR #221 at d6234df823a9d4d006c1665d563c0d21fe36bec9. Captures used e28c41190937a9ab7250661028115e72cec1e8e6 plus the eight-file refinement diff committed as the production revision above.

All captures come from the shared native inspector at scale 1 with synthetic data. Only the two top bars are included. The task-type icon replaces text in the workflow widget across all workflows and widths. Dataset pills stop at the natural text width plus padding, and the collapsed menu trigger is on the left.

| File | Scenario and viewport |
| --- | --- |
| before-icons-menu-390.png | Presence preset before this refinement, 390×844, revision e28c41190937a9ab7250661028115e72cec1e8e6 |
| before-icons-menu-600.png | Presence preset before this refinement, 600×800, same before revision |
| icons-menu-390.png | Presence preset, 390×844; left menu, fitted dataset pill and box icon |
| icons-menu-600.png | Presence preset, 600×800; dataset pill retains its natural width |
| icons-wide-1440.png | Presence preset, 1440×1000; box icon also used in the wide context widget |
| icons-zoom-layout-195.png | Presence preset, 195×422 logical points; retained targets and stacked controls |
| icons-migration-390.png | Migration review, 390×844; submitter avatar and skeleton icon |
| icons-migration-320.png | Migration review, 320×320; skeleton icon and complete control row |

Targeted local checks passed: 23 context tests, 11 presence tests, 32 navigation tests, the dataset pill width regression, and 12 retained/blank loading-bar tests. Some filters overlap. Formatting and documentation checks passed. Native build used the locked inspector manifest. At the user's request, the full suite and release browser build are delegated to CI. No new Chromium run is claimed for this refinement.
