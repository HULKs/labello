# Contributing To Labello

Labello changes move through three distinct gates: contributor evidence,
mechanical verification, and independent acceptance. An implementation is
**Awaiting CI** after its evidence is assembled. It becomes **Ready for review**
only after the required check succeeds on the pull request's exact current head;
it is not accepted merely because its author or implementation agent reports
that its tests pass.

## Issue Workflow

The sequence below covers end-to-end issue implementation. For analysis,
publication of prepared changes, or a separate review handoff, use the
[workflow scope](docs/verification.md#workflow-scope) to determine the requested
endpoint. Before orchestrating multiple issues, read the
[parallel development guide](docs/parallel-development.md).

1. Confirm the issue is current, actionable, and not already owned elsewhere.
2. Read the affected normative contracts and trace the complete production
   path, including target-specific adapters and failure boundaries.
3. Convert the issue requirements into observable acceptance criteria and
   establish a fail-before reproduction.
4. Implement the narrowest complete production change with regression
   protection and coordinated documentation updates.
5. Run the canonical changed-path verification from the repository root:

   ```sh
   ./scripts/verify.sh changed origin/main
   ```

6. Complete the applicable risk profile in
   [the verification contract](docs/verification.md), including manual,
   visual, browser, recovery, authorization, compatibility, and redaction
   checks that cannot be inferred from compilation or unit tests.
7. Fill in the pull-request template with exact commands and results, an
   acceptance-criterion-to-evidence map, omitted checks, and unrelated-change
   confirmation. Open or update the pull request as **Awaiting CI**.
8. Wait for the required check to succeed on the exact current head. A failure
   returns the change to implementation; a pending, cancelled, unavailable, or
   stale check does not satisfy the gate.
9. After that success, use the pull-request author as the accountable
   implementation owner and assign both the issue and pull request to that
   user. Preserve existing reviewer requests. Request review as a lifecycle
   transition by reporting the change as **Ready for review** and moving its
   project item to `In review`. The agent does not add or remove requested
   reviewers.
10. Have a human reviewer or separately instructed verification agent inspect
    the original issue, final diff, and evidence and try to disprove the
    completion claims. The implementer must not make the independent acceptance
    decision.
11. Integrate only through a pull request after the required `Testing` check
   and independent review pass. Repository
   administrators must protect `main` against direct pushes and require this
   status check before merge.

Never close an issue or mark a change accepted from implementation
self-assessment alone. A failing or unavailable required check is recorded as
not verified; it is never silently treated as passing.

## Verification Prerequisites

Install the repository-pinned Rust 1.98.0 toolchain, the
`wasm32-unknown-unknown` target, and Trunk 0.21.14. Browser checks also require
Node 24.19.0 and the pinned Chromium prerequisites in the
[browser verification guide](docs/browser-verification.md). Native Linux builds also require the Wayland, X11,
XKBCommon, and OpenGL development libraries used by `eframe`. The canonical
script uses tracked lockfiles with Cargo and Trunk locked mode, so verification
fails instead of rewriting dependency resolution.

See [the verification contract](docs/verification.md) for the baseline,
changed-path classification, risk profiles, CI equivalence, branch settings,
and evidence requirements.

## UI acceptance evidence

Map every applicable UI acceptance criterion to `egui_kittest`, native inspector,
Chromium, or an explicitly recorded manual review. State the production owner,
regression protection, exact commands and results, viewport/DPR/zoom, keyboard
and accessibility findings, artifact links, and omitted checks. Include
reproducible before/after browser evidence for visual fixes and an adjacent
breakpoint. Inspect paint, spacing, wrapping, focus, loading, and disabled
states; passing containment assertions alone do not prove visual quality.

Inspector presets are synthetic shared-renderer fixtures. An inspector-only
change cannot fix a product UI issue unless that issue concerns inspection
infrastructure. Use the [browser verification guide](docs/browser-verification.md)
for the executable matrix, allowed synthetic artifacts, and the current
annotation/migration comparison. Browser widget accessibility and platform
font scaling limitations must remain explicit in handoffs.
