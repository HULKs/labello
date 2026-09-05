# Release and deployment

> **Status:** Normative current reference
> **Owner:** Release and operations maintainers
> **Audience:** Maintainers and operators
> **Last verified:** 2026-08-20 in the issue #63 implementation worktree

Labello publishes stable x86-64 Linux releases through GitHub Actions and
deploys them from a runner inside the production Debian 12 LXC. Release builds
and production deployment use different runners. The deployment runner does
not build code, accept SSH submissions, install packages, or run the deploy job
inside a container.

## Trust and ownership

The release workflow verifies the exact `main` commit, builds the server and
browser payloads in a digest-pinned Debian Bookworm container, attests every release asset,
publishes an immutable stable release, then sends an explicit
`repository_dispatch` event. Drafts, prereleases, mutable releases, and tags
that do not resolve to the dispatched commit cannot enter deployment.

The deployment workflow runs on the production guest as the `hulk` account.
It downloads a pinned GitHub CLI into `RUNNER_TEMP`, verifies the CLI checksum,
checks release immutability and provenance, then streams the verified
deployment bundle to the local Rust transaction manager:

```sh
gzip --decompress --stdout "$RUNNER_TEMP/download/labello-deployment-$VERSION.tar.gz" |
    /var/lib/labello/bin/labello-deploy receive "$REQUEST_ID"
```

`labello-deploy` owns every application, configuration, backup, data-restore,
and admission mutation. The workflow polls only its bounded JSON status. A
systemd user service runs the transaction, so runner loss does not terminate
it.

Routine deployment is rootless. The `hulk` account owns `/var/lib/labello`
and controls only its systemd user services. It must not retain sudo or Docker
access after guest provisioning. Root performs the one-time setup needed to
create the deployment tree, enable user lingering, install host packages, and
allow unprivileged binding to ports 80 and 443.

This simple account model has a sharp trust boundary: the deployment runner
can read the production configuration and data because it is also the service
account. A restricted organization runner group should therefore allow only
`HULKs/labello/.github/workflows/deploy.yml@refs/heads/main`. The current
repository-scoped runner cannot join such a group. Until an organization owner
re-registers it, its private label, the protected `production` environment,
the `repository_dispatch` trigger, and the `main` ref check provide weaker
defense. Never allow pull-request CI to select this runner.

## Required GitHub configuration

Enable immutable releases before the first stable publication. GitHub applies
that setting only to future releases. The release workflow publishes a draft
with all assets and then makes it stable; it refuses to dispatch deployment
unless GitHub reports the result as immutable.

Create these repository environments:

- `release` protects stable publication. The release job uses Linux x86-64
  runners from the HULKs organization `Default` group.
- `production` protects the deploy job and should require an operator
  approval until the deployment has completed its rollout period.

Set these repository variables without recording their values in tracked
files or issue text:

| Variable | Contract |
| --- | --- |
| `LABELLO_RELEASE_BUILD_IMAGE` | Verified Rust 1.98.0 Bookworm build image pinned with `@sha256:<64 hexadecimal digits>` |
| `LABELLO_DEPLOY_RUNNER` | Private label for the runner inside the production guest |

Runner names, custom labels and groups, hostnames, addresses, and private
registry locations are operational data. Do not put their values in Git,
release assets, workflow output, issue comments, or screenshots. `Default` is
GitHub's standard organization runner group, not private infrastructure data.

## Release compiler and image verification

Build `deployment/release/Containerfile` from the repository root. It pins the
Rust 1.98.0 Bookworm image by digest and includes rustfmt, Clippy, the
`wasm32-unknown-unknown` target, Python 3.11, and Trunk 0.21.14. The canonical
policy audit compares its compiler tag with `rust-toolchain.toml`. The retained
`deployment/guest/github-runner-base/Containerfile` is historical upstream
reproduction input and is not a Labello build entry point.

