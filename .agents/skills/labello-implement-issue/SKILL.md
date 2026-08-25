---
name: labello-implement-issue
description: "Implement an existing Labello GitHub issue end to end, including issue status, production-path analysis, the narrowest complete change, canonical verification, draft publication, CI-failure fixes, and the gated review handoff. Use when the user asks to work on, implement, fix, or complete a specific already-filed HULKs/labello issue. Do not use to draft or independently verify an issue."
---

# Labello Implement Issue

Implement the issue against the real product path and leave a reviewer enough evidence to challenge every completion claim. Treat implementation and independent acceptance as separate jobs.

## Establish the contract

1. Read the repository instructions and the normative documents relevant to the affected subsystem.
2. Load the exact issue, including material comments, linked work, and current metadata. Confirm its identity and whether it is still actionable. Record its current project status so cancellation can restore it.
3. Identify the accountable human owner of the agent run only from explicit task or coordination context. Do not infer ownership from Git credentials, commit authorship, or the issue reporter. If ownership is unavailable, record that the handoff cannot assign an owner rather than guessing.
4. Inspect the worktree layout and appropriate comparison base. Use `git worktree list --porcelain` to identify the main worktree. If the current worktree is the main worktree, create a linked worktree under `<main-worktree>/.worktrees/` and continue the issue there. If the current worktree is already non-main, keep using it.
5. Before editing, ensure the active worktree uses a branch named `<type>/<description>`. Choose a type that matches the work and a short lowercase kebab-case description, for example `fix/selector-handling`. Preserve unrelated changes and identify which existing changes are part of the issue.
6. Trace the complete production flow before editing: shared owner, target-specific adapters, callers, state transitions, persistence or transport boundaries, tests, and current documentation. Treat code and tests as the authority for current behavior.
7. Convert every requirement into an acceptance matrix with:
   - the observable behavior;
   - the production owner and all affected runtime surfaces;
   - the evidence needed to prove it;
   - relevant negative, boundary, and failure cases.
8. Reproduce the defect or establish a concrete fail-before case. If a product choice missing from the issue would materially change the solution, stop and ask for that choice.

## Keep development status current

Treat issue project-status transitions and the delegated task-scoped pull-request workflow as part of an implementation request. Keep other project metadata changes within the user's authorization. Verify every status write through read-back.

- Before implementation starts, ensure the issue is in the Labello project and set its status to `In progress`. `Backlog` and `Ready` describe work that has not started.
- If the user cancels or the implementation is abandoned before the PR lifecycle starts, return the issue to the recorded open, unstarted status, normally `Backlog` or `Ready`. If the lifecycle has started, pass the cancellation and recorded status to `$labello-ready-pr-for-review` so it restores the remote metadata it changed. Keep any pull request draft and report the read-back state and remaining cleanup.
- Keep the issue `In progress` while code changes, draft publication, CI, or CI fixes are underway. `$labello-ready-pr-for-review` owns PR project state and the final cross-item transition to `In review`.
- Move the issue and pull request to `In review` only through the successful exact-head CI and reviewer handoff owned by `$labello-ready-pr-for-review`.
- Do not move either item to `Done` until independent review passes and the user authorizes the relevant merge, acceptance, or closure action.
- If one metadata update fails after another succeeds, repair the existing project items instead of creating duplicates. Report any state that could not be made coherent.

## Implement the complete production change

- Fix the root cause at the narrowest shared owner. Do not satisfy a product issue by changing only a demo, fixture, preset, inspector, or test expectation.
- Cover every affected runtime surface. For coordinated API changes, update the client facade and DTOs, API, UI or demo callers, documentation, and focused tests as applicable.
- Preserve Labello's domain, persistence, authorization, import, redaction, and UI ownership invariants.
- Add the smallest regression test that would fail for the original defect. Record how the fail-before condition was established; do not rewrite or reset the user's worktree merely to demonstrate it.
- Update normative current documentation in the same change when behavior or a contract changes. Advance a `Last verified` marker only after checking the complete affected flow.
- Keep generated and runtime paths out of the change unless the issue explicitly requires them. Maintain an exact task-related publication scope for `$labello-open-draft-pr` and preserve every unrelated worktree change.

