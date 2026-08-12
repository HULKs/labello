---
name: labello-implement-issue
description: "Implement an existing Labello GitHub issue end to end, from production-path analysis and reproduction through the narrowest complete code or documentation change, risk-proportional verification, and an evidence-backed Ready for review handoff. Use when the user asks to work on, implement, fix, or complete a specific already-filed HULKs/labello issue. Do not use to draft or file issues, or to independently verify another implementation."
---

# Labello Implement Issue

Implement the issue against the real product path and leave a reviewer enough evidence to challenge every completion claim. Treat implementation and independent acceptance as separate jobs.

## Establish the contract

1. Read the repository instructions and the normative documents relevant to the affected subsystem.
2. Load the exact issue, including material comments, linked work, and current metadata. Confirm its identity and whether it is still actionable. Do not edit issue or project metadata unless the user asks.
3. Inspect the branch, worktree, and appropriate comparison base. Preserve unrelated changes and identify which existing changes are part of the issue.
4. Trace the complete production flow before editing: shared owner, target-specific adapters, callers, state transitions, persistence or transport boundaries, tests, and current documentation. Treat code and tests as the authority for current behavior.
5. Convert every requirement into an acceptance matrix with:
   - the observable behavior;
   - the production owner and all affected runtime surfaces;
   - the evidence needed to prove it;
   - relevant negative, boundary, and failure cases.
6. Reproduce the defect or establish a concrete fail-before case. If a product choice missing from the issue would materially change the solution, stop and ask for that choice.

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

Run focused checks first and broaden them in proportion to risk. Follow the repository verification matrix for domain/event replay, storage atomicity and recovery, API authorization and redaction, import publication and recovery, UI behavior, WASM, and documentation-only changes.

For every command, retain the exact command, result, and material limitation. Review the complete focused diff, `git status`, documentation impact, and any lockfile changes. Do not silently describe skipped, unavailable, flaky, or failing checks as passing.

## Hand off for independent review

End with `READY FOR REVIEW`, not “complete,” “accepted,” or “verified.” Provide:

- issue and intended behavior;
- production ownership and runtime surfaces reached;
- each acceptance criterion mapped to concrete evidence;
- regression protection and why it would fail before the fix;
- commands and results;
- UI states, viewports, and artifacts inspected when relevant;
- documentation updated;
- checks not run, residual risks, and blockers;
- focused changed files and unrelated worktree changes left untouched.

Recommend a fresh `$labello-verify-issue` pass. Do not close the issue, move it to Done, merge, or claim independent acceptance unless the user explicitly requests the relevant action and the verification verdict supports it.
