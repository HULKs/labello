# Stack rebase evidence

Stack: main → PR #222 → PR #221. PR #221 head `d6234df823a9d4d006c1665d563c0d21fe36bec9` is based on PR #222 head `dbaaf0be3bd5298230cf58883885c20bf089d711`.

The rebase applied without conflicts. `git range-diff` confirms the original issue patch is unchanged; the production dialog, selection and transition owner files are unchanged from the original verified source head.

`env -u NO_COLOR ./scripts/verify.sh changed dbaaf0be3bd5298230cf58883885c20bf089d711` passed on this stack head, including 682 UI tests (2 existing ignored), the inspector, WASM checks and release Trunk build. Both production browser smoke checks below passed with the rebuilt release client and disposable server:

```sh
/tmp/labello-stylus-py311/bin/python -B /tmp/labello-220-stack-evidence/browser.py annotation smoke
/tmp/labello-stylus-py311/bin/python -B /tmp/labello-220-stack-evidence/browser.py review smoke
```

The two screenshots show same-name box and skeleton workflows at Chromium 149.0.7827.55, 320×568, DPR 1, 100% zoom. Annotation shows no work remaining; review shows no work awaiting review. Both confirm Escape cannot dismiss the dialog, no claim precedes acknowledgment, and keyboard acknowledgment claims the next task. Captures contain no image or annotation content. Each rendered on its first attempt and was visually inspected.

The original full native/browser matrix remains in the parent evidence index and depicts the unchanged dialog. Only these two browser configurations were repeated after the rebase. Physical-device and full screen-reader coverage remain unavailable; independent acceptance is pending.
