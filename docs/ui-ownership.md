# UI implementation

`LabelloApp` composes explicit feature state. It does not dereference implicitly
to workflow state. Read [UI design](ui-design-guidelines.md) for presentation and
acceptance, and [architecture](architecture.md) for crate boundaries.

## State and requests

| Owner | State |
| --- | --- |
| `runtime` | API transport, command queue, responses, active requests, repainting, persistence scheduling, presence |
| `auth` | Sign-in options, session discovery, server prelabel availability, failures, account-bound recovery, global preference loading and legacy choices |
| `datasets` | Dataset metadata/users, statistics, leaderboard selection, dataset request identities |
| `admin` | Filters, snapshots, roles, staged configuration, export |
| `import` | Wizard, source registration, planning, durable job progress |
| `navigation` | App drawer, statistics modal, focus restoration |
| `work` | Assignment, annotation, review, migration, canvas, edit history |

Closed `UiCommand` and `UiMessage` enums define the async boundary. The live loop
checks request ownership before delegating to feature reducers and dispatchers.
`live/ownership.rs` owns request IDs, auth/workspace/import epochs, command
rollback, stale-response rejection, and prepared-assignment cleanup. Feature
reducers use that gate rather than introducing their own. Each frame starts up to
eight already-queued requests in order. Dependencies still enter the queue only
after their prerequisite response; background refreshes do not add a frame each
before item activation. The bounded batch leaves rendering time for the UI.

Structured unauthorized failures survive dispatch. After an accepted failure is
reduced, the root coalesces a session recheck. Recovery blocks work commands and
hides account-scoped content while retaining the draft for its original account.
A different account replaces the workspace. Endpoint replacement clears account,
dataset, import, and pending ownership before new requests start.

## Rendering boundaries

`glossary.rs` owns canonical presentation names and definitions;
`glossary/shortcuts.rs` maps persisted action IDs to those names and their
contextual help. Renderers share that catalog in both WASM and the native
inspector. See the [product glossary](glossary.md).

| Module | Responsibility |
| --- | --- |
| `setup.rs` | Login, advanced connection, pre-authentication About, dataset setup and schema-copy preview |
| `panels/app_bar.rs` | Measured global navigation, expanding collapsed-header dataset identity, bounded presence, grouped navigation drawer; remains visible during loading |
| `panels/loading_bars.rs` | Scoped retained bars and validated inspector context during image loads |
| `app/shell.rs` | Layout, persistent bottom action panel, resize repaint |
| `panels/workspace.rs` | Canvas area, second-bar context, canvas controls |
| `panels/workspace_actions.rs` | Workflow commands at every width, unified Previous and Skip |
| `panels/workspace_overflow.rs` | Action measurement, visible prefix, overflow focus and command identity |
| `panels/task_selector.rs` | Task selection; `workflow_marker.rs` owns reason icons and the committed-workflow marker |
| `panels/inspector.rs`, `panels/prelabels.rs` | Context details, annotation controls, filtered suggestions |
| `prelabel_flow.rs`, `live/prelabels.rs` | Admin runs, reset, model checks and managed hint presentation |
| `prelabel_review.rs` | Pending editable prelabels, confirmation/deletion, sequence selection and progress; shared annotation history and browser drafts retain local changes |
| `panels/review_context_bar.rs`, `review_context.rs` | Measured inline/stacked context summary, compact progress presentation and height; exact review target identity, type, phase and version |
| `panels/overlays.rs` | Tutorial, recovery, transitions, settings, discard decisions |
| `review_corrections.rs` | Accumulated drafts, canvas previews, immutable retries, object/disposition editing |
| `review_revision.rs` | Locally staged replacement decisions and stable commit retries |
| `missing_objects.rs` | Read-only historical location evidence and browser exit warning |
| `manual_migration.rs` | Migration cursor, discovery focus, companion reconciliation |
| `workspace_canvas.rs` | Adapter from app state to reusable canvas |
| `statistics.rs`, `statistics/leaderboard.rs` | Statistics modal, periods/ranks, podium, history and activity |
| `statistics/streak.rs` | Daily flames and reduced-motion-aware goal animation from domain streak projections |
| `avatar.rs`, `statistics/avatar.rs` | Shared bounded public-avatar cache and contributor rows |
| `dataset_inspector` | Gallery, filters, preview scheduling, read-only canvas, return-to-review draft |

