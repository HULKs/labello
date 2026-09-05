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
The same image lock guards both objects. A competing active guide-task
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
dataset and task. The client clears that reference when the dataset, task,
account, or endpoint changes. It is not a history browser. The server validates
the exact previous assignment and creates a new assignment ID and lease. A
retry of the same opening returns that fresh active assignment.

A skipped normal review resumes its original submission round and preserves
valid object decisions. A completed review opens a decision-only revision.
Opening or cancelling that revision leaves the previous effective decisions,
outcome, and completion counts unchanged. The reviewer stages object decisions
locally, then explicitly commits the full-image decision. Approval requires all
captured targets to be approved. A staged rejection prevents approval.

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

A reviewer's original geometry correction remains authoritative. Its immediate
previous revision uses the corrected current geometry and cannot create, undo,
or edit geometry. Skipped normal reviews retain ordinary correction support.
Migration revisions require the original valid confirmation. A rejected
migration whose rejection invalidated confirmation cannot be reopened as a
decision-only revision; it needs the normal migration correction workflow.
Historical assignments created before captured review contexts were introduced
remain replayable but cannot be reopened through Previous.

Switching from current review work uses a confirmation that preserves its
correction or staged decisions when cancelled. The previous assignment is
validated before releasing current work. Skipping or leaving a revision discards
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
