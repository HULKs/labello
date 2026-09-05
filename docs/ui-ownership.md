# UI ownership

> **Status:** Normative current reference
> **Owner:** UI maintainers
> **Audience:** UI maintainers and contributors
> **Last verified:** 2026-07-30 at `4f9c332`

`LabelloApp` is the egui composition root. It owns navigation and the following
explicit feature states:

- `runtime`: API transport, command queue, response channel, active requests,
  repainting, and browser persistence scheduling.
- `auth`: authentication-option discovery, session state and failures, the
  active session request, and account-bound sign-in recovery.
- `datasets`: dataset metadata, users, statistics, and dataset-scoped request
  identities.
- `admin`: administration filters, snapshots, roles, and staged configuration.
- `import`: the import wizard, source registration, planning, and job progress.
- `navigation`: responsive application-drawer visibility, atomic app-bar
  collapse ownership, and focus restoration.
- `work`: assignment, annotation, review, adjudication, migration, canvas, and
  edit-history state.

Callers must name the feature owner. `LabelloApp` does not implement
`Deref<Target = WorkState>`, so an unqualified field cannot silently become
workflow state.

## Commands and responses

`UiCommand` and `UiMessage` remain closed enums. They are the static contract
between egui and asynchronous API work; they are not a general event bus.

The root live loop performs only scheduling and exhaustive delegation:

1. request ownership and epoch checks reject stale responses;
2. import, session, workflow, and support reducers apply accepted responses;
3. import, migration, auth, dataset, support, and workflow dispatchers start
   commands owned by those features.

Request IDs, auth/workspace/import epochs, command rollback, and prepared
assignment reservation release live in `live/ownership.rs`. A reducer must not
invent a second stale-response rule. Request failures retain structured
unauthorized status through the dispatcher/message boundary. After the normal
ownership checks and feature reducer, the central loop coalesces a session
recheck for an authentication rejection. Sign-in recovery blocks further work
commands, hides account-scoped rendering, and retains the draft until its owner
returns or a different account replaces the workspace. Feature reducers may update
their feature
state and the explicitly named navigation, loading, notice, or error effects
carried by the root.

## Rendering

`setup.rs` owns the dedicated login page, advanced connection view,
pre-authentication About destination, and authenticated dataset setup. The
section selector remains available across authenticated Setup destinations and
puts About last, as does the signed-out secondary navigation. Each section owns
its heading; there is no shared dataset welcome banner. Authentication methods are
hidden until both options and session discovery finish. Endpoint replacement
clears account and dataset state before scheduling requests against the new API.

Workspace rendering is grouped by the reason it changes:

- `panels/app_bar.rs` and `panels/workspace_actions.rs`: global and workflow
  actions;
- `panels/task_selector.rs`: task selection;
- `panels/inspector.rs`: annotation, review, and adjudication controls;
- `panels/workspace.rs`: central workspace and canvas controls;
- `panels/overlays.rs`: tutorial, recovery, transition, settings, and discard
  modals;
- `panels/prelabels.rs`: prelabel visibility and actions;
- `manual_migration.rs`: migration-specific workflow;
- `workspace_canvas.rs`: the adapter between app state and the reusable canvas.

The canvas keeps its public state and entry points in `canvas.rs`. Its internal
implementation is split only into rendering, painting, interaction,
hit-testing, and viewport geometry. Gesture and geometry tests stay attached to
the canvas module so these boundaries do not weaken behavioral coverage.

## Browser persistence

Browser persistence is a recoverable convenience cache, never workflow
authority. Server assignments, image state, and event history remain
authoritative.

`persistence.rs` composes focused implementations for record validation,
storage identity, the retry queue, restore orchestration, retry/completion
handling, the memory test store, IndexedDB, and local storage. Storage keys
include normalized server and user identity. A response or draft is applied
only when its complete identity and current workspace still match.

## YAGNI decisions

- Keep closed command and response enums; no dynamic message bus or reducer
  registry is needed.
- Keep one egui root; no dependency-injection container or feature framework is
  introduced.
- Keep direct feature-state mutation inside focused reducers; a second client
  domain model would duplicate server workflow rules.
- Keep the current canvas state model; no scene graph or generalized gesture
  engine is justified by the supported annotation tools.
- Keep the existing browser schemas and adapters; this refactor does not add
  synchronization, offline authority, or a new persistence format.

## Build information

`build_information.rs` owns public artifact identity state, comparison, About
rendering, clipboard feedback and the workspace status control. Server identity
uses the closed client `BuildInformationApi` capability and existing typed
`UiCommand`/`UiMessage` request ownership. Startup, About, explicit retry and the
browser focus notifier coalesce while loading. Refresh clears the old result;
endpoint changes invalidate old responses. A pending public request survives
session discovery, authentication changes, and workspace transitions because
its identity belongs to the endpoint. Its request ID admits exactly one
completion; endpoint replacement clears that owner and rejects the old result.
This metadata does not require a signed-in account or dataset.

The WASM bootstrap injects its own compiled identity and supplies the clipboard
promise and visible-focus adapters. It never treats mutable `release.json` as
the executing artifact. The shared UI announces copying only after success,
reports rejection or unavailable adapters and exposes complete selectable text
in a manual-copy disclosure. Copy failure or an unavailable adapter opens that
disclosure; egui retains its ordinary expanded state across redraws.

The lower-right mismatch control is rendered in a separate bottom status panel.
It has no workflow side effects while rendering. Activation uses `open_view`
and `PendingTransition::About`, retaining Admin, assignment and unsaved-draft
protections; cancelling leaves work intact. The panel reserves no height without
a mismatch and leaves room for future activity content to its left without
implementing activity statistics.

## Working Image Representations

`image_quality` owns the account-scoped manual Data saver choice, temporary
original-detail state, representation requests and cancellation registry.
`live_workflow::load_working_preview` owns the encoded Standard/Data Saver
capability and exactly one bounded Standard RGBA fallback. Initial annotation,
review, migration and prefetch use the selected profile; prefetch never requests
original detail. The shared client decodes under the same bounds/convention on
native and WASM; `ImagePreview::rgba` always means decoded RGBA.

A representation reply can replace only the current assignment's texture and
quality status. It cannot replace annotation/review/migration drafts, selection,
save generations, undo history or canvas transform. Initial image recovery can
apply a complete loaded assignment when no current work exists. Existing
request/auth/workspace epochs plus exact assignment/image identity reject stale
replies. Quality changes discard prepared representations, release reservations,
cancel superseded transfer futures, and refill under the selected profile.
Claim responses are allowed to finish so cancelled work can be released.

The Data saver checkbox is persisted separately from workspace location, per
normalized API endpoint and authenticated account. It does not follow viewport
or network estimates. The original-detail override lasts one image visit and is
never persisted. Context isolation cancels transfers and resets in-memory quality
state; stored per-account preference remains available for the next login.

Working views show quality selection, active/loading/failure status, explicit
original detail and retry. Compact layouts group detail actions in Image quality;
short viewports (height below 480 points) put quality controls in Settings. A
44-point context-bar button opens them and displays the active quality, preserving
canvas space. The command dispatcher schedules another frame while commands
remain, including when it discards a superseded image request.
No cached image implies an active assignment or offline annotation support.
