# UI ownership

> **Status:** Normative current reference
> **Owner:** UI maintainers
> **Audience:** UI maintainers and contributors
> **Last verified:** 2026-07-30 at `4f9c332`

`LabelloApp` is the egui composition root. It owns navigation and the following
explicit feature states:

- `runtime`: API transport, command queue, response channel, active requests,
  repainting, and browser persistence scheduling.
- `auth`: session state and the active session request.
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
invent a second stale-response rule. Feature reducers may update their feature
state and the explicitly named navigation, loading, notice, or error effects
carried by the root.

## Rendering

Workspace rendering is grouped by the reason it changes:

- `panels/app_bar.rs` and `panels/workspace_actions.rs`: global and workflow
  actions;
- `panels/task_selector.rs`: task selection;
- `panels/inspector.rs`: annotation, review, and adjudication controls;
- `panels/workspace.rs`: central workspace and canvas controls;
- `panels/overlays.rs`: tutorial, recovery, transition, settings, and discard
  modals;
- `panels/prelabels.rs`: prelabel visibility and actions;
- `panels/workspace_overflow.rs`: secondary-action measurement, prefix promotion,
  stable command locations and overflow keyboard focus; workflow owners supply
  action order, availability and command dispatch;
- `panels/review_context_bar.rs`: measured review identity/type/phase, Inspector details
  interaction and context-row height;
- `review_context.rs`: immutable exact-target context shared by review presentation;
  assignment identity and authoritative target order/version reject stale summaries;
- `review_revision.rs`: local staged replacement decisions and stable commit retries;
  effective decisions come from the domain review projection, not raw history;
- `manual_migration.rs`: migration-specific workflow, discovered-object review focus,
  companion status and explicit reconciliation with retained drafts;
- `workspace_canvas.rs`: the adapter between app state and the reusable canvas.

The canvas keeps its public state and entry points in `canvas.rs`. Its internal
implementation is split only into rendering, painting, interaction,
hit-testing, and viewport geometry. Gesture and geometry tests stay attached to
the canvas module so these boundaries do not weaken behavioral coverage.

The shared workflow-state reducer retains every persisted annotation ID,
including deleted versions. Undo/Redo rebases a restored annotation onto that
latest authoritative version before saving; a failed save keeps the same draft
available for retry. Visible annotations remain the active projection.

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


## Current-user activity

`datasets.activity` owns the current endpoint/account/dataset identity, UTC
window, loaded counts, request, refresh deadline and local failure state.
`activity` schedules and renders the work-view bottom summary. Requests use
ordinary read identities and never begin a workspace epoch or invalidate
assignment-owned work. Successful annotation, migration and final-review
completions request a refresh; completion during an existing request schedules
one follow-up. The active work view polls no more often than every 30 seconds;
explicit completion, retry and browser visibility-return hints may refresh
sooner and coalesce while pending. The thin WASM adapter only forwards document
visibility changes.

The server sample time plus monotonic elapsed time drives UTC rollover. The
request round trip supplies a conservative transit bound: a reply that may have
crossed midnight is rejected, and existing values can expire by that bound early
rather than carry yesterday into today. An
expired window is cleared before rendering and refreshed; stale endpoint,
account, dataset or older-day replies cannot replace current counts. Initial
loading, zero, initial error with Retry, refresh and stale-on-failure states are
distinct. Loaded values remain during same-day refresh and failures. Counts are
never optimistically incremented and do not authorize workflow actions.

The shell allocates the actual summary row below primary assignment actions.
Visible wording condenses using measured text width; full counter names and
"today in UTC" remain in tooltip and AccessKit text. Retry uses a 44-point
control. This row leaves lower-right build-warning ownership with the existing
shell warning layer and requires combined layout verification when that feature
is present.

At short heights, the activity summary and migration confirmation share the
compact footer allocation. Its vertical padding stays removed when either the
activity row is present or manual annotation migration needs the compact
footer, preserving confirmation targets and canvas space in both states.

A short manual-migration annotation view places failed-activity Retry in the
existing measured secondary actions. The bottom summary retains unavailable or
stale text and exposes the Retry location through its full accessible help.
This keeps the retry target at 44 points without taking another full canvas row;
Compact short Review follows the same rule below; other views retain the direct
summary Retry control.

The same short migration/activity combination removes only the canvas's vertical
outer inset, retaining its horizontal inset. This preserves at least 44 points
of canvas height at 320×320 with loaded counts or an activity failure, alongside
44-point confirmation and Retry controls.

At compact short Review sizes, activity failures keep the summary at its normal
line height. The primary review decisions reserve the actual measured width of
an ellipsis More trigger; Retry activity and Previous navigation use the shared
secondary-action owner. The trigger has a named More action and a 44px target.
Moving a focused Previous control into its menu uses the existing overflow focus
transfer. Compact revision buttons retain explicit Stage yes/no or Commit yes/no
labels. Correction mode keeps its own actions, and migration review uses the same
bounded retry placement. This changes neither review decisions nor assignment
ownership.

Short review layout uses the shared review-context projection to keep revision
mode in the existing context identity line. The central workspace omits its
redundant caption only when valid compact revision details are present; missing
or stale target context retains the caption fallback. This presentation does not
change captured targets, staging, or commit policy.
