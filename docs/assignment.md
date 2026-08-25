# Assignment

> **Status:** Normative current reference
> **Owner:** Workflow maintainers
> **Audience:** Operators, data administrators, and maintainers
> **Last verified:** 2026-08-25 for issue #11 implementation

Labello assigns work per enabled task. Availability and prepared queue results
are advisory; the storage claim transaction repeats the same eligibility checks
before creating an assignment.

## Completion balance

Dataset configuration may contain an assignment-balance policy. The policy has
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

Labello supports two policies:

- `ratio`: the selected task is blocked when its count divided by the
  least-completed enabled peer is strictly greater than `maxRatio`. If the
  least-completed peer is zero, any positive selected count is blocked. Two
  zero-count peers remain eligible. A ratio equal to `maxRatio` is allowed.
- `absoluteWindow`: the selected task is blocked when
  `selectedCount - minimumPeerCount` is strictly greater than `maxDifference`.
  Counts below the peer minimum are never blocked. A difference equal to
  `maxDifference` is allowed. A zero window therefore allows tied peers and
  blocks a task as soon as its count is above the least-completed peer.

The comparison uses current completed-work counts. It does not reserve future
capacity for active assignments. Concurrent workers can therefore finish work
after a policy boundary was observed; later availability and claim checks use
the resulting counts.

## Runtime consistency

Availability, direct claims, and prepared-queue claims call the same task-level
balance check. Event publication updates the completion projection and
invalidates cached availability and statistics. Dataset configuration changes
invalidate availability and statistics while retaining the count projection,
because enabling tasks or changing policy does not change historical counts.

The Statistics view reports the same annotation and review counts and the task
sets currently blocked by the enforced policy. Its explanation names the count,
denominator, peer, zero-count, and exact-boundary rules above.

Assignment leases, per-image eligibility, review workflow rules, and exact
assignment ownership still apply after the dataset-level balance check. See the
[HTTP API contract](api.md#assignment-and-image-routes) for routes and access.
