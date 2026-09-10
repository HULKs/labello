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
- `work`: assignment, annotation, review, migration, canvas, and
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
- `panels/inspector.rs`: annotation and review controls;
- `panels/workspace.rs`: central workspace and canvas controls;
- `statistics.rs`: the dataset statistics modal and its existing metric renderer;
  `statistics/leaderboard.rs` evaluates contributor periods/ranks and renders
  podiums, the user table, history and daily activity; its selection state belongs to `datasets`;
  `statistics/avatar.rs` owns public avatar loading, caching and shared person rows;
  `statistics/streak.rs` renders daily flames from the domain streak projection;
- `panels/overlays.rs`: tutorial, recovery, transition, settings, and discard
  modals;
- `panels/prelabels.rs`: prelabel visibility and actions;
- `review_corrections.rs`: accumulated correction drafts, canvas previews, stable submission retries and object/disposition editing.
- `missing_objects.rs`: read-only historical location evidence and browser exit warning.

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
The painting owner applies one outlined-stroke and keypoint-marker policy to
annotations, drafts, correction previews, migration guides, and suggestions.
The rendering owner describes the same projected keypoint states in AccessKit;
view adapters do not define their own visibility markers.

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
a mismatch. Presence belongs in the existing application header.

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

Reservation cleanup retains dispatched assignment-load ownership across workspace
and authentication invalidation. It waits until those responses have been reduced
before releasing unused assignments, because repeated claims can return the same
active assignment ID. Current work, prepared work, and queued commands targeting an
exact assignment retain that reservation. New assignment loads wait for pending
cleanup releases to finish. Failed prefetches use the same cleanup owner as stale
responses, and cleanup uses the API instance that performed the original load.

There is no image-quality selection, per-image representation override or saved
quality preference. Old `:data-saver` browser keys are ignored. Existing image-load
failure states and retry actions reload the same Data saver profile. Cached
images never imply an active assignment or offline annotation support.

Statistics data, remote status, and active request identity remain dataset-owned.
Authenticated users with an accessible selected dataset see a daily flame in the application
bar. Statistics refresh every 30 seconds with the modal closed, every three
seconds while open, and after successful saves. A refresh requested during an
in-flight load coalesces into one follow-up load. Existing request and epoch
gates reject obsolete samples. Unavailable and stale progress are explicitly
described by the flame control, which opens Statistics for details and retry.
The current streak counts consecutive UTC days with at least 20 distinct
image/task submissions in this dataset. Yesterday's streak remains extendable
until today ends; the flame is gray until today's goal is reached. Rankings
display each person's current flame and streak independently of the period
filter. The sortable Streak column sits beside the person's name on wide layouts;
compact rows reserve the measured flame/count width beside the name and keep the
flame/count together. A touch-sized Ranking order menu replaces the sorting grid
on compact rankings. Selecting
Streak sorts current streak length longest first; selecting it again reverses
the order. Equal lengths share rank, including zero-day streaks.
The domain projection uses the existing contributor history; no new
persistent counter or wire field is required. A newly reached goal produces a
single 700 ms pulse, suppressed for reduced or unknown motion preferences.
Initial loads, account/dataset changes and refreshes of an already lit flame
do not replay the celebration.
The shared statistics renderer orders activity and rankings before aggregates.
Dataset-owned leaderboard state retains the shared period, contributor filters,
history selection and selected activity day. The day selector exposes calendar
counts without hover and clamps to the current period after a period change.
The modal scroll owner brings newly keyboard-focused content controls into view;
its fixed header and separate popup layers keep their own placement.
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

After a migration review assignment completes, its review position stays fixed
until the next assignment is installed or the current image is cleared. The
completion timestamp must not restart object review on the outgoing image.
Active assignments still recompute their position from current-round approvals.

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

## Assignment navigation

`app/transitions.rs` owns the gate for view, About, and workflow changes. An
untouched assignment releases through the existing release command without a
confirmation dialog. The destination opens only after release succeeds. Release
failure leaves the current workspace available with the error. Pending navigation
blocks background editing and further transitions; request and workspace identities
reject stale completions.

`work.assignment_touched` records input for the current assignment. Annotation
edits, entering reviewer correction, review decisions, migration keypoint or
exclusion input, and migration decisions set it. Saving, undoing, discarding a
correction, or reloading the same assignment does not clear it. Loading a different
assignment resets it. Existing server annotations and review progress, selection,
canvas pan/zoom, and migration target inspection do not set it. Recovered drafts
retain confirmation protection. This state is local to the loaded assignment;
server leases and persisted workflow history retain their existing authority.

Unsent reviewer corrections count as work for the navigation gate.
Touched assignments keep the existing confirmation. Review Previous uses the
same touched-work check: untouched reviews switch directly, while changed
reviews require confirmation. It first reopens and loads the previous review,
then releases the displaced assignment. Releasing first would make the current
review the newest terminal assignment and invalidate the previous target. Failed
reopening preserves the current workspace and reports the error. While reopening,
the current image and texture stay visible with an Opening previous review status.
After confirmation the transition modal closes, but its pending transition remains
to block conflicting actions and correction edits until loading finishes. Runtime failures
show Error in the workspace status control, with the full error and annotation
save status in its details, rather than retaining a success label. The
Previous review control belongs to the shared review footer, including
migration and compact layouts. Statistics continues to use its assignment-preserving overlay.

