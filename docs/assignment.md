# Assignment

Labello assigns work per enabled task. Availability and prepared queue results
are advisory; the storage claim transaction repeats the same eligibility checks
before creating an assignment.

Annotation requires annotator authority; review requires reviewer authority.
Mutations bind the exact image, task, actor, assignment kind, and live lease.
Claim retries and successful assignment-backed writes renew the 30-minute lease.
Prepared queues default to two future assignments in addition to current work.
Dataset administrators configure their target size. Availability is advisory;
prefetch claims account for outstanding assignments before reserving more work.

For controls and the user sequence, see [annotation and review](annotation.md).

## Objects and Overview

Each task has an Objects queue and an Overview queue where applicable. A queue
item is one object, including a whole skeleton, or one image respectively.
The item lease captures its task definition, source identity and exact review
version. Different objects on an image may have different owners; Overview
excludes simultaneous object leases for that task.

Annotation prepares existing annotations, migration targets and managed prelabels
before assigning work. Images with no focusable sources use Overview directly.
The selector omits Objects when the workflow has no prepared objects. Pending
model generation blocks unused work until it finishes or an administrator changes
the managed configuration. Empty model results allow normal image annotation.

Confirming an object completes that item immediately and leaves the image's
Overview outstanding. Skipped and partial objects still block Overview. Completing
annotation Overview submits an approval task for review, or completes a task with
review workflow `none`. Review Objects includes annotations added during annotation
Overview. Review Overview waits for every current object decision and completes
the task. Existing completed tasks are not reopened to impose new passes.

## Review eligibility and corrections

Objects review excludes the object's final author. Earlier partial contributors
may review it. Overview review excludes anyone who annotated, corrected, or
recorded a review decision anywhere on the image, across tasks. Merely displaying
or prefetching work does not count as a contribution.

A reviewer may receive otherwise excluded work only when no independent review
item is available in any accessible dataset review queue. Storage checks this
under dataset admission at claim, display, mutation and history reacquisition.
The assignment records the exception durably. Its decision is final and satisfies
completion; it does not create a later independent-review obligation.

Approve confirms the current unchanged item. A correction must change geometry,
add/remove an annotation, or change a migration disposition. Comment-only and
unchanged corrections cannot reject work. Objects corrections submit immediately
without entering Overview. Overview edits missing objects in place.

Corrections preserve approvals for unchanged objects. Added objects require an
Objects review, then Overview. Editing an existing object requires another
Overview, without an additional Objects pass solely for that edit. The correction
receipt binds the exact corrected versions. Canonical migration geometry retains
its guide/group identity; discovered skeletons retain their companion boxes.

The correction transaction checks ownership, captured definition, current versions
and review eligibility, simulates the entire batch, then atomically publishes
geometry, attribution, decision and receipt. Exact retries are idempotent. Changed
retries, stale versions and lost leases append nothing. Historical whole-image
review and revision records remain replayable with their original meaning.

## Previous, Skip and partial work

Previous follows displayed items within a task, assignment kind and variant.
`workflowQueue.historyDepth` defaults to 5 and accepts 0 through 100. The server
keeps the current visit plus that many earlier visits. Returning from C to B to A
preserves forward reservations; advancing returns through B and C. Reopening does
not reorder the visit or duplicate its reward. The history index is derived from
events and rebuilt after restart.

Skip saves unfinished work before releasing its lease. Claims prefer other
eligible items before returning the skipped one. Object drafts persist partial
keypoints without confirming the skeleton. Overview additions and reviewer edits
are durable proposals, separate from committed annotations and review decisions.
Another eligible worker can resume them. Drafts use exact item scope, geometry
validation and compare-and-swap event sequences. Only explicit valid confirmation
finishes the item and earns its applicable reward.

Leaving a workflow releases its current, prepared and retained history leases,
while preserving saved work and visit history. Browser loss uses ordinary lease
expiry. Returning to released or expired history rechecks owner, current version,
role, definition and review eligibility. Changed or reassigned history is rejected;
a failed opening preserves the current workspace.

Existing pre-queue assignments continue through their legacy transaction rules.
Their historical completion and scores are preserved. They do not acquire new
fallback-exception metadata retroactively.

## Completion balance

Dataset configuration may contain an assignment-balance window. The window has
no effect when `imbalance` is absent or `imbalance.enforce` is `false`. When it
is enforced, Labello compares the selected task with every other enabled task.
Disabled tasks do not participate. A dataset with fewer than two enabled tasks
has no balance peer, so the policy cannot block its only enabled task.

Each task has a separate count. Labello does not aggregate counts by class when
multiple tasks share one class.

The count depends on the requested assignment kind:

| Assignment kind | Counted image-task states |
| --- | --- |
| Annotation | `Submitted` or `Completed` |
| Review | `Completed` |

An imported image-task pair with `excluded` coverage is outside the completion
denominator until an explicit include action adds it back. All other coverage
states participate. A missing task state contributes zero.

The selected task is blocked when `selectedCount - minimumPeerCount` is
strictly greater than `maxDifference`. Counts below the peer minimum are never
blocked. A difference equal to `maxDifference` is allowed. A zero window
therefore allows tied peers and blocks a task as soon as its count is above the
least-completed peer. Ratio-based assignment balance is not supported.

