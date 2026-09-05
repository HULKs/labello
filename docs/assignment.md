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
