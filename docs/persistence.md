# Persistence and recovery

This document defines the current on-disk authority, compatibility, atomicity,
and recovery contract. The code and storage/domain tests remain the executable
source of truth. See [Operations](operations.md) for backup, restore, upgrade,
and incident procedures. [Event history](event-history.md) details current
workflow events, exact retries, and historical replay compatibility.

## Root layout

```text
<datasetsRoot>/
  .labello-server/
    auth.json
    imports/
    exports/<job-id>/
    prelabels/<dataset-id>/
  <dataset-id>/
    labello.dataset.toml
    labello.schema.json
    images-index.json
    images/
    annotations/<image-id>/
      events.jsonl
      state.json
    users/<user-id>/
      keybindings.toml
    .labello/
      scoring/focus-v1.json
      imports/<import-id>/
        manifest.json
        source-objects.jsonl
      migrations/schema-v2-to-v3/
        journal.json
        generations/
      snapshots/<snapshot-id>/
        manifest.json
        ...
```

Import job workspaces and API control records below
`.labello-server/imports` have additional internal files. Their layout is
storage-private and must not be consumed directly by API, UI, operator scripts,
or external integrations.

## Artifact authority

| Artifact | Classification | Recovery rule |
| --- | --- | --- |
| `.labello-server/auth.json` | Authoritative secret authentication/session state | Restore only from the matching full-root backup; never log, publish, or merge it |
| `.labello-server/imports/` | Authoritative private in-progress import, reservation, upload, and API control state | Let startup recovery reconcile it; do not delete apparently stale workspaces manually |
| `.labello-server/exports/` | Private derived captures, job state, and verified archives | Startup interrupts unpublished jobs and preserves completed artifacts until expiry; never edit job records or publish partial files |
| `.labello-server/prelabels/` | Private durable hint generations, pause/run state and signing keys; derived indexed result files | Preserve in full backups; use authenticated reset/retry commands and automatic interrupted-run recovery; omitted from snapshots |
| `labello.dataset.toml` | Authoritative dataset configuration, workflow definition, and role state | Valid supported schema required; restore rather than hand-edit damaged data |
| `images-index.json` | Authoritative image identity, hash, path, and metadata index | Valid supported schema required; image-directory contents alone do not reproduce stable identities |
| `images/` | Authoritative image bytes addressed by the image index | Include in full backups; omitted from Labello snapshots |
| `annotations/<image-id>/events.jsonl` | Authoritative append-only audit and workflow history | Replay in sequence; never truncate, reorder, merge, or edit by hand |
| `annotations/<image-id>/state.json` | Derived, rebuildable cache | Rebuilt automatically when absent, stale by event sequence, on a supported older schema, or with an older review projection generation |
| `labello.schema.json` | Generated schema bundle | Regenerated during supported artifact migration and before publishing companion-link or captured review-assignment events or accepted prelabel origins; do not treat it as annotation authority |
| `users/<user-id>/keybindings.toml` | Authoritative keyboard and pan-drag user shortcuts, not workflow state | Back up separately from Labello snapshots; normalize missing current bindings through storage |
| `.labello/imports/<import-id>/manifest.json` | Authoritative committed import provenance | Must match the dataset and directory import ID |
| `.labello/imports/<import-id>/source-objects.jsonl` | Authoritative committed source-object audit record | Preserve with its manifest and event history |
| `.labello/migrations/...` | Durable migration journal and staged generation | Recovery state until migration completion; do not remove during an interrupted migration |
| `.labello/snapshots/` | Derived point-in-time annotation/audit packages | Downloadable but not directly restorable; retain according to operator policy |
| Statistics and process caches | Derived | Recompute or invalidate after authoritative writes |
| `.labello/scoring/focus-v1.json` | Authoritative versioned 20-minute focus selections | Preserve in full backups and snapshots; score totals rebuild from events plus this file. Never discard as a cache |

Browser IndexedDB/local-storage drafts and availability caches are recoverable
client conveniences. They are outside the server root and never authoritative
workflow state.

## Write and transaction boundaries

IDs and relative paths are validated before filesystem access. Dataset
repository JSON/TOML replacement writes use a temporary file, file sync, atomic
rename, and directory sync where required by the operation. Event mutation
transactions:

```text
load and authorize
  -> acquire the per-image process-local lock
  -> reload exact current state
  -> validate and simulate the full event batch
  -> atomically replace events.jsonl with the appended sequence
  -> replay/update state.json
  -> invalidate derived caches
```