Normal and revision review use the same correction owner. Its interaction module
owns item position, locally decided targets, targets requiring another decision,
editor validity, navigation, reset, and aggregate overview eligibility. Valid retained
corrections satisfy their target's rejection requirement without a separate item
decision; unchanged targets still require approval. Ordinary
unchanged-item approvals retain the existing server command; corrected-item
rejections remain local until overview submission. Revision approvals retain their
existing staged decision owner. Opening the automatic editor does not mark work
changed. Only actual differences enable rejection or receive amber preview styling.
Persisted annotations remain unchanged while the canvas previews edits, additions,
removals and migration replacements. Reset invalidates the affected local decision,
and earlier corrections do not block approval of another unchanged item.

The overview is the only correction submission point and requires a decision for
every original target and valid geometry. Correction success advances the assignment
after the server commits; failure retains the draft and frozen request. Local browser
records include position, local decisions, reset targets, changes and retry request;
assignment, round and sequence validation prevent cross-workspace recovery.
Historical missing-object locations remain read-only. The second-bar review indicator
owns its measured two-line presentation and toggles the existing Inspector panel or
drawer, retaining focus-return behavior. It shows item position before workflow
identity. The shared review footer owns decision buttons and Previous, Discard and
Skip across ordinary, migration and revision review. Compact layouts keep navigation
and discard actions in a second bottom row. Remove item for added migration objects
uses the same footer and the existing local correction owner. The Inspector starts closed.
Migration canvas preparation initializes the active skeleton draft independently of
Inspector visibility, so advancing targets keeps editing available with the panel closed.

## Dataset export administration

`admin.export`, implemented in `export_flow`, owns the saved-configuration
selection, retained job history, current capture, summary acknowledgement and
one pending export action. The closed `UiCommand::Export` and
`UiMessage::ExportFinished` delegate to the export dispatcher and reducer.
The shared request identity checks auth/workspace epochs and dataset ownership
before applying any reply. Auth or workspace invalidation clears export state.
Queue and dispatch failures clear pending state and remain in the export region.

The Export section loads capabilities and server history, restores only active
or Ready captures after reload or refresh, and polls active jobs at most once per second while
that section is visible. Background polls preserve form appearance, layout, and
selection controls. An explicit action can supersede a poll; late poll responses
are rejected by the pending request ID. Explicit requests still coalesce. Failed refreshes retain the last
loaded job with a stale marker and Retry. An uncertain mutation response retries
by refreshing history, so it does not blindly create another preflight.

Selection uses saved dataset metadata, explicit task/class identities and a
versioned detect or pose profile. Train, validation, and test checkboxes are all
selected by default and filter the output independently of the fallback. At least
one split is required. Train is the default fallback for images
without split provenance. Split conflicts offer explicit per-image choices.
Domain `ExportOptions::class_mapping` supplies local compatibility feedback.
An empty task/class selection disables preflight without showing a warning.
Server preflight owns coverage, image, geometry, source consistency and bounds.
Failed, blocked, cancelled, and succeeded jobs remain inspectable in history but
are not automatically selected on reload. Editing a new selection or starting a
preflight clears old terminal-job details. Blocked jobs do not retain a payload
and do not prevent another preflight. An active or Ready capture must be
cancelled before another preflight. Start requires
a Ready job, an explicit summary acknowledgement, unchanged options and saved
Admin configuration. Captured options and bounded omission/blocker examples
remain inspectable in history.

Completed exports use the client's authorization check and open the attachment
URL with the existing browser session. WASM never buffers archive bytes. The UI
reports only that a download was requested; the browser owns transfer progress
and completion. Native inspection presets model shared states and do not create
archives. Real download behavior requires an isolated server and Chromium.

The import plan offers an explicit policy for YOLO pose rows with no placed
keypoints. The default leaves coverage incomplete. PreserveAbsent is an opt-in
assertion that all-zero entries explicitly represent absent keypoints on an
existing object. The plan request and recovery preserve this choice, changing
it invalidates an accepted plan, and encountered preservation diagnostics still
require acknowledgement before commit. This choice does not infer labels for
an unlabelled source.

The explicit all-zero YOLO pose policy section bounds its selector, help, and acknowledgement warning to the visible content width. Earlier import mapping fields may expand their parent layout; that expansion must not push this choice or its warning beyond the viewport.

## Automatic workflow changes

The work state owns availability-fallback feedback separately from transient
runtime notices. It snapshots the previous and new task/class names only when
an accepted availability result changes the committed workflow. The shared
workspace presents this nonmodal, dismissible status before claiming the next
assignment. It does not require acknowledgement to continue work.