`panels/loading_bars.rs` scopes retained presentation to auth/workspace epochs,
view, dataset and selected workflow. It stores only display data and migration
action descriptions, never assignments, image state, drafts or command ownership.
Shell rendering refreshes it after accepted responses and navigation. Loading
disables retained controls and keyboard actions without reducing the opacity
of the containing panel or image. The shell applies the same rule to pending
navigation; actual confirmation dialogs retain their modal backdrops. Empty/error completion and
scope changes discard the presentation. The global app bar is outside this
blank/retained policy.

Same-view next/previous navigation retires assignment and request ownership while
retaining the image fields for display. `work.retired_image` marks those fields
as non-actionable. It never retains an active assignment. Work synchronization,
hint polling, canvas edits, panel actions and shortcuts cannot use retired work.
The accepted image response replaces the display together; an empty result,
failure, or scope change clears it. A settled-view marker keeps retries and later
availability checks silent even after an empty or failed result. Scope changes
reset that marker. Inspector loading presentation uses the last
validated `ReviewContext`, while live target validation remains unchanged.
Statistics, Setup lists, admin image/backup catalogs and incremental inspection
pages retain loaded data without routine refresh indicators. Errors, retries,
explicit filter changes and operation progress keep their existing feedback.
Inspect stages subsequent image pixels and annotation state in one replacement
until both requests succeed. The displayed identity, texture, state and canvas
change together; same-image refresh preserves pan and zoom. Replacement failures
keep the previous display and retry target. New selections and auth suspension
cancel obsolete requests. Return-to-review actions remain disabled during a
replacement. Admin distinguishes refresh requests from saves; an empty loaded
Setup catalog remains settled during refresh. About retains server identity
while checking it again, but the browser reload coordinator waits for the new
response before preparing or navigating. Import job status polls remain silent
while explicit operation progress remains visible.

`canvas.rs` keeps public entry points; rendering, painting, interaction,
hit-testing, and viewport geometry remain separate internal concerns. Painting
owns the shared keypoint/stroke policy. Rendering exposes the same projected
states through AccessKit without image coordinates. Gesture tests stay with the
canvas. The workflow reducer retains every persisted annotation ID, including
deleted versions; Undo/Redo rebases a restored object on its latest version.
Later keypoint autosaves mark an existing skeleton as a new human-edited revision.
Non-submitting annotation saves retain an operation-scoped background marker.
Workflow selection, local annotation edits, prelabel confirmation, and companion
guide navigation remain available while saving;
transaction guards still prevent overlapping save, submit, or release requests.
A workflow change stages the normal confirmation and waits for the save before
committing a transition. Save replies preserve newer local edits through the edit
generation check. Annotation reconciliation copies immutable origin, object group,
creation time, workflow, and annotation type from persisted state before comparing
drafts and rebasing versions. This also applies to Undo/Redo and browser recovery,
so a snapshot taken before prelabel acceptance cannot replace server provenance.
Routine saves use periodic statistics refresh; completion
requests an immediate refresh.

## Browser build recovery

`build_information` owns mismatch recovery decisions and the generation gate for
asynchronous preparation. Its browser adapter in `labello-wasm` owns the per-tab
attempt marker, fresh asset requests, and navigation. Native UI has no reload
adapter. The existing endpoint-owned identity request gate remains authoritative.