The event append is the authoritative commit. If cache publication fails or a
process stops after that boundary, later state loading replays the event log.
No transaction spans multiple server processes, multiple dataset roots, or an
external backup tool.

Migration transactions complete artifact migration, acquire the repository
configuration read guard, load and authorize current configuration, then acquire
the image lock. They retain both guards through event publication and cache
updates. Dataset configuration publication takes the matching write guard, so
a guide/task/role change either precedes command validation or waits for its
commit. This configuration-before-image order also covers explicit companion
reconciliation and administrator migration repair. The guard is process-local;
external configuration-file edits do not participate in this serialization.

Image-index publication is serialized with replacement of the shared parsed
cache. Dataset-root mutation locking coordinates normal dataset creation and
import publication only inside one process. Running multiple server processes
against one root is unsupported even when the backing filesystem is shared.

Import builds a complete dataset below the same root, verifies event replay and
sealed output, and publishes with one atomic no-replace directory rename.
There is no partial merge into an existing dataset.

## Schema compatibility

The current persisted schema is version 3; version 2 is the supported legacy
schema. Current artifacts must carry `schemaVersion`. Unknown older or newer
versions are rejected rather than guessed.

Version 2 event entries are decoded through explicit wire types and upcast for
current replay. Dataset configuration, image indexes, generated schema,
keybindings, and state caches migrate through
`.labello/migrations/schema-v2-to-v3`. The journal records a prepared
generation and each publication phase so a later access can resume after
interruption. State caches are rebuilt from event history during migration.

Current dataset configuration writes assignment balance directly as
`imbalance.maxDifference`. Ratio configuration cannot be converted to an
equivalent absolute window without an operator choice. Before upgrading,
replace `imbalance.maxRatio` in version-2 or version-3 dataset files with an
explicit `maxDifference`; loading rejects ratio and tagged-policy shapes. This
configuration change does not alter historical event bytes.

An upgrade is one-way unless a release explicitly documents reverse
compatibility. Preserve a full pre-upgrade backup; rollback means restoring
that backup, not changing `schemaVersion` fields manually.

## Recovery behavior

### State cache

Loading an image compares `state.json` with the image ID, supported schema, and
last event sequence. The current review projection generation must also match,
so a same-sequence cache from before review-round tracking is rebuilt. Missing or stale state is replayed from `events.jsonl` and
written back when appropriate. A malformed authoritative event prevents replay
and requires backup restore or maintainer-led forensic repair.

### Artifact migration

The repository validates an existing migration journal, verifies staged-file
hashes, resumes the next incomplete publication phase, rebuilds state caches,
and records completion. A completed journal is retained as evidence. Unknown
files must not be substituted into its generation.

### Import

Startup import recovery can:

- recognize a destination published before the job reached `succeeded`;
- verify and publish a sealed `committing` output;
- rewind interrupted preflight/build/verification to
  `awaiting_decision` when durable artifacts are valid, otherwise `sealed`;
- expire abandoned non-protected work; and
- release reservations not owned by active jobs.

