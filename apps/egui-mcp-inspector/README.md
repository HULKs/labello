# Labello egui MCP inspector

This standalone development app exposes Labello's shared UI through eframe's
inspection protocol. It is intentionally outside the main Cargo workspace and
uses deterministic demo state by default. It is an inspection harness, not a
supported native client. Presets prove shared rendering in synthetic states;
live mode exercises the supported native API path. Chromium is still required
for production WASM and browser behavior.

## Setup and MCP connection

Install the MCP server compatible with this inspector's egui 0.35 dependency:

```sh
cargo install egui_mcp --version 0.1.0 --locked
```

For Codex, register the installed binary with an absolute path:

```sh
codex mcp add egui -- "$(command -v egui-mcp)"
```

Open a new Codex session after registration so it loads the MCP tools. OpenCode
already has the `egui` server in the repository's `opencode.json`; restart
OpenCode after changing that configuration. The agent host starts the configured
MCP bridge when it loads the tools; launch the inspector app separately, then
call `attach`.

For a machine with a working graphical display, run from the repository or
assigned worktree root:

```sh
EGUI_INSPECTION=127.0.0.1:5719 cargo run --locked \
  --manifest-path apps/egui-mcp-inspector/Cargo.toml -- --preset setup
```

Keep inspection bound to loopback. It exposes app control without
authentication. `attach` defaults to `127.0.0.1:5719`; use an explicit port for
parallel instances.

## Headless operation