Foreground claims use current completed-work counts. This preserves progress
at a zero window: tied tasks can still claim one current item. Existing work is
not rejected solely because balance subsequently changes.

Prefetch additionally projects the selected task's count after completing its
live outstanding assignments and the candidate. Each image-task pair contributes
once; already counted work and expired leases contribute no additional count.
Peer counts include only actual progress, never unfinished peer assignments.
A projected gap equal to the window is permitted; a larger gap blocks prefetch
before image data is fetched. For A=12, B=10 and a window of five, one active A
and one prepared A leave room for one more A. Foreground and prefetch claims share a process-local dataset admission guard.

Availability refreshes report which of the caller's reservations still fit the
window, ordered by original claim time and assignment ID. Clients release
affected prepared entries, preserve current work, and retry when capacity opens.
Configuration changes and peer corrections can invalidate earlier preparations;
reconciliation follows the existing refresh interval, normally 30 seconds, and
local mutations trigger refreshes. This is not a promise that concurrent
foreground work can never move actual counts outside the window.

An optional `workflowQueue.maxPendingOverviews` limits distinct images whose
Objects work has begun and still needs Overview. It reserves capacity for active
objects on new images and counts each image once. Further objects on an already
started image remain eligible, so a crowded image cannot deadlock its own Overview.
The default is no cap. Objects and Overview share task-level image balance;
individual object completion never advances the completed-image count.

## Runtime consistency

Availability and direct claims retain the current-count task-level balance
check; prepared claims add the prospective check above. Event publication updates
actual counts and outstanding reservations together in the completion projection and
invalidates cached availability and statistics. Dataset configuration changes
invalidate availability and statistics while retaining the count projection,
because enabling tasks or changing the window does not change historical counts.

The Statistics view reports the same annotation and review counts and the task
sets currently blocked by the enforced window. Its explanation names the count,
denominator, peer, zero-count, and exact-boundary rules above.

Assignment leases, per-image eligibility, review workflow rules, and exact
assignment ownership still apply after the dataset-level balance check. See the
[HTTP API contract](api.md#assignment-and-image-routes) for routes and access.

## Return completed work from the inspector

Any dataset member may inspect indexed images without claiming an assignment.
Reviewers and dataset administrators may explicitly select completed workflows
and return them to review with a required reason. This action is independent of
Previous assignment and does not revise or supersede old decisions.

Only enabled approval workflows without a live assignment are eligible. The
server checks the exact image sequence and all selected workflows together under
the image lock, after acquiring the configuration read guard. A conflict changes
nothing. Successful return creates a new submission round, clears the selected
task outcomes, preserves geometry and audit history, and invalidates completion,
availability and statistics projections through the existing transaction.

Normal review claims can then select this work, including for the previous
reviewer. Every current object and the full-image target require fresh decisions.
Migration work retains its current confirmation and must pass the existing
migration terminal-state checks. Tasks without approval review and unfinished,
disabled or actively assigned work cannot be returned through this action.

## Migration companions

An active manual-migration annotation assignment authorizes a compound
skeleton/box mutation only for that migration's configured guide task and class.
Dataset configuration and role publication is serialized with migration commands,
including add, edit, delete, explicit reconciliation and administrator repair.
A command holds a configuration read guard from metadata capture through commit,
then the same image lock guards both objects. A competing active guide-task
assignment rejects the mutation; it is not implicitly cancelled or stolen.
The box task enters `NeedsCorrection`, clears its previous final outcome and
retains prior review records as history. Normal box-task claims, corrections,
review policy and completion projections then apply.

The frozen imported migration target count does not grow when a companion is
created. Migration review defaults to canonical dispositions, discovered skeletons
in stable annotation-ID order, and then full-image confirmation. Exact current
items may be approved in any order, including after an earlier locally retained
correction. Overview requires current object receipts, which may belong to different
reviewers. Each discovery
decision binds the current exact skeleton version. A box review cannot approve
its skeleton, and migration approval cannot approve its box.

## Direct canonical revisit

This describes legacy image assignments. Queue assignments revisit a canonical
object through Previous and validate its exact item lease.

Direct canonical revisit records a `ManualSelection` dependency for a valid guide,
including a previously annotated or excluded target. That selected target remains
the durable cursor while another target acquires a dependency. Its successful
save/exclusion clears only its own marker; the next cursor resolves remaining
pending/dependent work, then full-image confirmation. Guide/disposition versions
and assignment ownership are revalidated before mutation. A changed selected guide
replaces its selection with the applicable correction dependency and stale writes
are rejected. Existing global correction-pass records retain their audit history. The latest
pass for the current assignment owns outstanding decisions and the completion
gate; earlier passes do not reopen when later edits change an annotation.

## Historical missing-object evidence

Active review no longer creates location markers. A missing object is corrected
by creating its annotation. Existing marker events, revision records and evidence
remain replayable, available in snapshots/offline data, and visible as read-only
history. Historical records never become annotations implicitly.