See [Dataset Import](import.md#import-workflow) for the full lifecycle.
Configured terminal-job retention cleanup is not currently scheduled by the
production server.

### Snapshots

Snapshot creation reads the dataset configuration and image index, copies the
generated schema when present, includes committed import manifests and
source-object records, copies authoritative event logs, and rebuilds each
included `state.json` from those events. The completed snapshot directory is
published by rename.

Snapshots omit:

- image bytes;
- `.labello-server/auth.json` and all session/authentication state;
- user keybindings; and
- private import/export job and control state.

There is no native snapshot restore. Use the full-root procedure in
[Backup And Restore](operations.md#backup-and-restore).

Snapshots include recorded scoring focus history when present. Contribution
scores themselves remain derived. See [Contribution scoring](scoring.md) for
the first-activation, atomic-publication, historical-credit, and recovery rules.

## Repair rules

- Stop the server and preserve the complete root before investigating.
- Determine authority from the table above; do not infer it from file size or
  modification time.
- Rebuild only documented derived artifacts. A `state.json` cache can be
  removed or rebuilt by repository behavior only after the matching event log
  is known valid.
- Never repair authority by copying a nearby dataset's IDs, event entries,
  authentication file, import records, or migration journal.
- Never edit persisted schema numbers to bypass validation.
- Restore authoritative corruption from one consistent backup generation.
- Keep incident logs redacted according to [Operations](operations.md#redaction).

## Contract verification

The current contract is exercised by:

- `crates/labello-domain/src/v2_contract_tests.rs` and
  `v3_import_tests.rs` for wire compatibility and upcasting;
- `crates/labello-storage/src/repository/tests.rs` for event authority,
  stale/missing state rebuild, interrupted artifact migration, snapshot
  contents, and committed import records;
- `crates/labello-storage/src/import/tests.rs` for publication and startup
  recovery; and
- API tests for authorization and safe access to snapshots and import state.

Persistence-format changes must update this reference and [event history](event-history.md) and the smallest fixture
or test that would fail if the stated compatibility or recovery rule regressed.

## Derived preview cache

Encoded previews, including the 256-pixel Thumbnail v1 gallery proxies, are
disposable derived artifacts outside `datasetsRoot` in the
production server. They are not dataset images, image-index entries, import
outputs, events, export/snapshot contents, or authoritative backup contents.
The embedded `ApiState` default uses its private `.labello-server/previews`
subdirectory for tests/in-process composition; production overrides that default
with the configured separate cache root.

A cache key includes dataset repository identity, image ID, original BLAKE3,
source decoder format, fixed versioned profile, resize/orientation/color/alpha
policy, and a build-time digest of the dependency lockfile (covering encoder and
decoder versions). Uploaded names or arbitrary requested dimensions cannot name
cache files. Every read opens the indexed original beneath the dataset root,
rejects symlinks/traversal, and verifies its hash. Removed or changed originals
therefore cannot yield stale cached pixels, even before index reconciliation.
Unreachable entries are evicted under the finite quota.

Each private entry contains a bounded header and checksummed WebP payload.
Publication writes and syncs a private temporary file, renames it, then syncs the
directory. Corrupt or missing entries regenerate. A filesystem lock excludes
another live cache owner. Restart removes recognized interrupted temporary
files, validates the bounded directory inventory, and enforces quotas. Eviction
uses least-recently-read order in a process and oldest publication time after
restart. Unrecognized files are preserved and fail cache initialization.
Cancelling a caller before work starts publishes nothing; after start its bounded
worker retains permits and completes or cleans up atomic publication.

### Browser image previews

Working-image loads always use Data Saver v1. The application no longer reads or
writes the old `:data-saver` localStorage preference under an endpoint/account
`StorageIdentity` prefix. Existing values are harmless and ignored; neither a
saved `false` nor an invalid value can select a larger preview. Workspace and
draft persistence formats are unchanged. Signing out or changing endpoint clears
image references and rejects/cancels obsolete transfers. Derived previews do not
authorize offline work or restore a server assignment.

## Completion and preload projection

The completion projection also tracks live assignments for prospective preload
admission. Actual counts remain separate from reservations. Both derive from
the same replayed image state and are observed together after event publication,
before state-cache publication. Interrupted or failed event publication invalidates
the projection; restart rebuilds it from events. Warm balance checks read in-memory
counts and live reservations without rescanning image states per item. Expiry is filtered
against server time on every read.

`preloadQueueSize` is a defaultable dataset configuration field in supported
version-2 and version-3 representations. Missing values mean two; current storage
reads and writes validate the supported range. Snapshots retain the configured
value. This change adds no assignment or workflow event fields.

## Previous-review history index

Each repository and its clones share an in-memory index derived from per-image
assignment history. It stores the latest completed or intentionally cancelled
review per image, reviewer and task, plus event sequences and aggregate latest
candidates. Cancellation at or after lease expiry is maintenance and does not
advance this history. The index is not an authority or a persisted artifact.

Initialization reads replay-validated states with at most 32 concurrent workers.
Review claims warm the index before returning an assignment. Concurrent committed
observations supersede older scan results by image event sequence. A membership
generation prevents publishing a scan against an obsolete image index. Restart,
explicit state repair, and image membership changes require rebuilding; the first
review operation can therefore incur initialization cost.

Ordinary event transactions and offline synchronization observe committed history
synchronously after event publication and before derived state-cache publication.
A failure or interruption during a history-changing publication invalidates the
index because the event log may already have been renamed. Failed state-cache
publication after observation does not lose the committed history.

Lock order is configuration guards where applicable, image lock, history
membership read guard, then sorted reviewer/task guards. No history guard holder
acquires another image lock or initializes the index. Membership publication and
explicit repair take the history membership write guard. Reopening checks the
latest candidate under the same reviewer/task guard used by terminal review
publication and retains it through event publication and index observation.

## Export capture and recovery

Export captures replay exact per-image event cuts under the existing image
locks and copies original bytes after releasing those locks. Source event
logs, caches, and configuration are not modified. Fresh configuration and
index digests, root identity, and original-image hashes are checked before
artifact publication. Captured event sequences remain fixed if later source
events are appended.

Private mode-0700 job directories contain `job.json`, staged payloads, and
an unpublished archive. A verified archive is linked without replacement as
`dataset.zip`, synced, and then recorded as succeeded. The blocking worker
holds the shared configuration and image-index read guards from its final
source verification through the no-replace link and directory sync. It does
not wait for the job map between verification and publication. Cancellation
before durable success removes even an already linked archive. Only succeeded jobs
are downloadable; each download verifies size and BLAKE3. A crash before the
durable succeeded status makes the job interrupted on restart and removes
its payload. Completed archives survive restart until retention expiry.
Orphan reservations and expired entries are cleaned before retained capacity
is checked. See [export](export.md) for the artifact contract.

## Daily activity projection

Current-user daily activity replays authoritative per-image event history.
The domain projection counts one submission or final review per image/task/user
within a supplied UTC day. It includes `TaskStateChanged` submission and
no-review annotation completion, final `ReviewRecorded` records, and final
records in `ReviewRevisionCommitted`. It uses event commit timestamps and keeps
historical work counted after later task or review changes.

The storage owner caches one day and one statistics generation per repository,
with all users sharing the same coalesced scan. The scan has at most 32 image
workers, validates replay, and never derives activity from `state.json` or
current task status alone. Existing commit/index/config cache invalidation also
invalidates daily activity; restart recomputes it from events. No new persisted
artifact or schema is introduced. A malformed log fails the request rather than
silently omitting work. Atomic event-log replacement prevents partially appended
reads. A commit concurrent with a scan makes its generation stale for the next
request; the result is a bounded scan, not a global instantaneous snapshot.

## Lease-based presence

Server presence derives from replayed assignment state for indexed images.
Only `Active` annotation and review assignments with an expiry strictly later
than the server time count. Legacy assignments without `expiresAt` use the
existing `updatedAt` plus 30-minute lease duration. Multiple leases for a user
in one dataset reduce to their latest expiry. No presence artifact is persisted.

Repository clones share a presence cache and a single refresh lock. A cold scan
loads at most 32 image states concurrently. Existing assignment-availability
commit, configuration, index and rebuild invalidation also invalidates this
projection. Expiry is filtered on every cache read, independent of writes, and
restart reconstructs the projection. A concurrent invalidation prevents a scan
from publishing its cached result. Presence is a sampled view, not a transaction
snapshot across datasets, and reading it never renews leases.

## Inspector filter metadata

The repository shares a process-local cache of per-image workflow statuses and
active annotation class IDs. This is a disposable query projection, not an
authoritative artifact. Initial access loads the event-validated image state;
queries and writers share the per-image lock. Event publication invalidates the
image summary before the atomic rename so cancellation, directory-sync failure,
or a failed subsequent state-cache write cannot leave a stale summary. Explicit
state repair also invalidates the summary. Concurrent cold reads coalesce through
the image lock, and a changed image does not invalidate other images.

Image-index saves discard summaries for removed identities. Record/path changes
and configured Pending defaults are composed from current index/configuration
values. Restart rebuilds summaries lazily. No schema, snapshot, import/export
identity, or audit-history change is introduced. Warm filtering still evaluates
lightweight metadata in memory; this does not claim constant-time queries.

## Prelabel hints and accepted annotations

[Model prelabels](prelabels.md) owns the model contract, result publication,
retention, reset, and recovery details. Hints never write workflow events.
Acceptance holds the prelabel control guard before taking the image transaction
lock; reset and API configuration writes use the same control guard. The normal
lock/reload/validate/append/replay path rejects duplicate or suppressed acceptance.
Accepted prediction provenance lives in immutable annotation origin and remains
in event/state schema 3, snapshots, and offline bundles. Derived hint files,
private signing keys, pause markers, and jobs are excluded from snapshots.

Checked prelabel profiles add optional output tensor name, total class count,
explicit model-to-dataset mappings and model digest to dataset configuration.
Historical positional mappings remain readable without rewriting their meaning.
The private hint index records the successful server provider; historical index
entries without that field default to server CPU. Neither change rewrites events.
