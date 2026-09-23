# Contributing to Labello

Use the [architecture](docs/architecture.md) to locate the behavior's owner and
read the affected [current documentation](docs/README.md) before editing. Code and
tests define current behavior; plans and historical records stay in
[docs/archive](docs/archive/README.md).

## Set up and verify

`rustup show` installs the pinned Rust 1.98.0 compiler, rustfmt, Clippy, and WASM
target. Install Trunk 0.21.14. Native Linux builds also need Wayland, X11,
XKBCommon, and OpenGL development libraries used by eframe.

Run from the repository root:

```sh
./scripts/verify.sh changed origin/main
```

The script selects documentation checks or the locked native/inspector/WASM
baseline, plus a release browser build for affected paths. An unclassified path,
unavailable required check, or stale lockfile fails verification. Use a recorded
comparison-base SHA for stacked work. The [verification guide](docs/verification.md)
defines exact commands and additional risk-specific checks.

## Make a change

1. Inspect status/diffs and preserve unrelated work. Trace the complete production
   flow, callers, failure boundaries, and platform adapters.
2. Implement the narrowest complete change at its shared owner. Add a focused
   regression test for nontrivial behavior and update its current documentation.
3. Run canonical verification and applicable manual checks. Use egui_kittest and
   the [native inspector](apps/egui-mcp-inspector/README.md) for shared UI; use
   Chromium for browser claims.
4. Fill the PR template with requirements, evidence, commands/results, omissions,
   risks, and preserved unrelated changes. Visual changes require reviewer-accessible
   screenshots or recordings when opening the draft.
5. Publish a verified draft as Awaiting CI when publication is requested. Only
   exact-current-head success of the required `Testing` check permits an authorized
   Ready for review handoff. Independent review decides acceptance.

The requested endpoint determines whether the task also owns CI fixes, assignments,
and project transitions. Follow [workflow scope](docs/verification.md#workflow-scope).
Preserve existing reviewer requests. Merging, closing issues, and marking accepted
require user authorization, passing CI, and independent review. Before coordinating
several issues or testing stacked PRs together, read [parallel development](docs/parallel-development.md).

## Edit documentation

Edit current Markdown in this repository; the wiki is generated from those files.
Run `python3 scripts/docs.py check` for links, anchors, metadata, and publication
coverage. Keep proposals and delivery records under `docs/archive/`; the publisher
rejects archive entries. See [documentation and wiki](docs/wiki.md) for the page
list, local preview, and publishing procedure. Use Git history in place of document
owner, status, date, or commit headers.
