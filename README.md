# Workflow availability icons, issue 164

Native captures use the new `workflow-availability` inspector preset with synthetic labels and no image, annotation, import, or credential content. It renders the shared production selector. The ten restrictions appear together for inspection; this is not one dataset's simultaneous availability result.

- native-wide.png: 1440x1000, scale 1, all ten restriction icons with current-workflow dot.
- native-tooltip.png: 1440x1000, scale 1, disabled balance card hovered; short explanation visible.
- native-drawer.png: 390x844, scale 1, compact scrollable drawer.
- native-short.png: 320x320, scale 1, short drawer and reachable Close action.
- native-medium.png: 600x800, scale 1, drawer showing all ten restrictions.
- native-1288.png: 1288x820, scale 1, expanded panel.
- native-320.png: 320x568, scale 1, scrollable compact drawer.

Captured from worktree t3code-3b291bbf on commit 6d7c522cb on base 706eb943608f79c6b1aece54ba9df099b40cdd7f plus a test-fixture merge correction. The resulting published commit is recorded in the pull request. The restriction-icon gallery is a new preset; there is no corresponding before capture. The original selector's boolean-only behavior is documented in the PR regression evidence.

Native screenshots prove shared rendering only. Browser evidence is recorded separately.

Final implementation commit: `fa2ef1e25958696eaf3cc49b9ee8f4939f2036c1`. No product changes followed capture; the test-fixture correction was included in this commit.

Chromium 149.0.7827.55, release WASM, disposable empty dataset on loopback ports 8184/8185:
- browser-1440x1000-dpr1.png: disabled empty-dataset tooltip, current dot, 100% zoom.
- browser-review-disabled.png: disabled review tooltip, 1440x1000, DPR 1, 100%.
- browser-check-failed.png: injected availability HTTP 503; question icon, enabled card and retry action, 1440x1000, DPR 1, 100%.
- browser-WIDTHxHEIGHT-dprN.png: empty-dataset selector at 320x568, 390x844, 600x800, 1288x820, 1440x1000, 320x320, DPR 1/2, plus 390x844 DPR 3; compact sizes show the drawer. All at 100% browser zoom.
- browser-zoom200.png: actual Chrome tab zoom 2.0, 1440x913 physical capture / 720x456 CSS viewport, effective DPR 2; workflow drawer and Close remain reachable.

Native keyboard inspection reaches Close, skips disabled cards, and Escape dismisses the drawer. Chromium received Tab/Escape during the viewport pass. Browser AX exposes only a textbox, so no browser screen-reader or focus-restoration claim is made; shared reason semantics are covered by egui_kittest and native AccessKit. Headless Linux Chromium does not expose an OS larger-text setting; that platform check is omitted.
