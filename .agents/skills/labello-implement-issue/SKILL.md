---
name: labello-implement-issue
description: "Implement an existing Labello GitHub issue end to end, from production-path analysis and reproduction through the narrowest complete change, canonical verification, and a CI-gated evidence handoff. Use when the user asks to work on, implement, fix, or complete a specific already-filed HULKs/labello issue. Do not use to draft or file issues, or to independently verify another implementation."
---

# Labello Implement Issue

Implement the issue against the real product path and leave a reviewer enough evidence to challenge every completion claim. Treat implementation and independent acceptance as separate jobs.

## Establish the contract

1. Read the repository instructions and the normative documents relevant to the affected subsystem.
2. Load the exact issue, including material comments, linked work, and current metadata. Confirm its identity and whether it is still actionable. Do not edit issue or project metadata unless the user asks.
3. Identify the accountable human owner of the agent run only from explicit task or coordination context. Do not infer ownership from Git credentials, commit authorship, or the issue reporter. If ownership is unavailable, record that the handoff cannot assign an owner rather than guessing.
4. Inspect the branch, worktree, and appropriate comparison base. Preserve unrelated changes and identify which existing changes are part of the issue.
5. Trace the complete production flow before editing: shared owner, target-specific adapters, callers, state transitions, persistence or transport boundaries, tests, and current documentation. Treat code and tests as the authority for current behavior.
6. Convert every requirement into an acceptance matrix with:
   - the observable behavior;
   - the production owner and all affected runtime surfaces;
   - the evidence needed to prove it;
   - relevant negative, boundary, and failure cases.
7. Reproduce the defect or establish a concrete fail-before case. If a product choice missing from the issue would materially change the solution, stop and ask for that choice.

## Keep development status current

When the user has authorized issue, pull-request, and project metadata changes, keep project status in sync with the work. Verify every status write through a read-back call.

- Before implementation starts, ensure the issue is in the Labello project and set its status to `In progress`. `Backlog` and `Ready` describe work that has not started.
- When a pull request exists, add it to the Labello project and the current iteration when authorized. Set both the issue and pull request to `In progress`, and keep them there while code changes, CI, or CI fixes are underway.
- After the required `Testing` check succeeds on the exact current head and an eligible independent reviewer has been requested, move both items to `In review`.
- Do not move either item to `Done` until independent review passes and the user authorizes the relevant merge, acceptance, or closure action.
- If one metadata update fails after another succeeds, repair the existing project items instead of creating duplicates. Report any status that could not be updated.

## Implement the complete production change

- Fix the root cause at the narrowest shared owner. Do not satisfy a product issue by changing only a demo, fixture, preset, inspector, or test expectation.
- Cover every affected runtime surface. For coordinated API changes, update the client facade and DTOs, API, UI or demo callers, documentation, and focused tests as applicable.
- Preserve Labello's domain, persistence, authorization, import, redaction, and UI ownership invariants.
- Add the smallest regression test that would fail for the original defect. Record how the fail-before condition was established; do not rewrite or reset the user's worktree merely to demonstrate it.
- Update normative current documentation in the same change when behavior or a contract changes. Advance a `Last verified` marker only after checking the complete affected flow.
- Keep generated and runtime paths out of the change unless the issue explicitly requires them. Do not commit unless the user explicitly requests a commit.

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

## Hand off for independent review

Do not call a change **Ready for review** merely because local checks pass. When the user has authorized a pull request and its metadata:

1. Create or update the evidence-backed pull request without requesting review yet.
2. Wait for the required `Testing` check on the exact current head SHA. A stale success on an earlier head is not evidence.
3. If the check fails, investigate and fix the issue within scope, rerun local verification, push the correction, and wait for the new exact-head result. If it is pending or unavailable, report `AWAITING CI`; if it remains failing, report `CHANGES REQUIRED`. Do not assign the review handoff, request reviewers, or move project status to In review.
4. Only after the required check succeeds, assign the issue and pull request to the accountable human owner and request that owner as a reviewer when GitHub permits it and the owner is independent of the authored change. A pull-request author cannot approve their own work; in that case keep the owner as assignee and request a distinct eligible reviewer.
5. When authorized, add the pull request to the Labello project/current iteration. Move both the issue and pull request from `In progress` to `In review` only at this successful-CI handoff boundary.

Without authorization to commit, push, open a pull request, or change metadata, stop at `READY FOR PR` and state which remote gate and handoff actions remain. After successful exact-head CI and the authorized assignment/review request, end with `READY FOR REVIEW`, not “complete,” “accepted,” or “verified.” Provide:

- issue and intended behavior;
- production ownership and runtime surfaces reached;
- each acceptance criterion mapped to concrete evidence;
- regression protection and why it would fail before the fix;
- commands and results;
- UI states, viewports, and artifacts inspected when relevant;
- documentation updated;
- checks not run, residual risks, and blockers;
- focused changed files and unrelated worktree changes left untouched;
- issue and pull-request project statuses at each development and review boundary.

Recommend a fresh `$labello-verify-issue` pass. Do not close the issue, move it to Done, merge, or claim independent acceptance unless the user explicitly requests the relevant action and the verification verdict supports it.
