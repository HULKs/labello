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
  collapse ownership, statistics-overlay visibility, and focus restoration.
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
- `statistics.rs`: the dataset statistics modal and its existing metric renderer;
- `panels/overlays.rs`: tutorial, recovery, transition, settings, and discard
  modals;
- `panels/prelabels.rs`: prelabel visibility and actions;
- `missing_objects.rs`: assignment-and-round-scoped missing-object drafts, stable rejection retries, inspector guidance/history, and browser exit warning; canvas rendering owns marker transforms and gestures.
- `review_revision.rs`: local staged replacement decisions and stable commit retries;
  effective decisions come from the domain review projection, not raw history;
- `manual_migration.rs`: migration-specific workflow, discovered-object review focus,
  companion status and explicit reconciliation with retained drafts;
- `workspace_canvas.rs`: the adapter between app state and the reusable canvas.

The canvas keeps its public state and entry points in `canvas.rs`. Its internal
implementation is split only into rendering, painting, interaction,
hit-testing, and viewport geometry. Gesture and geometry tests stay attached to
the canvas module so these boundaries do not weaken behavioral coverage.
The painting owner applies one outlined-stroke and keypoint-marker policy to
annotations, drafts, correction previews, migration guides, and suggestions.
The rendering owner describes the same projected keypoint states in AccessKit;
view adapters do not define their own visibility markers.

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

Continuing an active skeleton after an earlier keypoint autosave creates the next
human-edited annotation revision. The edit owner marks the persisted annotation
modified before saving later keypoints, preserving Visible, Occluded and
coordinate-free Not present outcomes through save and reload.

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

## Working image previews

`live_workflow::load_working_preview` always requests the encoded Data Saver v1
profile for annotation, review, migration, assignment reload/reopen and prefetch.
It propagates transfer and decode failures without falling back to Standard,
legacy RGBA or original bytes. The shared client decodes under the same bounds
and geometry convention on native and WASM.

`image_transfer` owns transfer cancellation. Existing request/auth/workspace
epochs own stale-response rejection, assignment identity and image-reference
cleanup. Claims finish independently so obsolete reservations can be released.
The command dispatcher schedules another frame while commands remain, including
when it discards a superseded request.

There is no image-quality selection, per-image representation override or saved
quality preference. Old `:data-saver` browser keys are ignored. Existing image-load
failure states and retry actions reload the same Data saver profile. Cached
images never imply an active assignment or offline annotation support.

Statistics data, remote status, and active request identity remain dataset-owned.
The navigation-owned modal does not perform an assignment transition or start a
workspace epoch. Refresh uses the existing request/epoch gate and may run while
assignment requests are active. Authentication/workspace invalidation dismisses
the modal; losing its original assignment dismisses it without restoring work.
Viewport changes trigger one modal sizing pass and an immediate follow-up repaint,
so the open overlay stays constrained without waiting for statistics refresh or
new input. This geometry cache belongs to egui and carries no workflow state.

Required draft recovery and migration companion reconciliation take precedence
when Statistics is open. Recovery clears the overlay under the existing recovery
rules; reconciliation temporarily covers it without discarding its open state.
After reconciliation is cancelled or completed, Statistics resumes ahead of
ordinary revisit/assignment-transition dialogs. Closing Statistics preserves the
underlying migration assignment and draft.

## Direct Migration Revisit

At full-image confirmation, the resolved-object overview uses named buttons and
completed canonical skeletons and excluded guides can be selected on the canvas.
Selection submits the existing audited revisit command. Busy/loading state blocks
duplicate activation; canvas drags remain pan/edit gestures. Discard confirmation
retains the current draft until the server accepts the new target. Failed opening
can retry the exact request; repeated unchanged migration saves reuse their
idempotency identity until acknowledged. Reloading after a canonical edit conflict
requires discard confirmation; cancelling preserves the draft.

The server owns the returned cursor. A direct save returns to full-image
confirmation when all other targets are resolved and fresh, or to outstanding
correction work when a dependency changed. The UI does not force confirmation.
On return, keyboard focus goes to the overview entry, or the full-image primary
control when the compact inspector is closed. Additional discovered objects keep
their separate direct-edit and companion reconciliation workflow.

The browser and shared UI do not start global correction passes. Existing passes
remain readable. Reloading an assignment resumes its latest persisted pass at the
first outstanding object. The normal keep, edit and exclude controls record exact
current decisions until full-image confirmation is available. Resolved overview
entries then use the same direct revisit path as assignments without a pass.
