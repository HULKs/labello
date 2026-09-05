# Verification And Acceptance

> **Status:** Normative current reference
> **Owner:** Labello maintainers
> **Audience:** Maintainers, contributors, and reviewers
> **Last verified:** 2026-08-19 at issue #57 CI parallelization

This contract separates implementation evidence, mechanically enforced checks,
and independent acceptance. Passing commands is necessary evidence; it does not
replace review of the intended behavior, production path, or risks.

## Canonical Entry Point

From the repository root, verify a branch and any local changes against its
comparison base with:

```sh
./scripts/verify.sh changed origin/main
```

CI uses the equivalent `./scripts/verify.sh ci <pull-request-base-sha>` command
contract. The hosted workflow first calls its `audit` and `plan` stages, then
runs the selected verification stages as separate parallel jobs. Calling `ci`
without a stage runs those same checks sequentially for local reproduction.
Both forms classify every changed path, and an unclassified path fails closed.
Documentation-only changes run the documentation profile; every other change
runs the baseline, and browser-affecting changes also run the release Trunk
build and the Chromium workflow and visual gate. Use `./scripts/verify.sh all` to run the baseline and browser build
without changed-path optimization, or `./scripts/verify.sh classify <base>` to
inspect the selected profiles.

The required baseline is:

```text
cargo fmt --all -- --check
cargo fmt --manifest-path apps/egui-mcp-inspector/Cargo.toml --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features --exclude labello-ui
cargo test --locked -p labello-ui --all-features
cargo check --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml
cargo test --locked --manifest-path apps/egui-mcp-inspector/Cargo.toml --all-features
cargo check --locked -p labello-wasm --target wasm32-unknown-unknown
```

The workspace and `labello-ui` tests are disjoint so CI can run them in
parallel. The dedicated UI command uses all features, which keeps
`inspector-presets` coverage explicit without executing those tests twice.
Browser, shared-rendering, browser-persistence, browser-import,
deployment-asset, and relevant shared dependency changes additionally run,
from `apps/labello-wasm`:

```text
trunk build --release --locked
```

The browser stage then runs these commands from the repository root:

```text
node scripts/browser/build.mjs
cargo build --locked -p labello-server
cargo build --locked -p labello-storage --example browser_fixture
cargo test --locked -p labello-storage --example browser_fixture
npm --prefix scripts/browser ci
npm --prefix scripts/browser test
scripts/browser/node_modules/.bin/playwright install chromium
node scripts/browser/run.mjs
```

The [browser verification guide](browser-verification.md) owns the executable
matrix, initial workflow coverage, fixture isolation, visual baselines,
failure injections, timeouts, and inspector comparison. This selected CI job
is part of the required `Testing` gate. Node 24.19.0, the tracked npm lockfile,
and Chromium's Linux system dependencies are additional prerequisites. Install
the system dependencies with
`./scripts/browser/node_modules/.bin/playwright install-deps chromium` after
`npm --prefix scripts/browser ci`. The release build image includes them.

### Synthetic UI verification artifacts