Xvfb provides an in-memory X11 display; Mesa software rendering supplies the
OpenGL renderer. No monitor or physical GPU is required for this native smoke
test. Check for `Xvfb`, `xvfb-run`, `xauth`, `dbus-run-session`, and the native
eframe libraries before launching. Cargo build prerequisites are maintained in
the [verification reference](../../docs/verification.md#canonical-entry-point).

On a Debian-family system, obtain Xvfb from the host distribution's packages.
For a user-local installation, `apt-get download xvfb` downloads the package
without root, and `dpkg-deb -x <downloaded-deb> <installation-directory>`
extracts it. Check `ldd <installation-directory>/usr/bin/Xvfb` for missing
dependencies. Extraction alone does not install its dependencies.

With Xvfb installed and on PATH, run this from the exact checkout root:

```sh
env -u WAYLAND_DISPLAY \
  LIBGL_ALWAYS_SOFTWARE=1 EGUI_INSPECTION=127.0.0.1:5721 \
  dbus-run-session -- xvfb-run -a -s '-screen 0 1600x1200x24 -nolisten tcp' \
  cargo run --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml \
  -- --preset setup
```

`xvfb-run -a` chooses a free display. The screen dimensions are the virtual
display capacity; use MCP `resize` to change the app window. Keep the app
running in its own terminal/process session while sending MCP commands. Capture
screenshots while the window is rendering; a minimized window can time out.

## Development and verification loop

1. Record the worktree, `git rev-parse HEAD`, relevant uncommitted diff, preset
   or live scenario, and allocated inspection port. Build from that worktree
   with its tracked lockfile. Restart the app after code changes; a running
   inspector retains its old build.
2. Call `attach` with `{"host":"127.0.0.1","port":5721}` and verify `status`
   reports a connected `Labello MCP Inspector` on that port. Match the listener
   PID to the process you launched. The generic app label does not identify
   its checkout or revision. A listening socket alone is not a passed check.
3. Read `query_tree` before interacting. Use a node ID or an unambiguous
   role/text locator, then `click`, `type_text`, `press_key`, `scroll`, or
   `drag`. Use `wait_for` on the resulting state before asserting behavior;
   a successful input call alone does not prove the intended state appeared.
4. For a safe smoke test on the Setup preset, open Settings, wait for
   `Keyboard shortcuts`, dismiss with `Escape`, and verify that heading is
   absent. At compact widths first click `Open navigation`, then `Settings`.
   The wide app-bar button is named `Open settings`. Inspect actual names
   rather than assuming labels are identical across layouts.
5. Use `resize` and `screenshot` for the applicable
   [viewport and scaling matrix](../../docs/ui-design-guidelines.md#verification).
   A useful startup check is 1440x1000 and 390x844. Inspect the returned PNG
   as well as the widget tree. Screenshot `pixels_per_point` controls output
   resolution; it does not emulate browser DPR, zoom, or platform text scaling.
6. Map issue acceptance criteria to deterministic tests, native inspection,
   live interactions, and required Chromium checks. Record commands, outcomes,
   dimensions/scale, artifact paths, and checks not performed. Apply the
   [redaction contract](../../docs/operations.md#redaction) to logs, tree dumps,
   and screenshots. Persist only evidence free of credentials, image bytes,
   annotation geometry, review comments, uploaded filenames, and import paths.
   The Setup preset is suitable for testing screenshot capture without work
   content. Keep evidence outside tracked/runtime dataset paths.
7. Call `disconnect`, then stop the app using its owning process session.
   Disconnection only closes the MCP attachment. Let `xvfb-run` clean up its
   display and verify the allocated inspection port is no longer listening.
   Stop only your own processes.

## Parallel agents

Allocate one inspection instance per active driver. A sequential issue track
may reuse its worktree, but simultaneous implementation and independent review
need separate app instances.

| Resource | Isolation rule |
| --- | --- |
| Code and build | Use each driver's assigned worktree and its default inspector target directory. Avoid sharing an overridden `CARGO_TARGET_DIR` between independently changing branches. |
| App and display | Launch a separate process with `xvfb-run -a` and a private D-Bus session. |
| Inspection socket | Assign a distinct loopback port, for example 5721 and 5722. Record allocations before launch and check for occupied ports. |
| MCP bridge | Give each driver its own `egui-mcp` server process and attachment. |
| Evidence | Use a distinct directory per driver and tested revision, including unique screenshot filenames. |
| Live data | Give each live server its own configuration, API port, and disposable datasets root. One server process owns each datasets root. |

Each `egui-mcp` process stores one active attachment. Multiple clients can
connect to an app, but they operate on the same UI state. Two agents using a
shared MCP bridge can also change each other's attachment. Separate TCP ports
alone therefore do not isolate shared tools.

Check whether the agent host gives subagents independent MCP processes. If it
shares them, either use separate stdio clients per driver, configure distinct
named MCP servers for the concurrent drivers before starting their sessions,
or serialize inspector access. Do not assume inherited MCP tools are isolated.
Disconnect and reattach to change apps only when you own that bridge.

Keep allocations and process ownership in the handoff. A port conflict is a
failed launch, not permission to attach to or stop its occupant.

In live mode the current app initially targets `http://127.0.0.1:8080`. To use
a dedicated server on another port, select its endpoint through Advanced
connection > API URL > Reconnect before signing in or claiming work. There is
no inspector CLI endpoint flag. Configure each disposable server using the
[server configuration contract](../../docs/configuration.md), including
`LABELLO_CONFIG`, `LABELLO_BIND`, and `LABELLO_DATASETS_ROOT` where appropriate.

## Presets and live mode

The default is the annotation preset. Use `-- --preset <name>` with Cargo for
another frozen state.

Available presets are `dataset-gallery`, `dataset-inspection`, `annotation`, `presence`, `presence-fallback`, `setup`, `about`, `build-mismatch`,
`build-unavailable`, `review`, `review-correction`,
`admin`, `prelabels-disabled`, `prelabels-disabled-annotation`, `statistics`, `dialog-settings`, `dialog-transition`,
`dialog-admin-discard`, `setup-failure`, `admin-failure`,
`statistics-failure`, `streak-lit`, `assignment-failure`, `image-failure`, `import-source`,
`import-preflight`, `import-ready`, `import-running`, `import-failure`,
`import-success`, `import-multiple-descriptors`, `import-yolo-splits`,
`export-selection`, `export-loading`, `export-ready`, `export-blocked`,
`export-running`, `export-failure`, `export-success`, `export-request-failure`,
`import-server-folder-picker`, `import-server-descriptor-picker`,
`import-partial-categories`, `import-recovery-blocked`, `migration-object`,
`migration-single-optional`, `migration-exclusion`, `migration-pass`,
`migration-full-image`, `migration-review`, `migration-discovery`,
`migration-discovery-review`, `migration-companion-annotation`, `migration-annotated-edit`, and
`migration-guide-deleted`. The `migration-single-optional` preset reproduces a
pending imported guide with one optional `center` keypoint and no positioned
draft input, without a server or dataset. Preset actions
are intentionally local and deterministic; restart with another preset for a
clean inspection context. The `statistics` and `statistics-failure` presets open
an accessible, scrollable statistics modal above Setup; Escape or Close returns
to that underlying view.
The `streak-lit` preset shows a four-day labeling streak with today's goal met
in both the leaderboard and the underlying application bar.

`overlay-annotation`, `overlay-review`, `overlay-correction`, and
`overlay-migration` show visible, occluded, and not-present keypoints over
synthetic white, black, and textured regions. Annotation includes an active
skeleton draft and prelabel suggestions; correction includes a focused
occluded keypoint. These fixtures exercise the shared production painter.

To connect the inspector to a running Labello server instead, use live mode:

```sh
EGUI_INSPECTION=127.0.0.1:5719 cargo run --locked \
  --manifest-path apps/egui-mcp-inspector/Cargo.toml -- --live
```

Live mode defaults to `http://127.0.0.1:8080`. When the loopback server enables
`developmentAuth.localAdminLogin`, use `Continue as local admin` on the login
page. The inspector retains that local session without exposing credentials in
arguments, URLs, or UI fields.

Live mode uses real server state: opening work can claim an assignment, and UI
actions can modify the selected dataset. Use a disposable development dataset
for destructive testing and release or skip claimed work before exiting. Folder
upload, snapshot download, OAuth sessions, and persistent native drafts remain
browser-only or unsupported in the inspector.

Use the headless launch recipe with `--live` when no graphical display is
available. Preset or native live-mode evidence must be accompanied by the
Chromium checks required by the [verification contract](../../docs/verification.md).

The build-information presets use synthetic release identities. `about` shows
matching identities, `build-unavailable` shows the local unavailable state, and
`build-mismatch` adds the warning to an annotation workspace. Native inspection
proves shared layout and named semantics. Actual artifact binding, visible-tab
refresh and browser clipboard success/rejection require Chromium. Without a
clipboard adapter, the native About screen offers selectable manual-copy text
and reports that automatic copying is unavailable.

The presence presets show the current user and a local-ID fallback in the shared
work header. `presence` seeds the shared avatar cache with a synthetic image;
`presence-fallback` seeds an unavailable photo. Both are deterministic and make
no avatar requests. Hover and activation expose the GitHub handle and dataset
names. Photos and initials are static. Browser avatar networking requires Chromium.

The `workflow-reasons` preset shows a synthetic rejection comment and earlier approval
feedback alongside a workflow-change notice. Use it to inspect event headings,
message-first ordering, scrolling, dismissal and the short-viewport feedback window; it does not prove history loading or browser
behavior.

The `review-initial-load`, `review-next-image`, and `migration-next-image`
presets freeze an image request with no active image. Initial review leaves
the context and action bars blank; next-image presets retain the preceding
view's disabled bar presentation. They exercise the shared presentation owner,
not live networking. Use Chromium with delayed responses for the actual request
transition.

The `workflow-availability` preset shows all ten server restriction icons,
including a selected unavailable workflow, without image or annotation content.
Use it to inspect disabled tooltips, marker alignment, and the workflow drawer.
