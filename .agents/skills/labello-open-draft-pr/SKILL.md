---
name: labello-open-draft-pr
description: "Publish an already prepared and verified Labello change as an evidence-backed draft pull request. Use after `$labello-implement-issue` or `$labello-package-current-changes-as-pr` supplies an exact publication scope and verification record, or when the user explicitly supplies an equivalent handoff. Do not use to decide or repair implementation scope, manage CI, request review, or change issue and project status."
---

# Labello Open Draft PR

Turn a prepared local change into a verified remote draft. Own the Git publication mechanics and pull-request evidence; rely on the caller for scope, correctness, and verification decisions.

## Require a prepared change

Proceed only when the invoking request or caller authorizes the task-scoped commit, push, and draft pull request and supplies:

- repository and worktree;
- comparison base and the base SHA used for verification;
- a branch named `<type>/<description>`;
- intended behavior and pull-request summary;
- exact included paths and every checkout change to preserve;
- verification commands, results, and material limitations;
- applicable manual, visual, browser, local-link, and anchor evidence;
- documentation impact, omitted checks, and residual risks;
- an issue reference when the caller has one.

An equivalent direct user handoff is acceptable. Missing scope, verification, or authorization returns `PREPARATION REQUIRED` without changing Git or GitHub state.

## Revalidate the handoff

1. Read the repository instructions governing commits, generated files, runtime data, and sensitive content.
2. Confirm the repository, worktree, comparison base, verified base SHA, branch, complete diff, branch-only commits, and staged, unstaged, and untracked inventory still match the handoff. If the base or prepared change moved after verification, return `PREPARATION STALE` so the caller can verify again.
3. Confirm every included path belongs to the stated purpose and every excluded checkout change will remain untouched. Block credentials, secrets, runtime data, generated distributions, and unexplained paths.
4. Inspect the remote branch and pull-request state. Stop on ambiguous divergence or an existing ready pull request. Never force-push or convert a ready pull request back to draft without separate authorization.

## Publish the draft

1. Stage only approved pending paths. Commit them with a message describing the prepared change; do not create an empty commit when all approved work is already committed.
2. Push the current `<type>/<description>` branch without disturbing unrelated local or remote work.
3. Create an evidence-backed draft pull request, or update the existing draft for the branch. Use the supplied issue reference when present and never invent one.
4. Fill the repository pull-request template from the handoff. Record behavior, production ownership, acceptance evidence, regression protection, exact verification results, manual evidence, documentation impact, risks, omitted checks, and preserved worktree changes. Check `Awaiting CI`; leave CI success, ownership, reviewer, Ready for review, and independent-review items unchecked.
5. Read back the pull request. Confirm its URL and number, draft state, base and head branches, and exact head SHA. Confirm the head SHA equals the pushed local head.

Return `AWAITING CI` with the pull-request URL and number, exact head SHA, branch, comparison base and verified base SHA, committed files, verification summary, omitted checks, residual risks, and remaining worktree changes.

Stop after the verified draft exists. Leave CI, draft promotion, assignments, reviewer requests, and project metadata to `$labello-ready-pr-for-review` when that workflow is requested.