Before changing `LABELLO_RELEASE_BUILD_IMAGE`, build and publish the replacement
image using the authorized registry workflow, then test the exact published
digest in an isolated checkout. Run `python3 scripts/rust-toolchain.py check`,
`./scripts/verify.sh all`, and the locked release server/deployment-tool build
inside that image. Verify `trunk --version` reports 0.21.14. Record the tested
source commit, image digest, compiler and tool versions, commands, and results
in the private rollout record. Keep registry locations and runner details out
of public evidence. An unavailable or older image is an unmet rollout check.

The release workflow validates a complete SHA-256 image reference and checks
the effective compiler against the repository policy before verification or
payload compilation. It does not upgrade an outdated image during release.
Changing the source Containerfile alone does not update the external image or
the repository variable; both publication and verified configuration are
required before rollout. Future baseline changes follow the coordinated
[compatibility contract](verification.md#rust-compatibility-contract).

## Release assets

The manual release input must be canonical stable SemVer, such as `v1.2.3`.
The release job refuses leading zeroes, suffixes, an existing tag, an existing
release, a dirty checkout, or a commit other than the fetched `main` head.

Each release contains:

- `labello-server-x86_64-linux-<version>.tar.gz`;
- `labello-browser-<version>.tar.gz`;
- `labello-deployment-<version>.tar.gz`, containing the exact tree accepted by
  `labello-deploy`;
- `release-metadata-<version>.json`, inventorying the payload names, hashes,
  tag, and source commit; and
- `SHA256SUMS`, inventorying every other asset and excluding itself.

The browser payload contains `MANIFEST.sha256`, which inventories every other
browser file and excludes itself. It also contains `release.json` with the
release tag and source commit. The deployment bundle contains a separate
`release-manifest.json` for every extracted server and browser file. The Rust
manager rejects missing, additional, duplicate, non-regular, non-UTF-8, unsafe,
or hash-mismatched files and confirms that the browser inventory names the
same release.

The release workflow generates GitHub build-provenance attestations for every
asset. Deployment requires the `HULKs/labello` repository, the release workflow
path, `refs/heads/main`, and the exact dispatched source digest. The deploy job
does not read the checksum or metadata files until their attestations have
passed. The read-only Rust `verify-release` command then rejects unexpected or
duplicate checksum paths and requires the metadata payload names and hashes to
match that checksum inventory exactly.

## Guest layout

The stable root is mount-friendly:

```text
/var/lib/labello/
  bin/labello-deploy
  data/                         complete datasetsRoot; may become a mount
  config/                       operator-edited source configuration
  configurations/
    <release>/                  immutable configuration generation
    current -> <release>
  releases/
    <release>/
      server/labello-server
      browser/
    current -> <release>
  backups/<request>/
  requests/<request>/journal.json
  state/deploy.lock
  caddy/
    live/Caddyfile
    maintenance/Caddyfile
    current -> live|maintenance
```

`data/` includes `.labello-server/auth.json`, private import state, every
dataset, images, events, caches, keybindings, and committed provenance. A
later mount must use this exact location and preserve the `hulk` UID/GID,
ordinary permissions, atomic rename, file and directory sync, and available
space required by import.

Operators edit only `config/`. Each transaction copies and verifies it as
`configurations/<release>` before candidate data access. The release and
configuration links always move together, so executable rollback uses the
matching configuration. Published release and configuration generations are
never edited in place.

The complete Debian 12 LXC source, pinned HULKs runner resources, package list,
Labello release-image layer, rootless hardening, mount preparation, and
user-service installation procedure are in
[`../deployment/guest/README.md`](../deployment/guest/README.md).

## Transaction contract

`receive` validates a bounded request ID and a bounded POSIX tar stream, writes
the candidate below a new request directory, verifies the complete candidate
inventory, flushes the initial journal, and starts
`labello-deploy@<request>.service`. It never replaces an existing request.

The worker takes an exclusive host lock and advances one flushed journal:

1. Switch Caddy to maintenance and reload it.
2. Stop Labello through its graceful systemd user-service shutdown and wait for
   the process to exit.
3. Inventory the stopped source, copy the complete data root, including
   dot-directories and empty directories, to a request backup, then require the
   source and copy to match the pre-copy inventory.
4. Record file hashes, entry types, and permission modes, then sync the backup
   tree.
5. Publish the no-replace release and configuration generations.
6. Flush `candidate_data_access_started` before switching generations or
   starting the candidate server.
7. Restart the server and poll the loopback deployment-readiness route for the
   exact tag and source commit.
8. Flush `admission_started` before switching Caddy to the live configuration.
9. Reload Caddy and mark the transaction complete.

The journal phases exposed by `status` contain only the request ID, release
identity, barriers, phase, and a bounded failure category. They never contain
paths, command output, request bodies, credentials, dataset contents, or probe
data.

### Failure and recovery

After the maintenance transition begins and before candidate data access, a
failure keeps the data untouched, selects the previous release and
configuration when one exists, and remains in maintenance mode. A failed first
installation has its own `first_install_failed` terminal state. Boot recovery
leaves Caddy live only for a receipt-only request whose worker never began that
transition.

After candidate data access and before admission, the manager stops the
candidate, verifies the backup again, clears only the managed data directory,
restores every verified entry, selects the previous executable and
configuration, and restarts it. A first installation restores the original
data but has no previous executable to start.

At `admission_started` and later, automatic data restoration is forbidden. A
failure enters `manual_recovery`. Preserve the journal, backup, failed release,
configuration generation, and logs. Keep Caddy in maintenance, determine
whether writes reached admitted traffic, and follow the full-root restore rules
in [Operations](operations.md#backup-and-restore). Do not edit the journal or
point an older binary at migrated data.

At guest boot, `labello-deploy-recover.service` examines every nonterminal
journal before the Labello server starts. An uncertain pre-admission candidate
is restored from its verified backup. A post-admission journal becomes
`manual_recovery` and blocks the server. Boot recovery never resumes a new
candidate automatically. A request that never advanced beyond durable receipt
has not changed a symlink or service; recovery records its previous release and
leaves that live deployment untouched.

## Readiness and admission

`GET /deployment/readiness` is public because the local transaction manager
cannot use a browser session. The API normally binds to loopback, and Caddy is
the only network path. The response contains only:

- service and compiled release identity;
- current persistence schema version; and
- `ok` or `failed` for dataset-root traversal and authentication-store load.

The probe does not create, rename, or write any data. It does not prove free
space, write capacity, a representative dataset read, import filesystem
features, OAuth reachability, or browser networking. The transaction separately
verifies the browser inventory and the operator must monitor capacity.

## Caddy and browser runtime configuration

The tracked user service runs Caddy as `hulk`. The one-time guest sysctl allows
unprivileged processes to bind ports 80 and 443. Caddy terminates TLS, serves
the current browser generation, and strips `/api/` before proxying to the
loopback API. It serves the untracked `labello.client.json` from the current
configuration generation with `Cache-Control: no-store`. A missing static file
returns 404; there is no SPA fallback.

The maintenance configuration returns 503 for every network request. The
transaction manager calls only the loopback API while maintenance is active.

## Outbound access

The deployment guest needs outbound HTTPS access to GitHub release, API, and
artifact hosts plus the Sigstore trust and transparency endpoints used by
`gh attestation verify`. Caddy also needs DNS and outbound access to the
configured certificate authority when it manages public TLS certificates. The
guest does not need inbound SSH for automation. Operator SSH access is outside
the release path and should follow the normal restricted administration
policy.

## Rollout checklist

Before the first production release:

1. Complete the guest setup and remove `hulk` from `sudo` and `docker`.
2. Enable immutable releases and create the two protected environments.
3. Set the three repository variables without exposing their values.
4. Confirm the deployment runner cannot accept pull-request CI.
5. Verify the data mount, ownership, capacity for a full backup, TLS, OAuth,
   and exact browser origin.
6. Start a release with canonical `vX.Y.Z` input.
7. Record the exact release workflow SHA, source commit, deploy run, request
   ID, terminal phase, readiness result, and backup verification result.
8. Perform a restore drill before treating rollback as operationally proven.

The implementation remains `In progress` until exact-head CI and independent
review pass. A local transaction test does not prove the real guest's mounts,
systemd user manager, Caddy TLS, GitHub environment rules, or runner-group
restriction.