### Product UI changes

Treat the native MCP inspector as a deterministic view of shared egui behavior, not as proof of the browser product.

- Locate the shared production rendering owner and confirm both the inspector and WASM/browser paths reach it. Inspector-only code is an acceptable fix only when the issue itself concerns the inspector.
- Add or update an `egui_kittest` case for deterministic behavior, geometry, and AccessKit semantics when useful.
- Inspect the relevant states visually for clipping, overlap, padding, alignment, crowdedness, hierarchy, wrapping, truncation, action reachability, disabled/loading/error behavior, keyboard operation, and accessibility names.
- Exercise the issue's relevant viewport sizes, device-pixel ratio or zoom, and long-content cases. Use Chromium for claims about real WASM startup, browser layout, networking, cookies, IndexedDB, or browser input.
- Never infer browser correctness from a native inspector screenshot or a passing geometry assertion alone.

## Verify before handoff

Run focused checks first and broaden them in proportion to risk. Before any remote review handoff, run `./scripts/verify.sh changed <comparison-base>` and follow the repository verification matrix for domain/event replay, storage atomicity and recovery, API authorization and redaction, import publication and recovery, UI behavior, WASM, and documentation-only changes.

For every command, retain the exact command, result, and material limitation. Review the complete focused diff, `git status`, documentation impact, and any lockfile changes. Do not silently describe skipped, unavailable, flaky, or failing checks as passing.

## Publish, fix CI, and hand off

Do not call a change **Ready for review** merely because local checks pass. Use the shared PR skills for their owned remote-state transitions; keep implementation decisions and all CI-driven code changes here.

1. After local verification, assemble the prepared-change input required by `$labello-open-draft-pr`. Include the issue reference, intended behavior, exact scope, comparison base and verified base SHA, acceptance evidence, regression protection, commands and results, manual artifacts, documentation impact, omitted checks, residual risks, preserved worktree changes, and publication authorization.
2. Continue with `$labello-open-draft-pr`. Require its `AWAITING CI` result, draft pull-request link, and exact pushed head SHA before starting the review lifecycle.
3. Continue with `$labello-ready-pr-for-review`, passing the draft result, accountable owner, linked issue, recorded issue status, and authorized project metadata. That skill owns hosted CI observation, PR draft state, assignments, reviewer requests, and the gated `In review` transition.
4. When it returns `CI FIX REQUIRED`, inspect the failed job evidence and determine whether the failure was introduced by the change, exposes an incomplete implementation, or comes from hosted infrastructure.
5. For an implementation failure, fix the root cause within the issue contract. Update production code, regression coverage, and normative documentation as needed. Rerun focused checks and `./scripts/verify.sh changed <comparison-base>`, then refresh the acceptance evidence.
6. Continue with `$labello-open-draft-pr` again so it commits and pushes the correction and updates the existing draft. Pass the new exact head to `$labello-ready-pr-for-review` and repeat the gate.

Continue the loop while each iteration makes scoped progress. Stop and report `CHANGES REQUIRED` or `BLOCKED` when the failure needs a product choice, material scope expansion, unavailable credentials or infrastructure, or no defensible in-scope fix remains. Keep the pull request draft and both project items `In progress`. Never change product behavior merely to mask an infrastructure failure.

End the loop only when `$labello-ready-pr-for-review` returns `READY FOR REVIEW`. A stale CI success, pending check, or local verification result cannot satisfy this boundary.

After successful exact-head CI and the human review handoff, end with `READY FOR REVIEW`, not “complete,” “accepted,” or “verified.” Provide:

- issue and intended behavior;
- production ownership and runtime surfaces reached;
- each acceptance criterion mapped to concrete evidence;
- regression protection and why it would fail before the fix;
- commands and results;
- UI states, viewports, and artifacts inspected when relevant;
- documentation updated;
- checks not run, residual risks, and blockers;
- focused changed files and unrelated worktree changes left untouched;
- ready pull-request link and exact head SHA;
- issue and pull-request project statuses at each development and review boundary.

Recommend a fresh `$labello-verify-issue` pass. Do not close the issue, move it to Done, merge, or claim independent acceptance unless the user explicitly requests the relevant action and the verification verdict supports it.
