# Issue 219 mobile bars evidence

Production revision: ac144db3669fe79330860cdc63de08b1b8bbd739. Verification base: 958b654ed5b2f8b00ce0d51777ace3c13df38afc.
All data is synthetic. Only application bars and navigation drawers are included.

Native inspector uses scale 1. Before images use 2d7f350cde42e04216e56c04dbcbd9ae86439204. After images use the task diff published as the production revision above. The final keyboard capture was refreshed on the verification base; other native images precede the base update, which did not change their depicted states.

| Image | Scenario and viewport |
| --- | --- |
| before-presence-390.png | Presence preset before change, 390×844; omitted dataset and stacked second bar |
| after-presence-390.png | Presence preset, 390×844; dataset retained and second bar compact |
| after-presence-600.png | Presence preset, 600×800; additional space available to dataset |
| after-presence-320-short.png | Presence preset, 320×320; narrow bar allocation |
| before-drawer-390.png | Original navigation drawer, 390×844 |
| after-drawer-390.png | Aligned icon rows and reordered utility/account groups, 390×844 |
| drawer-short-320.png | Scrollable drawer, 320×320 |
| after-keyboard-scroll.png | Keyboard-focused Sign out scrolled into view, 320×320; Escape subsequently restores trigger focus |
| review-correction-390.png | Review correction, 390×844 |
| review-correction-320.png | Review correction wraps when needed, 320×320 |
| migration-discovery-390.png | Migration discovery retains phase context, 390×844 |
| native-195-zoom-layout.png | Extreme logical-width header and context controls, 195×422 |
| browser-390.png | Chromium review, 390×844 CSS pixels, DPR 1, zoom 100% |
| browser-zoom-200.png | Chromium review, 390×844 CSS viewport, DPR 1, actual tab zoom 200% |
| browser-drawer-zoom-200.png | Chromium scrollable drawer, 390×844, DPR 1, actual tab zoom 200% |
| browser-drawer.png | Chromium navigation drawer, 390×844, DPR 1, zoom 100% |

Chromium captures use the verification base plus task diff and production release WASM, with intercepted synthetic API responses. They demonstrate browser rendering and input, not live-server authentication or networking. Browser matrix: 320×320, 320×568, 390×844, 600×800, 1288×820, 1440×1000 at DPR 1 and 2, plus 390×844 DPR 3, failed-avatar fallback, and 200% tab zoom.