The shared notice renderer records its current render pass when it is visible.
The central workspace suppresses its fallback only when another workspace slot
has already presented that notice in the same pass. A compact short fallback
owns an inline slot before the canvas; the shell reclaims the vertical canvas
inset for that slot, preserving the review identity and controls without covering
the image. Viewport size or presentation in an earlier frame cannot suppress the current fallback or leave a claim deferred.

Image loading, prefetch, retry and unrelated status updates preserve the notice.
A later fallback replaces it with that transition's identities. Dismissal,
committed explicit workflow selection, authentication changes, dataset changes
and leaving the work view clear it. Existing request epochs reject stale
availability results before they can change selection or feedback.


Short review layout uses the shared review-context projection to keep revision
mode in the existing context identity line. The central workspace omits its
redundant caption only when valid compact revision details are present; missing
or stale target context retains the caption fallback. This presentation does not
change captured targets, staging, or commit policy.

## Automatic workflow changes

The work state owns availability-fallback feedback separately from transient
runtime notices. It snapshots the previous and new task/class names only when
an accepted availability result changes the committed workflow. The shared
workspace presents this nonmodal, dismissible status before claiming the next
assignment. It does not require acknowledgement to continue work.

The shared notice renderer records its current render pass when it is visible.
The central workspace suppresses its fallback only when another workspace slot
has already presented that notice in the same pass. A compact short fallback
owns an inline slot before the canvas; the shell reclaims the vertical canvas
inset for that slot, preserving the review identity and controls without covering
the image. Viewport size or presentation in an earlier frame cannot suppress the current fallback or leave a claim deferred.

Image loading, prefetch, retry and unrelated status updates preserve the notice.
A later fallback replaces it with that transition's identities. Dismissal,
committed explicit workflow selection, authentication changes, dataset changes
and leaving the work view clear it. Existing request epochs reject stale
availability results before they can change selection or feedback.

Compact review availability uses a reserved slot in the identity line. Only that
truncatable line gives up text width; type/phase and canvas allocation remain
stable while loading. The shared spinner description retains its existing
progress-indicator name and tooltip across workspace placements.

The shared shell measures the compact action panel against its allocated bottom edge.
When its height changes after a resize, it requests the next repaint to settle
growing or shrinking content without waiting for pointer input. It compares the
existing panel cache with the new measurement, so unchanged clipped content at
an unsupported tiny size cannot cause a repaint loop. This preserves dynamic
wrapping and does not discard a pass or replay input commands.

## Workspace presence and connection status

`runtime.presence` owns endpoint/account identity, the current server-wide
presence sample, one outstanding request, poll timing and consecutive failures.
The shared command/reducer path polls authenticated `/presence` every ten
seconds in annotation and review workspaces. The transport times out after eight
seconds. Visibility return requests an immediate coalesced refresh. Auth and
workspace epoch invalidation clears the owner; obsolete replies cannot restore
another account's names or modify assignments, drafts or save state.

The existing application header shows presence between navigation and a compact
status dot. The dataset badge shares the header when there is enough room and yields its space to presence on narrower screens. Names form one muted horizontal
line, falling back to a people count when measured text does not fit. Hover or
activation exposes the usernames and active dataset names. An empty successful
sample reads `No active labellers`; an initial sample reads `Checking presence…`.
Presence retains the requester and deduplicates by internal ID. `PresentUser`
resolves the presentation name from `githubLogin` with internal-ID fallback.
The shared `presence.rs` renderer owns the stationary glyph-color sweep and
sparse repaint schedule. The WASM `motion` adapter observes the browser's
reduced-motion media query and updates the shared context preference, including
changes while the app is open. Unknown preferences default to static text.
There is no daily-count footer or automatic daily-count polling. The existing
daily-count API remains available for future Statistics work.

The dot replaces the workspace Idle/status pill. Green means a successful
connection with no pending problem. Yellow covers initial checking, one or two
failed polls, saving and unsaved edits. Red covers three consecutive failed
polls, save failure or application/storage errors. Hover and activation expose
connection, save and error details; keyboard focus exposes the same accessible
name. Successful recovery clears connection failures immediately. Last-known
presence remains during one or two failures; three failures display `Presence
unavailable` until a successful response arrives. The indicator does not change
assignment ownership or present unsaved work as saved.

## Migration companion annotation focus

Opening a bounding-box annotation assignment without a restored selection selects
the first visible migration companion in the selected workflow. The shared workspace canvas focuses a selected
companion once per activation, using the existing context margin and bounded zoom.
Focus tracks annotation identity rather than version, so editing, autosave and
manual pan, zoom or Fit do not repeatedly reset the view. Selecting another object
and returning focuses it again. Refocus active object, bound to R by default,
also works for selected companions. Independent companion edits retain their link and
focus behavior. Ordinary boxes retain their existing annotation viewport behavior.

The domain companion derivation supplies the initial size described in
[the API contract](api.md). Focus padding changes only the view;
it does not write geometry, provenance, or review state.
