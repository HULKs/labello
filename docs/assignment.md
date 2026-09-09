# Assignment

> **Status:** Normative current reference
> **Owner:** Workflow maintainers
> **Audience:** Operators, data administrators, and maintainers
> **Last verified:** 2026-08-25 for issue #11 implementation

Labello assigns work per enabled task. Availability and prepared queue results
are advisory; the storage claim transaction repeats the same eligibility checks
before creating an assignment.

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
created. Migration review visits canonical dispositions, discovered skeletons
in stable annotation-ID order, and then full-image confirmation. Each discovery
decision binds the current exact skeleton version. A box review cannot approve
its skeleton, and migration approval cannot approve its box.


## Previous review and decision revisions

Review offers Previous and the configured Previous image shortcut, Arrow Left by
default, for the immediately previous skipped or completed review in the same
dataset and task. Background cleanup of an expired reservation does not count
as a skipped review and does not replace the immediately previous review.
A shared process-local index tracks terminal review history per reviewer and task.
It is initialized with a bounded parallel scan before the first review claim and
maintained from committed image state. Warm Previous checks read the index and
the target image only; they do not load every other image's state or event log.
The index tracks the latest finished review even when that review later becomes
ineligible, so it cannot authorize falling back to an older review. Equal terminal
timestamps retain the strict newer-than comparison. Per-image event sequences
order observations, not events on different images.

The client clears that reference when the dataset, task,
account, or endpoint changes. It is not a history browser. The server validates
the exact previous assignment and creates a new assignment ID and lease. A
retry of the same opening returns that fresh active assignment. As in normal
review, the reviewer may also be the original annotation submitter. Reopening
still requires the Reviewer role and ownership of the exact previous review;
it does not grant permission to revise another reviewer's decisions.

A skipped normal review resumes its original submission round and preserves
valid object decisions. A completed review opens an exclusive revision.
Opening or cancelling that revision leaves the previous effective decisions,
outcome, and completion counts unchanged. The reviewer stages object decisions
locally, then explicitly commits the full-image decision. Approval requires all
captured targets to be approved. Rejection requires a substantive correction submission.

Commit atomically supersedes the reviewer's captured decisions, appends their
replacements, recomputes the task outcome and counts, and completes the fresh
assignment. Historical reviews remain in the event log. Each reviewer counts
at most once in the current submission round. An identical commit retry returns
the recorded result without adding reviews; a different retry is a conflict.

The original submission identity, task configuration, annotation versions,
migration dispositions, dependencies, and confirmation must remain current.
Later assignment attempts invalidate reopening even if their lease has expired.
Later relevant events are ordered by event sequence, including when timestamps
are equal. Another active lease also prevents reopening. The revision lease
excludes competing task mutations and is checked again at commit. Configuration
publication is serialized with revision validation and commit.

Revision reviewers can edit, create, or remove annotations and correct guided
migration dispositions using the same correction transaction as normal review.
A correction ends the revision and creates a new submission round. Its rejection
belongs to the old round; old approvals cannot finalize the corrected work.
Migration revisions require a current valid confirmation; historical rejection
that invalidated confirmation still needs the normal migration correction flow.
Historical assignments created before captured review contexts were introduced
remain replayable but cannot be reopened through Previous.

Switching from changed review work uses a confirmation that preserves its
correction or staged decisions when cancelled. Untouched reviews switch directly.
The previous assignment is validated and loaded before releasing current work;
a failed opening leaves the current workspace intact. Skipping or leaving a revision discards
its local staged decisions only after confirmation; server decisions remain
unchanged. Staged decisions are not persisted for browser reload recovery.

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

The comparison uses current completed-work counts. It does not reserve future
capacity for active assignments. Concurrent workers can therefore finish work
after a policy boundary was observed; later availability and claim checks use
the resulting counts.

## Runtime consistency

Availability, direct claims, and prepared-queue claims call the same task-level
balance check. Event publication updates the completion projection and
invalidates cached availability and statistics. Dataset configuration changes
invalidate availability and statistics while retaining the count projection,
because enabling tasks or changing the window does not change historical counts.

The Statistics view reports the same annotation and review counts and the task
sets currently blocked by the enforced window. Its explanation names the count,
denominator, peer, zero-count, and exact-boundary rules above.

Assignment leases, per-image eligibility, review workflow rules, and exact
assignment ownership still apply after the dataset-level balance check. See the
[HTTP API contract](api.md#assignment-and-image-routes) for routes and access.

## Direct Canonical Revisit

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

## Reviewer correction submissions

Both ordinary and guided migration reviews accumulate unsaved corrections.
A submission must edit geometry, add an annotation, remove an erroneous object,
or change a canonical migration disposition. Empty changes, unchanged geometry,
comment-only changes, unchanged exclusion reasons, bare rejection, and new
missing-object markers cannot reject work. The legacy `allowReviewerCorrections`
configuration field remains readable but no longer gates approval review.

The review UI edits the focused item directly. Approve is available for an
unchanged item; Reject retains a valid correction locally and advances. Earlier
corrections do not disable approval of another unchanged item. Reset restores the
item and requires a new decision. A valid retained correction satisfies that item's
rejection requirement when navigating to the overview; unchanged items still need
explicit approval. Navigation alone does not approve items.
The final overview permits adding missing annotations and revisiting existing
items. Once every original item has a decision, it submits approval if there are
no corrections, or submits the complete correction batch with rejection. Invalid
or unfinished additions block submission. No corrected-item rejection reaches the
server before this overview submission.

The transaction holds the configuration guard and image lock, reloads state,
checks the exact reviewer lease, captured round, task definition and target
fingerprint, validates the complete correction batch, then publishes all changes,
reviewer attribution, rejection, assignment completion and fresh submission in
one atomic event-log replacement. Canonical skeletons retain their guide/group
identities. Discovered skeletons retain the derived-box pairing rules above.

Corrected work remains `Submitted` with no completion outcome. Other review
leases are cancelled, and the same reviewer can claim a fresh assignment.
One reviewer must approve every current object and the final image in the new
round. Previous approvals remain historical evidence and cannot count toward it.

The correction ID and complete request identify an exact retry. Changed retries,
stale versions or targets, lost ownership, and changed configuration fail without
appending a partial correction. Cancelling navigation preserves staged edits;
failed requests retain the immutable submission for retry. Browser recovery is
best effort and does not replace the server event log.

## Historical missing-object evidence

Active review no longer creates location markers. A missing object is corrected
by creating its annotation. Existing marker events, revision records and evidence
remain replayable, available in snapshots/offline data, and visible as read-only
history. Historical records never become annotations implicitly.

## Single-reviewer completion

Approval tasks admit one active reviewer per image and task. That reviewer
performs object-level decisions and the final full-image check. Final approval
atomically records the decision, marks the task `Completed` with its approved
outcome, completes the owned assignment, and cancels any outstanding competing
review leases. Rejection requires substantive corrections and returns work to a fresh
`Submitted` review round.
A task configured with review workflow `none` completes on annotation submission.

Task statistics use five mutually exclusive states: Pending, In progress,
Awaiting review, Needs correction, and Completed. Approved and reviewer-corrected
completed tasks both count as Completed. Enabled-task eligibility and excluded
import coverage retain the completion-denominator rules above. Review decisions
remain in audit history and contributor activity.