The shell queues current drafts and writes preferences before advancing recovery.
The persistence owner confirms that those writes have committed. Recovery waits
for active commands, gestures, and unrecoverable staged input. Asset preparation
never navigates; the shared owner checks current work again after it completes,
so edits made during asset downloads must also be saved. Once navigation starts,
the shell shows `Updating Labello...` and accepts no further edits. Recovery does not release
assignments or change auth/workspace epochs. Failed preparation retains the app
and exposes manual retry in About. See [deployment](deployment.md#product-build-identity)
for the cache and attempt-limit contract.

## Browser input

`apps/labello-wasm/src/pen_input.rs` normalizes browser pen Pointer Events before
eframe's mouse/touch compatibility listeners. Its browser-only app wrapper
feeds egui pointer events to the unchanged `LabelloApp`, including synchronous
press/release processing for browser user activation. It owns pen capture,
cancellation, duplicate-event suppression, and separate finger touch events.
`pointer_input.rs` retains the adapter's pointer identity and canvas hit region.
Shared scroll areas consult that identity to keep pen presses out of drag-scroll;
pen ownership also suspends egui's touch long-press timeout and restores the
configured timeout when a mouse or finger owns the pointer again. This prevents
active finger contacts from stealing a held pen's widget drag ownership.
Canvas gestures allow independent pen editing during two-finger navigation. Annotation policy and persistence
remain in the shared canvas and workflow owners. See the
[stylus input contract](stylus-input.md) for evidence boundaries.

## Browser persistence

`persistence.rs` composes record validation, normalized storage identity, retry
queues, restore orchestration, completion handling, IndexedDB, local storage,
and an in-memory test store. Server and user identity scope keys. Applying a
response or draft requires its complete identity and current workspace to match.
Browser storage is recoverable convenience; server assignments and events remain
authoritative. No second client domain model, offline authority, or synchronization
framework belongs here.

Pending prelabel objects are projected into the normal canvas and object selector,
but remain outside `work.annotations` and annotation save commands. Confirmation
transfers one object's edited geometry and its original signed evidence into the
annotation draft. Deleting a pending box marks its local object deleted, hides its
canvas geometry, and preserves selection and view until explicit confirmation
records the dismissal and advances. Pending skeleton deletion still advances
immediately. The normal history snapshot includes pending objects, so Undo/Redo
retains their geometry, pending deletions, and selection. Browser annotation drafts
store pending objects in a separate optional field and remain recoverable after autosaving confirmed work. Older drafts default
to no pending objects; older clients cannot mistake the separate field for accepted
annotations. Model and generation checks gate visibility and confirmation after
recovery. Submission cannot bypass visible pending objects. The retained workspace
bar presentation includes the current confirmation action and object progress.

## Assignment navigation and review

Annotation and Review share the measured two-line context summary and centered
canvas-control layout. Workflow and Inspector toggles stay at the left and right
edges. Each workflow projects its own progress, type and class into retained bar
presentation; image transitions retain the summary and control geometry together.
Review additionally shows the submission author avatar. The summary is passive text. Panel toggles own
Inspector opening and focus return. Profile presentation loads with review images
through `get_review_submitters` and the existing request/epoch gate. It matches
image, workflow and authoritative submitter ID before using public profile data.
Ordinary review uses `ReviewRound.submitted_by`; migration review uses the current
confirmation author. Profile-request failure falls back to the known submitter ID
without blocking review. Missing submission provenance reads Submitter unavailable.
Retained image-loading presentation includes the avatar and attribution, and is
cleared with the same view/workflow/account scope as the rest of the bar.

`app/support.rs` consumes completion shortcuts including their repeat events,
but dispatches only fresh key presses. `app/shortcuts.rs` applies this to
NextImage, review approval/rejection, and prelabel acceptance/discard. The egui
held-key state survives item changes, loading, errors, and modal guards; no
per-item cooldown or rearming is used. After shortcut dispatch, `app/shell.rs`
removes repeated Space/Enter events before focused controls render, including
dialogs and overflow menus. Text editing retains repeat input, as do unrelated
shortcuts. Pointer controls retain their normal single-click activation.

`app/transitions.rs` gates view, About, and workflow changes. Untouched work releases
before navigation; failure keeps the workspace. Pending navigation blocks edits
and further transitions. `work.assignment_touched` records edits, decisions,
correction input, migration placement/exclusions, and recovered drafts. Saving,
undoing, discarding, or reloading the same assignment does not clear it. A new
assignment resets it. Selection, pan/zoom, and target inspection alone do not set it.

Previous review first reopens and loads the previous assignment, then releases the
displaced one. Releasing first would make the displaced review the latest terminal
assignment. The current image stays visible while opening; accepted confirmation
closes its modal but keeps conflicting actions blocked until completion. Errors
preserve current work. Statistics uses its separate assignment-preserving modal.

Normal, migration, and revision review share the correction owner for item
position, validity, local decisions, navigation, reset, and overview eligibility.
Opening an editor is not a correction. Actual differences enable rejection and
amber previews. `NextImage`, Space by default, approves an unchanged item or submits
a valid correction. Y/N actions use the same owners. Objects corrections submit
immediately; Overview submits its image changes together. Historical whole-image
revision approval remains staged. Reset invalidates the affected decision.
Unchanged items still require approval, even when other items have corrections.

Overview additions retain editor and undo history after completion. The next blank
canvas placement starts another object. Reopened or recovered completed additions
are staged before another starts; editing an earlier point in an unfinished
skeleton does not overwrite it on placement. A selected positioned keypoint can
toggle Visible/Hidden; hidden placement resets to Visible after use. Delete
annotation discards the selected local addition as a whole, with text-focus,
busy-state, and overlay guards. Invalid additions block confirmation.

Objects review projects a migrated skeleton's source box as a read-only guide,
including automatic focus and Refocus. The guide never enters editable annotation
state or selectable canvas IDs. Excluded-object review exposes Create skeleton for
excluded object in the bottom action bar, with a skeleton-plus icon when the label
does not fit. The Inspector retains exclusion-reason editing.

Each item submission sends its scoped correction batch. Workflow assignments use
the loaded state's review round and target fingerprint; historical assignments
retain their captured review context. Missing context reports a recovery action
and preserves staged corrections. Failure retains
an immutable retry request. The server preserves unchanged approvals and requires
Objects review for additions and Overview review for edited images. Review context is derived from exact assignment and
target identity, never stale display state. Completed migration review retains
its outgoing position until replacement or clearing; it does not restart review.

## Migration and companions

Direct revisit uses the audited server command and returned cursor. Busy state
blocks duplicate activation; discard confirmation preserves a draft until the
new target is accepted. Failed opens/saves reuse stable retry identity. Canonical
save conflicts require discard confirmation before reloading. Returning focus
selects the overview entry or compact full-image action. The server chooses
confirmation or outstanding dependency work; the UI does not override it.
Historical passes resume the latest pass's outstanding decisions, with no new
global pass-start control. Discovery editing and companion reconciliation retain
their separate drafts and transaction rules.

`companion_guides.rs` projects untouched, unreviewed companion boxes as their
exact source keypoints in Annotate. The projection retains the box ID for
selection but never enters annotation state or save payloads. Drawing revises
that box through the normal edit/history/save path. Existing reviewed or edited
boxes remain boxes, and missing historical sources fall back to ordinary editing.
The annotation primary action advances through pending companion guides, saving
the current box without completing the assignment and focusing the next guide.
Only after all guides are drawn or deleted does it submit the image. Explicit
whole-assignment transition submission still requires every guide to be resolved.
Keypoint-guide focus permits a small canvas margin at image edges so corner
markers remain visible; it does not change annotation coordinates. Box creation
uses a three-canvas-point minimum per dimension, rather than a fixed fraction
of the image. Geometry clamping uses only a numerical floor, so small boxes are
not enlarged when rendered, hit-tested, or edited at deep zoom. Workspace
preferences are not overwritten while dataset/assignment loading or restoration
is pending, so a restored deep view survives asynchronous browser startup.

Automatic annotation focus is limited to pending/accepted prelabels and
migration companions. Manually drawn boxes keep the current zoom and pan even
after prelabel review has started; existing explicit Refocus remains available
during prelabel review.

Bounding-box assignments without a restored selection select their first visible
migration companion. Focus occurs once per activation of an annotation identity,
not each version, autosave, or manual pan/zoom. Selecting another object and
returning refocuses it; R can explicitly refocus. View padding never changes
geometry or provenance. Migration canvas preparation initializes skeleton drafts
independently of Inspector visibility.

## Images and reservations

`live_workflow::load_working_preview` uses Data Saver v1 for loads, retries,
reopen, reload, and prefetch. Failures never request Standard, legacy RGBA, or
original bytes. Native and WASM decoding use the same bounds and geometry.
`image_transfer` owns cancellation; existing request epochs reject stale replies.
Old `:data-saver` preferences are ignored. The dispatcher requests another frame
while commands remain, including after discarding superseded requests.

Claims finish independently of image cancellation. Cleanup retains dispatched
load ownership across invalidation until replies are reduced, because repeated
claims may share an assignment ID. Current/prepared work and queued commands
retain their exact reservation. New loads wait for pending releases; stale and
failed-prefetch cleanup use the original API instance.

The dataset's `preloadQueueSize` owns the annotation/review target. Resizing
returns surplus reservations to the centralized cleanup owner. Prefetch uses
the existing claim request with `prefetch: true`; a denied claim loads no image
data. The claim response distinguishes an imbalance limit from unavailable work.
The workflow tooltip always retains the loaded/target count and appends
`(imbalance limit)` for a balance pause. Expected pauses retain background retries
without appearing as load failures; transport or image-load failures retain their
failure status. Availability replies carry queue size and optional eligible reservation
IDs. A reply prunes only reservations captured by that request, so a delayed
snapshot cannot discard a newer preparation. Current work and its draft are
independent of queue reconciliation. Prepared lease deadlines use elapsed time
from claim dispatch rather than the browser's wall clock. Promotion discards
expired entries, and review retains its existing authoritative revalidation.

## Statistics and presence

Statistics data and requests belong to `datasets`; visibility and focus belong
to `navigation`. Opening or refreshing the modal changes no workspace epoch and
preserves work. Legitimate workflow replies continue beneath it. Authentication
or workspace invalidation and lost original ownership dismiss it. Recovery takes
precedence; companion reconciliation temporarily covers it and then resumes it
ahead of ordinary transition dialogs. Resize requests a sizing pass and immediate
repaint. Keyboard focus scrolls content into view; fixed headers/popups keep their
own placement.

Domain owns [scoring](scoring.md); storage selects focus and aggregates. UI never
awards points. Background statistics refresh runs every 30 seconds, or every three
seconds while the modal is open. Saves, reviews and migration completions request
an immediate refresh; in-flight requests coalesce into one follow-up. Existing
epoch gates reject stale responses, and expired or failed-refresh focus data is
hidden. Dataset-owned state retains period, contributor/history selection, streak
sorting and activity day, clamping the day after period changes.

Domain derives streaks from contributor history. `statistics/streak.rs` renders
flames and reduced-motion-aware goal animation. The app-bar flame opens Statistics.

`runtime.presence` binds samples to endpoint/account, one request, poll timing,
and consecutive failures. Every authenticated view polls every ten seconds with
an eight-second timeout; visibility return coalesces an immediate refresh. Epoch
invalidation clears names without modifying drafts or assignments. Avatar loading
shares bounded credential-free public GitHub requests and caches failures.
Last-known presence survives one or two failures; three show unavailable until
recovery. Annotation and review headers show active users; all authenticated
headers show the same connection dot. Green means connected without a pending
problem; yellow covers initial checking, one or two failed polls, saving, or
unsaved edits; red covers three failures, save failure, or application/storage
errors. Hover, activation, and keyboard focus expose the same details. Recovery
clears connection failures immediately. Presence has no daily-count footer.

## Dataset inspector

Inspect uses closed commands and the shared auth/workspace/request gate. Leaving
clears its data and cancels transfers. Workspace preferences restore the destination;
gallery filters, scroll state, and return drafts are memory-only. Same-account
recovery retains the reason and cancels obsolete reads.

Filters apply before pagination under the [API rules](api.md#dataset-inspection-and-return-to-review).
Filter menus recalculate available viewport height each frame and scroll only
when needed. Choices keep full accessible names without repeating them in hover
tooltips. The gallery loads consecutive metadata batches of up to 100 without
waiting for scrolling or previews. Counts distinguish matching images from the
loaded list. Pending filter replacement labels the still-displayed previous
results; accepted replacement resets the list, and obsolete replies are rejected.
Failures preserve displayed results and require Retry.

Virtualized rows allow two concurrent thumbnail reads and 48 cached textures.
Offscreen reads are cancelled and removed from request ownership before newly
visible previews load. Selected images immediately reuse a thumbnail while state
and Data Saver load independently. An uncached selection requests its thumbnail
even outside the visible grid. Selected-image previews take priority over new
background thumbnails. Previous/next crosses loaded-page boundaries. Pan/zoom
and intersecting workflow/status/type visibility never edit annotations.

Boxes and skeletons use class colors. A visible box supplies its object group's
label; hiding boxes restores labels on visible keypoints. Labels use screen-space
text, bounded two-line layout, full accessible names/tooltips, and canvas clipping.
Opaque class-color backgrounds choose black or white text for at least 4.5:1
contrast. Labels start inside the visible box corner when possible and move down
to avoid crowding where space permits; a label can exceed a small box's width.

Return-to-review controls remain available in the Overlays panel or drawer for
reviewers/data admins. Full-width workflow-name buttons include type icons and
selected state independently of overlay visibility. The domain's shared
`return_to_review_block` policy supplies eligibility for boxes and skeletons;
adjacent info controls expose exclusion reasons to pointer, keyboard, and
assistive-technology users. Imported workflows default to review `None`, so
completed imports need explicit approval configuration before return.

A pending request or entered reason guards navigation until completion/discard.
Failure preserves the exact retry identity; refresh prepares a new identity while
keeping the reason. Success or discard clears the draft and keeps controls
available. Notification delivery is not implemented.

## Import, export, and schema reuse

Import owns editable drafts, recovery, source registration, planning, polling,
and upload; storage owns durable lifecycle. The all-zero YOLO pose policy is an
explicit plan choice, defaults to incomplete, survives recovery, and invalidates
accepted plans on change. Its width and acknowledgement stay bounded even when
other mapping fields expand the form.

`admin.export` in `export_flow` owns saved-metadata selection, job history, summary
acknowledgement, and one pending action. Closed export commands/replies use shared
auth/workspace/dataset ownership. Invalidation clears state; queue/dispatch failure
clears pending state locally. Polling occurs at most once per second while visible.
Explicit actions can supersede polls; stale replies lose their request owner.
Refresh failure retains stale data and Retry. An uncertain mutation refreshes
history instead of blindly creating another job.

Reload restores active/Ready captures only. Terminal jobs stay inspectable in
history; changing selection clears old terminal details. Active/Ready jobs require
cancellation before another preflight. Start needs unchanged options, saved Admin
configuration, a Ready job, and explicit summary acknowledgement. Domain validates
local class mapping; server preflight owns completeness and source consistency.
Download performs authorization checks and opens the attachment URL without
buffering bytes in WASM. UI reports requested download, not transfer completion.

Setup schema reuse has a dedicated preview request without workspace switching.
Only the latest selected source can populate it. None clears the preview; a source
blocks creation until its catalog entry and preview are available. Failures never
implicitly select None. Server creation rechecks source permissions and definition
validity. See [administration](administration.md#create-or-reuse-a-schema).

## Notices and build information

Work owns automatic workflow-change feedback separately from transient errors and
saved workflow reasons. An accepted fallback captures old/new task, class and
annotation-type identities with the previous workflow's availability explanation.
Later refreshes cannot rewrite that text. Missing reasons use the generic
unavailable category. The nonmodal notice remains visible after item loading;
it does not consume confirmation shortcuts or require acknowledgment. Explicit
selection, auth/dataset changes, or leaving work clear obsolete feedback.

`workflow_reasons.rs` owns image-and-workflow-scoped saved feedback. Assignment
load fetches it with state/preview under the same ownership gate; failure fails
that load. Only reasons matching both assignment identities are installed;
unscoped reasons do not match, and no matching feedback means no notice. Optional
public GitHub author identity arrives in the same API response without a
Statistics visit. This transport enrichment changes no persisted event.

Newest messages lead and earlier/replaced feedback stays explicit. The event and
explanation remain visible, followed by workflow and status. A 24-point avatar
and username share a row with the initially collapsed Additional info disclosure;
expanded object identity and UTC time use the full width below it. The shared
avatar cache and initials fallback avoid repeated downloads; unavailable login
reads Unknown author. Message identity keeps disclosure state stable across
redraws, and multiple-message counts appear at the bottom. Long explanations
scroll; short viewports open full feedback from the event title in a bounded
non-modal window. Dismissal lasts for the opened context; accepted reload or
reopening installs a new one, and image/workflow/dataset/view changes clear it.
Revisiting completed review alone creates no notice.

Return-to-review reasons are not yet included. Object reasons survive navigation
and browser recovery by annotation identity; reset/discard clears them. Submission
combines labels, separators, and text under the existing 2000-byte limit without
truncation, preserving exact retry identity.

`build_information.rs` owns public endpoint-bound identity requests, comparison,
About, clipboard feedback, and the bottom warning. Requests coalesce and admit
one completion; endpoint changes invalidate them, while session/workspace changes
do not. Refresh clears stale server identity. WASM injects its compiled identity,
clipboard promise, and visible-focus adapter; mutable `release.json` never defines
the executing browser's identity. Copy succeeds only after the platform confirms
it; failure opens selectable manual-copy text. Mismatch navigation uses ordinary
transition guards. No mismatch means no reserved status-panel height.

The primary action in annotation, migration, and review is anchored to the bottom
right, after the secondary actions, with the same icon fallback at narrow widths.
This includes migration saving and final confirmation, and review approval or
correction submission. Compact review puts secondary actions above the decision row.

The bottom action bar remains empty until session, dataset, image, and required
assignment are loaded. Background availability refresh preserves loaded actions.
Resize measurement requests a settling repaint only when height changes, avoiding
input-dependent gaps or repaint loops at unsupported tiny sizes.

`admin/prelabel_models.rs` renders managed model inspection, named tensor selection
and explicit dataset-class mappings. Checks belong to the Admin owner and use the
existing request/epoch gate. Replies must also match the current configuration ID
and filename. Filename edits, configuration reload, save and discard clear stale
inspection state. Rendering stages profile edits; ordinary Admin save remains the
publication boundary.

## Editing after placement

New annotation boxes stay selected for movement/resizing. Completed skeletons
keep their selected annotation and expose per-keypoint Visible/Occluded controls
in the inspector when the task permits hidden points. These changes use the
same edit history, versioning, and autosave owner as geometry edits. Canvas
keypoint selection survives pointer release and is validated against the current
editable object before routing visibility changes. Placement retains the most
recent point as the editing target; next-point placement mode remains separate.

Review overview box additions retain their correction editor after staging,
matching skeleton additions. Further edits remain local until review submission.
Blank-canvas placement can start another object after retaining the current one.

Migration annotation previews keypoint placement from primary press through drag,
then sends the final clamped position to the workflow on release. Escape, lost
pointer input, read-only state, and view gestures cancel the preview. A guided
one-keypoint object's blank-canvas press repositions its existing point while
preserving its visibility and object identity. Multi-keypoint placement order
and explicit guide confirmation remain unchanged.

Migration full-image confirmation starts a missing-object skeleton on a blank
canvas press and release and selects existing objects directly. No separate Add
missing object or Edit added object buttons are shown in the inspector. The class
workflow panel selects the separate Overview queue. After a complete
one-keypoint draft, the next blank placement saves it through the existing
migration command and starts the next object only
after success. Selecting another object uses the same save-before-switch path.
The reducer retains the final dragged position in the pending canvas action and
the current draft on failure for retry. Multi-keypoint drafts require explicit
object confirmation before starting another object. Assignment completion still
requires explicit confirmation. Existing objects
retain selection priority. Completed drafts remain editable before
explicit confirmation, with placed-point visibility controls for both guided and
missing-object drafts. These controls preserve migration dirty-state ownership.

Shortcut display names, button-name aliases, category ordering and conflict-context
copy live in `panels.rs`; the settings renderer in `panels/overlays.rs` uses that
catalog for display and search. This presentation catalog does not change domain
action IDs, conflict eligibility, dispatch or persisted bindings. Row text receives
bounded width before controls, with stacked controls below 600 content points.
The existing shortcut draft/save/reducer owner remains authoritative for settings.


## Global preferences and statistics scope

`preferences.rs` owns account preference loading and the legacy-source selector.
The existing shortcut draft/save reducer remains the editing owner. Global loads
use account-bound requests without dataset identity. Accepted loads preserve an
already edited settings draft. Dataset loads do not replace shortcuts; they ensure
account preferences have loaded.
Workspace/authentication invalidation rejects obsolete requests.

`statistics/overview.rs` owns the modal's independent scope, request identity,
remote state and authorized dataset projections. `datasets.stats` remains the
active dataset projection used by the workspace streak. Scope selection starts
no workspace epoch and never replaces work metadata, annotations or assignments.
The domain aggregates counts, raw score components and contributor UTC days;
rankings, acceptance percentages and streaks derive from that combined history.
Matching task/class identifiers remain in separate source-dataset breakdowns.
Global views omit dataset scoring multipliers, focus and assignment balance.

The active dataset keeps its existing coalesced refresh path. Other scopes refresh
through the overview owner while the modal is visible. Scope changes clear the
old view before loading; accepted failures clear overview data because its
authorized membership could have changed. Authentication/workspace invalidation
clears the overview and dismisses the modal. The global endpoint fails closed
rather than returning partial aggregates. Dataset reads are sequential; there is
no cross-dataset atomic snapshot or large-server performance guarantee.

## Class workflow navigation

In Annotate and Review, `panels/task_selector.rs` groups enabled one-class tasks under their
class identity. Each class is one card with its name above equal-width activity
columns: bounding-box annotation, migration, and direct skeleton annotation when
applicable. Each split activity contains Objects and Overview buttons; an unsplit
activity uses one Annotate or Review button. Review uses bounding-box and skeleton review activities
and retains its existing task-transition and correction guards. Unconfigured activities do not take up columns;
multiple tasks within one activity use a single tile with a dropdown chevron.
The count appears in its tooltip, accessible description, and chooser. Row height
is measured from the activity labels with a stable status slot and shared padding.
The bounded chooser wraps full task names, marks the selected task, explains disabled
options, and restores focus on dismissal. Single-task activities act directly.
The shared workflow selection guard supplies both disabled state and its reason
icon for activity tiles and chooser options. Phase restrictions, all blocking
loads, saving, migration updates, transitions, and modal blocks have state-only
tooltips and accessible descriptions. No explanatory text or focus-bonus footer
appears beneath selection buttons. The availability retry control sits above
the class cards.
`panels/workflow_boost.rs` paints the boosted activity and matching chooser option
without changing layout or native button state. It uses the shared reduced-motion
preference, defaults to static rendering without an adapter, and requests frames
only during its bounded activation/hover/focus shimmer. The scoring window remains
authoritative; failed statistics or expiry remove the cue. Class cards use the
ordinary item spacing without an extra spacer between groups.
The annotation panel has bounded width; task names wrap or truncate without
expanding the canvas layout, and accessible names include class, activity, and
task. Temporarily blocked configured activities remain disabled with an explanation.

Objects and Overview select server-owned queues, not local phases of the current
image. Switching saves unfinished input and releases the departing workflow's
reservations. Overview eligibility waits for prerequisite objects; the selector
cannot bypass that gate. A migration workflow with no focusable sources uses
ordinary full-image annotation. Loading, saving and pending transitions retain
their guards, and workflow-cycle shortcuts continue selecting tasks.

## Work-item presentation

`work_items.rs` owns Objects/Overview selection, item-scoped editor projection and
backward/forward navigation over server history. `work_items/edits.rs` maps local
editors to typed durable proposals and restores them after validated assignment
loading. It never treats a browser draft as authoritative completion. Review
browser backups contain unsaved edits, not untouched selection or proposals
already saved by the server.

The command/reducer path saves partial work before Skip, Previous or departure;
failed saves retain the editor and lease. Prefetch identities include the item,
so multiple objects on one image remain distinct. Activation records display/seen;
prefetch only revalidates. Reservation cleanup waits for in-flight claims before
releasing the workflow. Shared rendering presents one Previous and Skip action.
