# UI implementation

`LabelloApp` composes explicit feature state. It does not dereference implicitly
to workflow state. Read [UI design](ui-design-guidelines.md) for presentation and
acceptance, and [architecture](architecture.md) for crate boundaries.

## State and requests

| Owner | State |
| --- | --- |
| `runtime` | API transport, command queue, responses, active requests, repainting, persistence scheduling, presence |
| `auth` | Sign-in options, session discovery, server prelabel availability, failures, account-bound recovery |
| `datasets` | Dataset metadata/users, statistics, leaderboard selection, dataset request identities |
| `admin` | Filters, snapshots, roles, staged configuration, export |
| `import` | Wizard, source registration, planning, durable job progress |
| `navigation` | App drawer, statistics modal, focus restoration |
| `work` | Assignment, annotation, review, migration, canvas, edit history |

Closed `UiCommand` and `UiMessage` enums define the async boundary. The live loop
checks request ownership before delegating to feature reducers and dispatchers.
`live/ownership.rs` owns request IDs, auth/workspace/import epochs, command
rollback, stale-response rejection, and prepared-assignment cleanup. Feature
reducers use that gate rather than introducing their own.

Structured unauthorized failures survive dispatch. After an accepted failure is
reduced, the root coalesces a session recheck. Recovery blocks work commands and
hides account-scoped content while retaining the draft for its original account.
A different account replaces the workspace. Endpoint replacement clears account,
dataset, import, and pending ownership before new requests start.

## Rendering boundaries

| Module | Responsibility |
| --- | --- |
| `setup.rs` | Login, advanced connection, pre-authentication About, dataset setup and schema-copy preview |
| `panels/app_bar.rs` | Measured global navigation, utilities, account controls, drawer collapse; remains visible during loading |
| `panels/loading_bars.rs` | Transient blank or retained view-specific bars during image loads |
| `app/shell.rs` | Layout, persistent bottom action panel, resize repaint |
| `panels/workspace.rs` | Canvas area, second-bar context, canvas controls |
| `panels/workspace_actions.rs` | Workflow commands at every width, Previous image/object |
| `panels/workspace_overflow.rs` | Action measurement, visible prefix, overflow focus and command identity |
| `panels/task_selector.rs` | Task selection; `workflow_marker.rs` owns reason icons and the committed-workflow marker |
| `panels/inspector.rs`, `panels/prelabels.rs` | Context details, annotation controls, filtered suggestions |
| `prelabel_flow.rs`, `live/prelabels.rs` | Model choice, independent current/queued hint requests, cancellation, generation invalidation, admin runs and reset, model-check request ownership |
| `panels/review_context_bar.rs`, `review_context.rs` | Exact-target identity, type, phase, version and context height |
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
disables retained controls and keyboard actions. Empty/error completion and
scope changes discard the presentation. The global app bar is outside this
blank/retained policy.

`canvas.rs` keeps public entry points; rendering, painting, interaction,
hit-testing, and viewport geometry remain separate internal concerns. Painting
owns the shared keypoint/stroke policy. Rendering exposes the same projected
states through AccessKit without image coordinates. Gesture tests stay with the
canvas. The workflow reducer retains every persisted annotation ID, including
deleted versions; Undo/Redo rebases a restored object on its latest version.
Later keypoint autosaves mark an existing skeleton as a new human-edited revision.

## Browser input

`apps/labello-wasm/src/pen_input.rs` normalizes browser pen Pointer Events before
eframe's mouse/touch compatibility listeners. Its browser-only app wrapper
feeds egui pointer events to the unchanged `LabelloApp`, including synchronous
press/release processing for browser user activation. It owns pen capture,
cancellation, and duplicate-event suppression. Annotation policy and persistence
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

## Assignment navigation and review

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
amber previews. `NextImage`, Space by default, approves an unchanged item or retains
a valid correction. Y/N actions use the same owners. Ordinary unchanged approval
uses its server command; revision approval is staged; corrected-item rejection
stays local until overview submission. Reset invalidates the affected decision.
Unchanged items still require approval, even when other items have corrections.

Overview additions retain editor and undo history after completion. The next blank
canvas placement starts another object. Reopened or recovered completed additions
are staged before another starts; editing an earlier point in an unfinished
skeleton does not overwrite it on placement. A selected positioned keypoint can
toggle Visible/Hidden; hidden placement resets to Visible after use. Delete
annotation discards the selected local addition as a whole, with text-focus,
busy-state, and overlay guards. Invalid additions block confirmation.

Only overview submission sends the complete correction batch. Failure retains
an immutable retry request. The server starts a fresh review round and enforces
all-current-item approval. Review context is derived from exact assignment and
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

Work owns automatic-workflow notices separately from transient runtime errors.
An accepted availability change captures old/new identities and presents them
before the next claim, without requiring acknowledgement. Same-pass presentation
suppresses duplicate fallback; short layouts reserve inline space and reclaim the
canvas inset. Loading/prefetch/retry keep the notice. Dismissal, explicit selection,
auth/dataset changes, or leaving work clear it.

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
