# Labello documentation

Start with [getting started](getting-started.md), then choose the guide for your
work. These pages describe the implemented application. Code and tests resolve
any disagreement with the prose.

## Use Labello

| Guide | Contents |
| --- | --- |
| [Getting started](getting-started.md) | Local setup, first dataset, sign-in, connection troubleshooting |
| [Annotation and review](annotation.md) | Boxes, skeletons, controls, corrections, navigation, drafts |
| [Dataset administration](administration.md) | Schema reuse, roles, image ingestion, inspection, statistics, snapshots |
| [Dataset import](import.md) | Supported YOLO/COCO profiles, source upload, planning, coverage, publication |
| [Guided migration](migration.md) | Box-to-skeleton work, exclusions, discoveries, companion reconciliation |
| [Dataset export](export.md) | Selection, completeness, artifacts, preservation and round trips |
| [Assignment](assignment.md) | Eligibility, leases, completion, balance, previous review, correction transactions |
| [Contribution scoring](scoring.md) | Rewards, deductions, daily tiers, focus workflows, leaderboard meaning |
| [Current limitations](limitations.md) | Partial features, unsupported behavior, operating constraints |

## Operate a server

| Guide | Contents |
| --- | --- |
| [Configuration](configuration.md) | Server and browser settings, authentication, environment, resource limits |
| [Release and deployment](deployment.md) | Release verification, guest transaction, readiness, rollback |
| [Guest setup](../deployment/guest/README.md) | Debian LXC provisioning, permissions, user services, Caddy |
| [Operations](operations.md) | Logging, health, capacity, backup, restore, upgrades, incident handling |
| [Persistence and recovery](persistence.md) | Artifact authority, event transactions, schema compatibility, recovery |
| [Event history](event-history.md) | Workflow event replay, historical compatibility, server-owned commands |

## Develop Labello

| Guide | Contents |
| --- | --- |
| [Contributing](../CONTRIBUTING.md) | Setup and change/review workflow |
| [Architecture](architecture.md) | Crates, dependency direction, shared interfaces |
| [Workflow policy](workflow-policy.md) | Domain validation, API authorization, storage transaction boundaries |
| [HTTP API](api.md) | Routes, access, wire types, limits, responses |
| [UI design](ui-design-guidelines.md) | Layout, interaction, accessibility, viewport acceptance |
| [UI implementation](ui-ownership.md) | State, async requests, rendering, browser recovery |
| [Native inspector](../apps/egui-mcp-inspector/README.md) | Headless setup, presets, live inspection, evidence |
| [Verification](verification.md) | Canonical checks, risk profiles, CI, review gates |
| [Parallel development](parallel-development.md) | Worktrees, dependent branches, combined verification |
| [Documentation and wiki](wiki.md) | Editing, checking links, publication, archive boundary |

Update the relevant current page when behavior changes. Keep each detailed
contract in one place and link to it from user guides. Documents have no owner,
status, date, or revision headers; Git records their history.

Plans, proposals, target requirements, and delivery records belong in the
repository-only [archive](archive/README.md). They are preserved for context and
excluded from the wiki. Planned work is tracked in
[GitHub issues](https://github.com/HULKs/labello/issues).
