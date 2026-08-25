---
name: labello-verify-issue
description: "Independently verify an implementation for an existing Labello GitHub issue by reconstructing its acceptance contract, auditing the production diff and regression coverage, running risk-proportional checks, and attempting to falsify completion claims. Use when the user asks to review, validate, QA, or decide whether work for a specific HULKs/labello issue is ready for integration. Do not use to implement the fix, draft or file an issue, or rubber-stamp the implementer's handoff."
---

# Labello Verify Issue

Decide whether the current implementation satisfies the issue in the real product, using independently gathered evidence rather than the implementer's confidence or test summary.

## Protect independence

- Read the repository instructions and relevant normative documents.
- Prefer a fresh agent context. If prior implementation context is present, reconstruct the case from the raw issue, repository, and diff, and disclose that independence is reduced.
- Use `git worktree list --porcelain` to identify the main worktree. If the current worktree is the main worktree, create a linked verification worktree under `<main-worktree>/.worktrees/` and continue there. If the current worktree is already non-main, keep using it after confirming its revision.
- Pin the verification worktree to the pull request's exact current head. Use a detached checkout when the implementation branch is already checked out in another worktree. Do not create, rename, or move the implementation branch.
- Work read-only by default. Do not fix code, change tests, edit issue metadata, commit, close, merge, or move work to Done unless the user separately asks for that action after the verdict.
- Treat the implementation handoff as a list of claims to test, not as evidence.
- Run verification commands without silently updating tracked dependencies or artifacts. Recheck the worktree after verification and preserve unrelated changes.
- Verify that the required `Testing` check succeeded for the pull request's exact current head SHA. A success for an earlier commit does not satisfy the gate.

## Reconstruct the acceptance case

1. Load the exact issue, material comments, linked work, and current metadata. At a Ready for review handoff, confirm the issue and pull request project items are both `In review`. Report any mismatch without changing it.
2. Inspect the implementation branch name, verification revision, worktree, appropriate base revision, and complete focused diff. Confirm the branch follows `<type>/<description>`, such as `fix/selector-handling`, and the verification revision matches the pull request's exact head. Report naming mismatches without renaming the branch. Separate issue changes from pre-existing or unrelated work.
3. Build an independent matrix for every explicit and implied acceptance criterion:
   - observable production behavior;
   - responsible shared owner and affected runtime surfaces;
   - positive, negative, boundary, and failure cases;
   - evidence required for a verdict.
4. Trace changed code through its callers, target-specific adapters, tests, and normative documentation. Look specifically for fixture-only, preset-only, inspector-only, test-only, or unreachable fixes, plus scope creep and missing coordinated changes.
5. Establish that the regression protection detects the original defect. Use safe base/diff reasoning or an existing reproduction; do not destroy or rewrite the user's worktree to manufacture a fail-before run.

## Attempt to falsify the implementation

Reproduce the issue at the current revision and challenge the happy path. Select checks according to the affected risk: invalid and long inputs, empty/loading/error states, retries, stale responses, concurrency, restart or interrupted recovery, authorization boundaries, redaction, version compatibility, and cross-target behavior.

Run focused tests first, then broader checks proportional to risk. A passing test suite does not by itself prove the acceptance criteria. Record exact commands, outcomes, skipped checks, environmental limitations, and whether failures appear introduced, pre-existing, or unrelated.

Treat a failed required exact-head CI check as a blocking finding and return `CHANGES REQUESTED`. Treat a pending, missing, cancelled, or inaccessible required check as missing evidence and return `NOT VERIFIED — MISSING EVIDENCE`. Do not substitute a local run for the repository's required hosted check.

### Product UI verification

The native MCP inspector proves only deterministic shared egui rendering for the state it exposes. It does not prove that the production WASM path reaches the same state or that browser-specific behavior works.

- Confirm the fix lives in, or is reached by, the shared production rendering owner. Reject inspector-only proof for a product UI issue.
- Use `egui_kittest` for deterministic behavior, geometry, and AccessKit semantics; use the native inspector for shared egui visual inspection; use Chromium for production WASM/browser claims.
- Inspect clipping, overlap, padding, alignment, crowdedness, hierarchy, wrapping, truncation, action reachability, interaction states, keyboard behavior, and accessibility names.
- Exercise the relevant viewport sizes, device-pixel ratio or zoom, long content, and loading/failure states from the issue and UI guidelines.
- Treat missing required browser or visual evidence as unverified, even when unit tests and geometry assertions pass.

## Report the verdict

Lead with findings in severity order. Give exact files and line references where possible, explain the user-visible or invariant impact, and distinguish blocking findings from follow-ups.

Return exactly one verdict:

- `VERIFIED FOR INTEGRATION` only when every acceptance criterion has adequate evidence and no blocking finding remains;
- `CHANGES REQUESTED` when the implementation is incorrect, incomplete, regressive, or violates scope or invariants;
- `NOT VERIFIED — MISSING EVIDENCE` when required checks or environments are unavailable or inconclusive.

Include:

- findings;
- an acceptance matrix marked pass, fail, or unverified;
- commands and results;
- visual states and runtime surfaces inspected when relevant;
- residual risks and evidence limitations;
- focused diff and worktree hygiene.
- exact-head required CI status and the accountable owner, assignee, and requested-reviewer identities. Flag self-review or a missing independent reviewer as blocking the acceptance handoff.

Do not soften missing evidence into a pass. A verifier verdict is an integration recommendation; it does not itself authorize closing the issue, merging, committing, or changing project status.
