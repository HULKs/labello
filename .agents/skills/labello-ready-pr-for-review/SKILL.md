---
name: labello-ready-pr-for-review
description: "Manage a Labello draft pull request from Awaiting CI through the CI-gated Ready for review handoff. Use after `$labello-open-draft-pr` returns an exact draft head, especially within `$labello-implement-issue`. Keep implementation fixes with the caller; this skill owns hosted CI state, draft promotion, assignments, reviewer requests, and review-status metadata."
---

# Labello Ready PR for Review

Move a verified draft pull request to human review only after the required hosted gate passes on its exact current head. Treat `Testing` as the aggregate result of the entire applicable pull-request CI pipeline, not as one test job.

## Establish the lifecycle state

Require the draft pull-request URL or number, expected base and head branches, exact head SHA, accountable human owner, any linked issue, and the issue's recorded pre-work project status when cancellation recovery applies. Also require authorization for assignments, reviewer requests, draft promotion, and applicable Labello project metadata. Invocation from `$labello-implement-issue` carries its task-scoped authorization; a direct request to use this skill authorizes these lifecycle actions for the named pull request.

1. Read back the pull request and confirm it is still draft, has the expected branches, and points to the supplied head SHA. Return `STALE HEAD` without mutating state if it moved.
2. Identify the accountable owner only from explicit task or coordination context. Never infer ownership from Git credentials, commit authorship, or the issue reporter.
3. Record the issue and pull request's current assignments, project membership, iteration, and project status before changing them. Reuse existing project items and verify every write through read-back.
4. For an issue-backed implementation, confirm the issue remains `In progress`. When authorized, add the pull request to the Labello project and current iteration and keep both items `In progress` while CI or CI fixes are pending.

If the user cancels before the review handoff, leave the pull request draft, restore project metadata changed by this lifecycle from the recorded snapshot, and return the linked issue to its recorded open, unstarted status when that context was supplied. Report any state that could not be restored. Closing the pull request requires separate authorization.

## Gate on exact-head CI

Monitor the required `Testing` check for the supplied head SHA. Progress updates may say `AWAITING CI`, but do not end the implementation handoff while the check is merely pending.

- A success for an earlier SHA is stale and does not pass the gate.
- A pending check keeps the pull request draft and both project items `In progress`.
- A missing, cancelled, or inaccessible check is not green. Retry only when a new run is expected; otherwise return `BLOCKED` with the exact state.
- A failed check keeps the pull request draft. Inspect the selected failing jobs and retain safe, redacted evidence. Return `CI FIX REQUIRED` with the pull-request URL, exact head SHA, failing checks, useful failure details, and run links. Leave code, tests, and documentation unchanged so the implementation caller can fix them.
- If the head changes while waiting, return `STALE HEAD`. The caller must supply the new prepared and published head before CI evaluation resumes.

Do not modify product code to accommodate an infrastructure failure. Return `BLOCKED` when credentials, hosted infrastructure, permissions, or another external condition prevents a trustworthy result.

## Complete the human handoff

After `Testing` succeeds, read back the exact head once more. A missing owner, missing project authority required by the repository workflow, or lack of an eligible independent reviewer blocks the handoff and leaves the pull request draft.

1. Assign the linked issue and pull request to the accountable owner.
2. Determine reviewer eligibility. Request the owner only when GitHub permits it and the owner is independent of the authored change. If the owner authored the pull request, keep them as assignee and request a distinct eligible reviewer.
3. Mark the pull request ready for review and request the selected reviewer.
4. Move the linked issue and pull request project items to `In review` at this same boundary. Never move either item to `Done`.
5. Update the pull-request handoff checklist to record exact-head CI, assignment, reviewer request, and Ready for review. Leave independent review unchecked.
6. Read back the pull request, assignments, requested reviewer, issue and pull-request project statuses, and exact head SHA.

If a write fails after another succeeds, repair the existing items instead of creating duplicates. Do not report success until the remote state is coherent; report the exact partial state when it cannot be repaired.

Return `READY FOR REVIEW` only when the exact current head passed `Testing`, the pull request is no longer draft, ownership and independent reviewer requirements are satisfied, and every applicable project item is `In review`. Include the pull-request link, exact head SHA, CI run, assignees, requested reviewer, and read-back project statuses. This is a human review handoff, not independent acceptance.
