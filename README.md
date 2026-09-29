# Issue 219 mobile bars evidence

Current production revision: e28c41190937a9ab7250661028115e72cec1e8e6. Verification base: PR #221 at d6234df823a9d4d006c1665d563c0d21fe36bec9. Stack: #222 → #221 → #223. Common integration base: 958b654ed5b2f8b00ce0d51777ace3c13df38afc.

All data is synthetic. Captures contain only application bars, navigation drawers and the image-free workflow-change dialog. Native scale is 1.

All header, review, migration, workflow-change dialog and Chromium captures were refreshed on d4478c6ca849d158a1be1d855bc7c4670870431d plus the four-file padding diff, committed as e28c41190937a9ab7250661028115e72cec1e8e6. The second bar now has six-point vertical frame padding at all widths. At 320×320, migration review and companion annotation retain 45- and 47-point canvas heights; their outer canvas gutter is six points. Bottom navigation is unchanged.

Historical captures: before-presence-390.png and before-drawer-390.png use 2d7f350cde42e04216e56c04dbcbd9ae86439204. before-padding-390.png, after-drawer-390.png, drawer-short-320.png and inspect-icon-800.png use a534d6ed7298c836f8229b3c814f9a3b5bc23878. after-keyboard-scroll.png uses ac144db3669fe79330860cdc63de08b1b8bbd739. The retained drawer and icon crops depict unchanged content.

| Image | Scenario and viewport |
| --- | --- |
| before-presence-390.png | Presence preset before change, 390×844; omitted dataset and stacked second bar |
| after-presence-390.png | Presence preset, 390×844; dataset retained and second bar compact |
| after-presence-600.png | Presence preset, 600×800; additional space available to dataset |
| after-presence-320-short.png | Presence preset, 320×320; narrow bar allocation |
| before-padding-390.png | Previous zero-padding compact second bar, 390×844; compare after-presence-390.png |
| migration-review-padding-320.png | Migration review with six-point padding, 320×320 |
| companion-padding-320.png | Companion annotation with six-point padding, 320×320 |
| before-drawer-390.png | Original navigation drawer, 390×844 |
| after-drawer-390.png | Aligned icon rows and reordered utility/account groups, 390×844 |
| drawer-short-320.png | Scrollable drawer, 320×320 |
| after-keyboard-scroll.png | Keyboard-focused Sign out scrolled into view, 320×320; Escape subsequently restores trigger focus |
| review-correction-390.png | Review correction, 390×844 |
| review-correction-320.png | Review correction wraps when needed, 320×320 |
| migration-discovery-390.png | Migration discovery retains phase context, 390×844 |
| native-195-zoom-layout.png | Extreme logical-width header and context controls, 195×422 |
| stack-workflow-change-390.png | Combined parent reason/type dialog and child bars, 390×844, native scale 1; blocked underlying navigation |
| stack-workflow-change-320.png | Combined short-screen dialog, 320×320, native scale 1; acknowledgment remains visible; keyboard behavior was tested before the padding follow-up |
| inspect-icon-800.png | Inspect uses multiple image primitives in the icon-only top bar, 800×800, native scale 1 |
| browser-390.png | Chromium review, 390×844 CSS pixels, DPR 1, zoom 100% |
| browser-zoom-200.png | Chromium review, 390×844 CSS viewport, DPR 1, actual tab zoom 200% |
| browser-drawer-zoom-200.png | Chromium scrollable drawer, 390×844, DPR 1, actual tab zoom 200% |
| browser-drawer.png | Chromium navigation drawer, 390×844, DPR 1, zoom 100% |

Chromium captures use the verification base plus task diff and production release WASM, with intercepted synthetic API responses. They demonstrate browser rendering and input, not live-server authentication or networking. Browser matrix: 320×320, 320×568, 390×844, 600×800, 1288×820, 1440×1000 at DPR 1 and 2, plus 390×844 DPR 3, failed-avatar fallback, and 200% tab zoom.
