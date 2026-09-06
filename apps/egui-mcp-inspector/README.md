# Labello egui MCP Inspector

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

The current development account has the package extracted at
`~/.local/share/labello-inspection/xvfb`. The following command works with that
installation or with system-installed Xvfb. Run it from the exact checkout root:

```sh
env -u WAYLAND_DISPLAY \
  PATH="$HOME/.local/share/labello-inspection/xvfb/usr/bin:$PATH" \
  LIBGL_ALWAYS_SOFTWARE=1 EGUI_INSPECTION=127.0.0.1:5721 \
  dbus-run-session -- xvfb-run -a -s '-screen 0 1600x1200x24 -nolisten tcp' \
  cargo run --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml \
  -- --preset setup
```

`xvfb-run -a` chooses a free display. The screen dimensions are the virtual
display capacity; use MCP `resize` to change the app window. Keep the app
running in its own terminal/process session while sending MCP commands. Capture
screenshots while the window is rendering; a minimized window can time out.

This account also has `~/.local/bin/labello-inspector-headless`, an untracked
convenience launcher. From a worktree root, explicitly select that checkout and
the allocated port:

```sh
LABELLO_INSPECTOR_ROOT="$PWD" LABELLO_INSPECTION_PORT=5721 \
  labello-inspector-headless --preset setup
```

Check the manifest exists under that root before using the shortcut. The
installed launcher falls back to the main checkout if it cannot find the
manifest; a screenshot from that fallback does not verify a worktree change.
On another account or machine, use the full Xvfb command above rather than
assuming this local launcher exists.

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

If the session cannot expose newly registered MCP tools, the server can also
be tested by a client speaking MCP JSON-RPC over its stdio transport. This
account's local `~/.local/share/labello-inspection/mcp_call.py` does that, taking
a JSON array of tool calls on stdin. It starts a separate `egui-mcp` process
per invocation; include `attach` at the start and `disconnect` at the end.
For example, with the app running on 5721:

```sh
python3 "$HOME/.local/share/labello-inspection/mcp_call.py" <<'JSON'
[
  {"tool":"attach","args":{"host":"127.0.0.1","port":5721}},
  {"tool":"query_tree","args":{"role":"Button","limit":20}},
  {"tool":"disconnect"}
]
JSON
```

For parallel runs with this helper, place a copy in each driver's artifact
directory; it writes `mcp-stderr.log` beside itself. Redirect its stdout to that
same directory. The helper and its smoke-test artifacts are installation
conveniences, not repository tools or substitutes for issue regression tests.

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

The local launcher supports concurrent instances through
`LABELLO_INSPECTION_PORT`; use a different value in each driver's command.
Keep allocation and process ownership in the orchestration handoff. A port
conflict is a failed launch, not permission to attach to or stop its occupant.

In live mode the current app initially targets `http://127.0.0.1:8080`. To use
a dedicated server on another port, select its endpoint through Advanced
connection > API URL > Reconnect before signing in or claiming work. There is
no inspector CLI endpoint flag. Configure each disposable server using the
[server configuration contract](../../docs/configuration.md), including
`LABELLO_CONFIG`, `LABELLO_BIND`, and `LABELLO_DATASETS_ROOT` where appropriate.

On 2026-09-05 at `be61e3d`, two Setup instances were exercised concurrently on
ports 5721 and 5722 with separate Xvfb displays and MCP server processes. One
opened compact Settings at 390x844 while the other retained Setup at 1288x820.
PNG dimensions and independent widget trees were checked again after both
drivers finished. This verifies native process isolation on this host; it does
not establish subagent-host MCP isolation or parallel live-server behavior.

## Presets and live mode

The default is the annotation preset. Use `-- --preset <name>` with Cargo or
`--preset <name>` with the local headless launcher for another frozen state.

Available presets are `annotation`, `setup`, `about`, `build-mismatch`,
`build-unavailable`, `review`, `review-correction`,
`adjudication`, `admin`, `statistics`, `dialog-settings`, `dialog-transition`,
`dialog-admin-discard`, `setup-failure`, `admin-failure`,
`statistics-failure`, `assignment-failure`, `image-failure`, `import-source`,
`import-preflight`, `import-ready`, `import-running`, `import-failure`,
`import-success`, `import-multiple-descriptors`, `import-yolo-splits`,
`import-server-folder-picker`, `import-server-descriptor-picker`,
`import-partial-categories`, `import-recovery-blocked`, `migration-object`,
`migration-single-optional`, `migration-exclusion`, `migration-pass`,
`migration-full-image`, `migration-review`, `migration-annotated-edit`, and
`migration-guide-deleted`. The `migration-single-optional` preset reproduces a
pending imported guide with one optional `center` keypoint and no positioned
draft input, without a server or dataset. Preset actions
are intentionally local and deterministic; restart with another preset for a
clean inspection context.

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
