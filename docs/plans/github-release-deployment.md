# GitHub release and rootless deployment plan

> **Status:** Active
> **Owner:** Release and operations maintainers
> **Audience:** Maintainers, implementers, and reviewers
> **Last verified:** 2026-08-20 in the issue #63 implementation worktree
> **Completion condition:** Exact-head CI, guest rollout evidence, and independent issue #63 review

## Decision

Release builds remain on the existing build runner. A separate runner inside
the Debian 12 production LXC executes only the deploy workflow. There is no SSH
transport between runners and no job-level deployment container.

Routine deployment is rootless under `hulk`. The transaction manager is Rust,
lives in `tools/labello-deploy`, and owns the host lock, flushed journal,
backup, release and configuration generations, service changes, readiness,
admission, rollback, and boot recovery. Python and sudo are absent from the
deployment path.

The guest source starts with the pinned resources from
`HULKs/hulk/tools/ci/github-runners` at
`6c6cec2f8ef8023987e036af783bf34544aac2cf`, then removes the general build
runner's sudo and Docker access after one-time provisioning.

## Acceptance matrix

| Observable behavior | Production owner | Evidence and failure cases |
| --- | --- | --- |
| Canonical manual stable version resolves to exact `main` | `.github/workflows/release.yml` | Reject malformed SemVer, dirty checkout, existing tag/release, or mismatched head |
| Full canonical verification precedes packaging | Release workflow and `scripts/verify.sh` | `./scripts/verify.sh all` on the resolved commit |
| Pinned x86-64 build environment and immutable assets | Release environment and workflow | Digest validation, deterministic archives, GitHub immutable flag |
| Payload, browser, and checksum inventories agree | Release workflow and Rust release validation | Attest fixed names before parsing; reject missing, extra, duplicate, unsafe, or hash-mismatched files; metadata and checksums agree; checksum file excludes itself |
| Provenance binds workflow, repository, ref, and commit | GitHub attestation action and deploy workflow | Verify every asset with exact signer workflow, `main`, and source digest |
| Publication explicitly starts deployment | Release workflow | Stable immutable release sends `repository_dispatch`; drafts and prereleases do not deploy |
| Deployment enters locally without SSH or sudo | `.github/workflows/deploy.yml` | Verified bundle is piped to rootless `labello-deploy receive`; no SSH material or remote polling exists |
| Runner loss does not kill the transaction | systemd user template | `receive` flushes the journal before starting the asynchronous worker |
| One owner owns release validation and deployment state | `tools/labello-deploy` | Workflow attests fixed assets, then calls `verify-release`, `receive`, and `status`; manager holds the host lock for mutations |
| Complete backup precedes candidate access | Rust manager | Graceful server stop; source-to-copy hash, type, permission, dot-directory, and empty-directory coverage; backup failure blocks the barrier |
| Pre-admission failure restores safely | Rust manager | Tests inject candidate start/readiness failures and verify the complete original data and previous generation |
| Admission forbids automatic restore | Rust manager | Failure after flushed admission barrier enters `manual_recovery` with candidate data intact |
| Boot handles every barrier conservatively | Rust manager and recovery unit | Receipt-only crash leaves the live release alone; pre-admission candidate uncertainty restores; post-admission uncertainty blocks startup for manual recovery |
| First install has a distinct safe state | Rust manager | No previous binary is guessed; original data is restored and maintenance stays active |
| Readiness is explicit and non-mutating | API deployment handler | Healthy, missing-root, and corrupt-auth tests; bounded output contains no probe data |
| Browser and API use one TLS gateway | Caddy and systemd user templates | Caddy serves current browser, strips `/api/`, and API binds loopback |
| Debian 12 guest can be reproduced | `deployment/guest` | Pinned upstream LXC source plus Labello package, ownership, linger, mount, and hardening steps |
| Private infrastructure stays private | Workflows and documentation | Only repository variable names and stable local paths are tracked |

## Remaining rollout work

- Finish the `production` environment approval policy and make the configured
  digest-pinned release image publicly pullable by the organization `Default`
  runner group. The deployment runner variable is configured.
- Enable immutable releases before publishing the first stable release.
- Point production DNS at the guest, update the OAuth callback to the
  same-origin API route, and only then enable the Caddy user service.
- Publish and exercise the first stable release through both workflows.
- Re-register the repository-scoped runner in a workflow-restricted
  organization group when an organization owner is available.
- Run a full backup restore drill against the intended data mount.
- Obtain exact-head CI and an independent issue #63 verification verdict.

On 2026-08-20, the Debian 12 guest was provisioned through the pre-cutover
state. The runner account had sudo and Docker access removed; the Rust
transaction manager, Caddy templates, stable configuration, and systemd user
units were installed rootlessly. A release-mode server built in the pinned
Bookworm image passed the guest readiness endpoint. User-manager restart and
full guest reboot checks restored recovery and the direct Actions runner while
cleanly skipping the absent first release. Caddy remains disabled until DNS
cutover to avoid failed ACME validation.

Current behavior and operator procedure live in
[`../deployment.md`](../deployment.md). Keep this plan as the decision and
acceptance record after completion.