Only the exception in [operations](operations.md#synthetic-ui-verification-artifacts)
permits canvas evidence. The browser gate creates repository-owned artificial
patterns, geometry, and labels with `labello-browser-v1` provenance. It admits
only its own newly created fixture environment and allowlisted PNG/JSON
outputs. Tests must prove refusal of non-fixture, foreign-origin, and unknown
artifact-state inputs before CI publishes those outputs. Reports contain safe
state names, revision, display metrics, bounded failure counts, and cleanup
results. They must not include raw URLs, HTTP bodies, cookies, tokens,
profiles, storage dumps, or production/user-derived content. CI uploads the
explicit file allowlist for 14 days, never the runtime directory or a trace.
A screenshot is evidence for independent visual review, not acceptance itself.

CI preserves those test selections with pinned cargo-nextest 0.9.143 and a
separate doctest job:

```text
cargo nextest run --locked --workspace --all-features --exclude labello-ui
cargo nextest run --locked -p labello-ui --all-features
cargo test --locked --workspace --all-features --doc
```

The final Cargo command is intentionally retained because Nextest does not run
doctests on stable Rust. The local `changed`, `baseline`, and `all` commands use
ordinary `cargo test`, so contributors do not need cargo-nextest. The `ci`
command requires it and fails if the pinned prebuilt tool was not installed.

Every Cargo command that resolves dependencies and the Trunk build use locked
mode. A stale tracked lockfile is therefore a failure and is never implicitly
rewritten by verification.

Prerequisites are Rust 1.98.0, Cargo, `rustfmt`, Clippy, the
`wasm32-unknown-unknown` target, Trunk 0.21.14, and the native libraries required
by `eframe`. CI declares the applicable prerequisites per job in
`.github/workflows/ci.yml`, restores job-scoped dependency build artifacts for
the root or standalone-inspector target directory, downloads pinned prebuilt
Trunk and cargo-nextest binaries only where needed, and invokes this script
rather than duplicating verification commands. Job identity is part of each
cache key, preventing parallel jobs from racing to publish one immutable cache;
keys also include the Rust toolchain, Cargo manifests and lockfiles, and relevant
compiler environment. Caches therefore work across compatible hosted runners
without relying on machine reuse. The final `Testing` job fails closed unless
the plan and every selected parallel job succeeded, while requiring an
unselected conditional job to be skipped. The script's audit checks those
links, the PR evidence sections, shell syntax, diff whitespace, and complete
classification of tracked paths.

## Rust compatibility contract

`rust-toolchain.toml` is the compiler source for local commands and CI bootstrap.
The root workspace and standalone inspector workspace both require Rust 1.98.
Every maintained package inherits that MSRV. Commands from either directory
resolve to the exact 1.98.0 compiler, including rustfmt, Clippy, and the WASM
target. Install the pinned prerequisites with:

```sh
rustup show
```

Rustup reads the repository toolchain file and installs the declared compiler,
components, and target. Cargo manifests declare the MSRV for both workspaces.
CI uses that toolchain to build and test the supported matrix below. Release
images include the same compiler and build prerequisites. Keep these settings
aligned when changing the baseline; there is no custom toolchain-policy audit.

| Workspace or target | Required locked compatibility evidence |
| --- | --- |
| Root, native host | All maintained members including server and deployment tool; all targets/features in Clippy and all features in tests, with separate UI and doctest coverage |
| Standalone inspector, native host | Its own lockfile; formatting, all-target/all-feature Clippy, compiler check, and all-feature tests |
| Browser, `wasm32-unknown-unknown` | Production WASM app compiler check and release Trunk build with the root lockfile |
| Release, Linux x86-64 Debian Bookworm | Same baseline and browser build, then locked release server/deployment-tool builds inside the configured immutable image |

The inspector is a native application. Browser-only code is verified through
`labello-wasm`; inspector-only features do not imply browser support. Native GUI
and Chromium evidence retain the limits and risk-specific requirements below.

Dependency updates must pass this matrix with both tracked lockfiles on 1.98.0.
Do not repair lockfiles during verification. An MSRV increase requires an
explicit coordinated change to workspace policies, the toolchain pin,
CI/release configuration and images, and current documentation. Optional checks
on newer compilers do not replace this baseline. The external release image
must be verified before rollout as described in [deployment](deployment.md).

## Risk Profiles

The script selects profiles conservatively from changed paths. The machine
baseline is shared; the following checks are additional acceptance evidence.
If a change crosses profiles, apply all of them.

### UI And Shared Rendering

Read [`ui-design-guidelines.md`](ui-design-guidelines.md) and
[`ui-ownership.md`](ui-ownership.md). Add the smallest `egui_kittest` regression
covering behavior, geometry, and AccessKit semantics. Exercise long content and
loading, empty, stale, failure, disabled, keyboard, focus, and overlay states
that the change can affect. Inspect applicable shared states at the viewport,
DPR, zoom, larger-text, and keyboard matrix in the UI guidelines. Native
inspector evidence proves shared egui behavior only; use Chromium for browser
layout, zoom, input, accessibility-tree, networking, cookie, or IndexedDB
claims. Shared-rendering changes require the locked release Trunk build.

For native inspection, follow the
[inspector guide](../apps/egui-mcp-inspector/README.md#development-and-verification-loop).
It includes headless startup, MCP readiness checks, independent parallel
instances, and evidence tied to the tested checkout. Headless execution does
not remove the visual checks or replace the Chromium evidence above.

### Browser And WASM

Read [`ui-ownership.md`](ui-ownership.md),
[`ui-design-guidelines.md`](ui-design-guidelines.md), and the browser portions
of [`operations.md`](operations.md). Run the WASM compiler check and locked
release Trunk build. In Chromium, exercise startup plus the affected networking,
credentials, persistence, folder import, responsive layout, input, and failure
paths. Record browser version, viewport, DPR/zoom, accessibility inspection, and
unsupported coverage. Do not infer browser behavior from the native inspector.

### Domain, Events, And Schema

Read [`architecture.md`](architecture.md), [`persistence.md`](persistence.md),
and any affected import or UI ownership contract. Cover validation and invalid
transitions, replay at every event boundary, current and supported legacy wire
decoding, schema compatibility, digest or provenance behavior, and negative
geometry/identifier bounds. A persisted-shape change also requires interrupted
migration and historical replay evidence across every affected artifact.

### Storage, Migration, Ingestion, And Import

Read [`persistence.md`](persistence.md), [`import.md`](import.md), and
[`operations.md`](operations.md). Test the complete lock/reload/validate/
append/replay/cache-invalidate transaction, atomic publication, no-replace and
rollback behavior, cache rebuilding, restart and interruption recovery, limits,
duplicate and invalid input, and concurrent or stale-assignment races relevant
to the change. Import changes must cover parse, plan, build, verification,
publication, durable job recovery, provenance, and bounded-resource failures.

### API And Security Boundaries

Read [`api.md`](api.md), [`configuration.md`](configuration.md), and
[`operations.md`](operations.md). Cover route and role matrices, exact
assignment ownership, authentication, OAuth state and flow cookies, CSRF,
credentialed CORS, request limits, untrusted-input conversion, safe public
errors, and failure responses. Inspect logs for the redaction contract: no
credentials, raw URLs or bodies, filenames or source paths, image content,
annotation geometry, review comments, or idempotency values. Test denied and
cross-dataset cases, not only the authorized path.

### Documentation Only

Run `./scripts/verify.sh docs`. Review the changed content against current code
and tests, check every changed local link and anchor, run `git diff --check`, and
inspect the focused diff. Do not advance a normative document's `Last verified`
marker without auditing its complete affected flow. Documentation-only changes
do not require the Rust baseline unless they exercise generated contracts or
examples; documentation parity automation remains outside this profile.

Infrastructure changes never receive the documentation-only optimization. They
run the baseline, and dependency/workflow/script changes conservatively run the
browser build as well. An unknown path has no profile: if the classifier cannot
prove a surface irrelevant, verification fails instead of skipping it.

Release workflow, deploy workflow, `deployment/`, and `tools/` changes use the
infrastructure profile. The baseline runs the Rust deployment transaction
tests. The audit also rejects SSH, sudo, package installation, and Python in
the deploy workflow and requires its pinned GitHub CLI checksum, local
`verify-release`/`receive`/`status` boundary, immutable-release check,
attestation signer, and flushed barrier documentation. Real guest systemd,
mount, Caddy, TLS, and GitHub environment tests remain manual rollout evidence.

## Workflow scope

The requested task determines which lifecycle stages the agent owns. These
boundaries do not waive verification or independent acceptance:

| Request | Endpoint and owner |
| --- | --- |
| Analyze or review | Findings and evidence; no implementation or publication unless requested |
| Clean up or implement a change, then open a PR | Finish the authorized edits, verify, then publish a draft marked **Awaiting CI** |
| Package existing changes as a PR | Verify and publish the prepared scope as a draft marked **Awaiting CI**; report implementation blockers to the caller |
| Implement an existing issue end to end | Implement, verify, publish, fix CI failures, and complete the CI-gated **Ready for review** handoff |
| Ready a draft for review | Manage CI state and handoff metadata; return implementation failures to the implementation owner |
| Independently verify | Audit the requirements, final diff, and evidence; report an acceptance decision separately from implementation |

The repository skills implement these stages:
[package current changes](../.agents/skills/labello-package-current-changes-as-pr/SKILL.md),
[open a draft](../.agents/skills/labello-open-draft-pr/SKILL.md),
[implement an issue](../.agents/skills/labello-implement-issue/SKILL.md),
[ready for review](../.agents/skills/labello-ready-pr-for-review/SKILL.md), and
[independent verification](../.agents/skills/labello-verify-issue/SKILL.md).
Finish edits authorized by the user before entering the packaging stage.
Publication alone does not authorize CI management, assignments, or project
transitions. A later request can extend that scope without repeating completed
preparation when the verified revision and base remain unchanged.

For a PR without an issue, use the user's requirements as its acceptance
contract and apply issue/project updates only to existing linked items. Do not
create an issue to satisfy the handoff checklist. Merging, closing issues, and
marking work accepted require user authorization as well as the gates below.

Before coordinating multiple issues or testing dependent PRs together, follow
[parallel development](parallel-development.md) for worktree ownership and
verification of the combined group. Each PR still needs its own required CI.

## Evidence And Independent Acceptance

The pull-request template is the required proof bundle. It records:

- changed behavior or contract and the complete production ownership path;
- every acceptance criterion mapped to concrete evidence;
- regression protection and why it would fail before the fix;
- exact commands, outcomes, environment, and material limitations;
- required visual/browser states and artifacts;
- normative documentation reviewed or updated;
- unresolved risks and checks not performed; and
- preservation of unrelated worktree changes.

Artifacts and logs must follow [`operations.md`](operations.md): never upload
secrets, credentials, runtime datasets, raw request data, image bytes,
annotation geometry, review comments, uploaded filenames, or import paths.

After assembling the proof bundle and publishing a draft, the contributor
leaves the change **Awaiting CI**. The required check must succeed for the pull request's exact
current head SHA; a local run or success on an earlier head is not a substitute.
A failed check returns the change to implementation, while a pending, missing,
cancelled, or inaccessible check is recorded as not verified.

Within an authorized review handoff, only after exact-head CI success may the
change become **Ready for review**.
At that boundary, use the pull-request author as the accountable implementation
owner and assign the pull request and any linked issue to that user. Keep the
existing reviewer requests unchanged. Request review as a lifecycle transition
by moving project items to `In review` and reporting the change as **Ready for
review**. The agent does not add or remove requested reviewers.

An independent human or separately instructed verification agent then
reconstructs the contract from the original issue, audits the final production
diff and proof bundle, and tries to falsify each claim. High-risk review follows
the complete applicable transaction, failure, recovery, authorization,
compatibility, and redaction boundaries. Only after that review and all required
checks pass may an issue be accepted or closed.

## Repository Enforcement

The `CI` pull-request workflow exposes descriptive parallel jobs behind the
required `Testing` status check. Its aggregate job is the stable branch
protection contract; the worker job names may evolve without changing that
contract. The workflow has read-only repository permissions and uploads no
artifacts. Repository administrators must configure a branch protection rule
or ruleset for `main` that:

- requires pull requests and at least one approving independent review;
- requires `Testing` to pass on the current head;
- blocks direct pushes and force pushes, including administrator bypass unless
  an audited emergency procedure explicitly applies; and
- requires conversations to be resolved before merge.

The workflow file cannot activate repository-hosted branch protection by
itself. The rule must be enabled in GitHub before this contract is considered
fully enforced; record that repository-setting check in rollout evidence.
