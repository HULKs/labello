---
name: labello-package-current-changes-as-pr
description: "Package existing changes in the current Labello checkout as a draft pull request by clarifying their purpose, verifying their behavior, reviewing the diff, and checking cleanliness before delegating publication. Use when the work already exists and the user wants it checked and published without implementing or handling an issue. Do not use to repair the change, manage CI, or request human review."
---

# Labello Package Current Changes as PR

Turn the current checkout's existing work into a checked, reviewable change and then hand it to `$labello-open-draft-pr`. Treat the present diff and commits as the implementation scope, not as permission to add or repair behavior.

A request to use this skill authorizes the delegated task-scoped commit, push, and draft pull-request actions after the preparation gates pass. It does not authorize issue or project changes, implementation fixes, or a human review handoff.

## Audit the current checkout

1. Read the repository instructions and the normative documents relevant to the changed paths.
2. Inspect the current branch, worktree, staged and unstaged changes, untracked files, commits relative to the appropriate comparison base, and complete diff. Use the current checkout; do not create a linked worktree.
3. Infer the change's purpose, expected behavior, and intended scope from the diff, tests, documentation, and user context. State that interpretation before verification. If several plausible purposes remain or the intended outcome cannot be checked, ask focused clarifying questions and wait before changing Git or GitHub state.
4. Define the exact publication scope. Account for every staged, unstaged, untracked, and branch-only change. Exclude unrelated work, generated or runtime paths, credentials, and secrets. If intended and unrelated changes cannot be separated safely, ask which files belong in the pull request.
5. Keep GitHub issues, project fields, and iterations untouched.
6. Ensure the checkout uses a branch named `<type>/<description>`. Choose a matching type and a short lowercase kebab-case description, for example `docs/clarify-import-limits`. If the checkout is on `main`, detached, or on a nonconforming unpublished branch, create or rename the branch without moving the changes. Stop if an existing remote branch or pull request makes the safe operation ambiguous.

## Verify the existing change

- Trace changed code through its production callers and affected runtime paths. Check that the implementation matches the stated purpose and that the evidence covers the behavior most likely to regress.
- Exercise the changed behavior where practical. Run focused checks, then `./scripts/verify.sh changed <comparison-base>`. Complete the applicable manual, visual, browser, local-link, and anchor checks from the repository verification contract.
- Treat a failed, unavailable, or inconclusive required check as a blocker. Report the exact command and result. Leave implementation and tests unchanged.

## Review once

After verification, inspect the complete diff and evidence for correctness, regressions, missing tests or documentation, unsafe input handling, redaction or authorization mistakes, debug code, dead code, and changes that do not reach the production path.

Judge whether the proposed pull request is clean enough to review. Warn when it mixes purposes, includes unrelated or generated churn, hides behavior changes in formatting noise, contains unexplained files or commits, is too large to review confidently, or leaves checkout changes unaccounted for.

- Stop on a correctness, safety, or required-verification blocker and report the finding without fixing it.
- For a messy but publishable change, explain the concrete problems and ask whether to narrow or split it, publish the draft as-is, or stop. Wait for that choice before publication.
- Proceed without a cleanliness warning only when the purpose is coherent, the evidence supports it, and every checkout change is accounted for.

Recheck the complete diff and `git status` after the review pass.

## Hand off for publication

Assemble a prepared-change handoff for `$labello-open-draft-pr` containing the repository and worktree, comparison base and verified base SHA, branch, intended behavior, pull-request summary, exact included paths, preserved checkout changes, verification evidence, manual artifacts, documentation impact, omitted checks, residual risks, and confirmation of publication authorization. Do not invent an issue reference.

Continue with `$labello-open-draft-pr` using that handoff. End with the publisher's `AWAITING CI` result and include the draft pull-request link and exact head SHA.

Stop there. Leave CI waiting, draft promotion, ownership, reviewer-request preservation, and project status to a separately requested workflow.
