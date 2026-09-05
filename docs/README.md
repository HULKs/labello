# Documentation

> **Status:** Documentation index and governance reference
> **Owner:** Labello maintainers
> **Audience:** All contributors and operators
> **Last verified:** 2026-07-30 at `4f9c332`

## Current References

The code and tests are the source of truth for current behavior. These
documents are maintained current references:

| Document | Status | Owner | Audience | Purpose |
| --- | --- | --- | --- | --- |
| [HTTP API contract](api.md) | Normative current; internal/unversioned | API maintainers | Client, API, UI maintainers | Routes, roles, transport limits, errors, and compatibility policy |
| [Architecture](architecture.md) | Normative current | Labello maintainers | Maintainers, contributors | Crate boundaries and implementation ownership |
| [Assignment](assignment.md) | Normative current | Workflow maintainers | Operators, data administrators, maintainers | Assignment eligibility, completion balance, counts, boundaries, and cache consistency |
| [Server configuration](configuration.md) | Normative current | Server maintainers | Operators, maintainers | Supported settings, defaults, validation, and deployment configuration |
| [Dataset import](import.md) | Normative current | Import maintainers | Operators, maintainers, UI contributors | Supported sources, lifecycle, invariants, and ownership |
| [Dataset export](export.md) | Normative current | Storage and API maintainers | Dataset administrators, operators, contributors | Selection, capture, artifacts, and explicit round-trip guarantees |
| [Release and deployment](deployment.md) | Normative current | Release and operations maintainers | Operators, maintainers | Stable release, rootless guest transaction, rollback, and rollout contract |
| [Operations](operations.md) | Normative current | Server maintainers | Operators, maintainers | Logging, redaction, health, deployment, backup, upgrade, and recovery |
| [Persistence and recovery](persistence.md) | Normative current | Storage maintainers | Operators, maintainers | On-disk authority, schema compatibility, atomicity, snapshots, and repair |
| [UI and design guidelines](ui-design-guidelines.md) | Normative current | UI maintainers | UI designers and contributors | UI behavior, design, accessibility, and verification acceptance |
| [UI ownership](ui-ownership.md) | Normative current | UI maintainers | UI maintainers and contributors | UI state, request, persistence, and rendering boundaries |
| [Verification and acceptance](verification.md) | Normative current | Labello maintainers | Maintainers, contributors, reviewers | Canonical checks, risk profiles, evidence, CI, and independent acceptance |

Supporting project material is grouped by purpose:

- The [parallel development guide](parallel-development.md) defines the
  contributor workflow for issue tracks, stacked PRs, isolated worktrees, and
  combined group verification.
- The [native inspector guide](../apps/egui-mcp-inspector/README.md) covers
  headless setup, MCP operation, parallel-agent isolation, and native UI
  verification. Browser acceptance remains in the verification reference.
- [`plans/`](plans/) is indexed by explicit status and contains active,
  completed, and historical implementation records plus the current
  [workflow policy ownership](plans/structural-refactor-policy-ownership.md)
  inventory.
- [`history/`](history/) contains baselines and delivery records retained for
  context. Historical records are not normative for current behavior.

Planned work is tracked in [GitHub issues](https://github.com/HULKs/labello/issues)
and organized for authorized maintainers in the
[Labello project](https://github.com/orgs/HULKs/projects/12). Neither is
evidence of current product support.

[`labello.md`](../labello.md) is target product intent and can describe
unimplemented behavior. The root [`README.md`](../README.md) is the current
repository overview and setup guide.

## Status And Metadata

Documents use these statuses:

- **Normative current:** describes supported current behavior and must agree
  with code and tests.
- **Proposed:** design work that has not been accepted for implementation.
- **Active:** an accepted implementation plan with unfinished work.
- **Completed:** an implemented plan retained for its decisions and acceptance
  record.
- **Historical:** a baseline or delivery record retained only for context.
- **Target product:** desired product behavior that is not evidence of current
  implementation.

Every normative current document must name its status, role-based owner,
audience, and last verified date/revision. Add `Supersedes` when an older
document could otherwise be mistaken for current guidance. Plan status and
replacement documents live in the [plans index](plans/README.md). Historical
documents may retain the revision-specific paths and wording they recorded,
but must link to the current replacement when one exists.

## Freshness Rules

Review a normative document when a change affects its routes, configuration,
persistence, lifecycle states, logging/events, redaction, ownership boundary,
UI behavior, or operator procedure. Update its `Last verified` marker only
after checking the complete affected flow against code and tests.

At minimum, maintainers review current references before each release and after
schema or security changes. A stale verification marker is a prompt to audit,
not proof that later code still matches. Documentation-only changes require
content, local-link/anchor, and diff checks. Historical documents are exempt
from current code-path checks only when their historical status is explicit.
