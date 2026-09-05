# Persistence And Recovery

> **Status:** Normative current reference
> **Owner:** Storage maintainers
> **Audience:** Operators and maintainers
> **Last verified:** 2026-07-30 at `4f9c332`

This document defines the current on-disk authority, compatibility, atomicity,
and recovery contract. The code and storage/domain tests remain the executable
source of truth. See [Operations](operations.md) for backup, restore, upgrade,
and incident procedures.

## Root Layout

```text
<datasetsRoot>/
  .labello-server/
    auth.json
    imports/
    exports/<job-id>/
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

## Artifact Authority

| Artifact | Classification | Recovery rule |
| --- | --- | --- |
| `.labello-server/auth.json` | Authoritative secret authentication/session state | Restore only from the matching full-root backup; never log, publish, or merge it |
| `.labello-server/imports/` | Authoritative private in-progress import, reservation, upload, and API control state | Let startup recovery reconcile it; do not delete apparently stale workspaces manually |
| `.labello-server/exports/` | Private derived captures, job state, and verified archives | Startup interrupts unpublished jobs and preserves completed artifacts until expiry; never edit job records or publish partial files |
| `labello.dataset.toml` | Authoritative dataset configuration, workflow definition, and role state | Valid supported schema required; restore rather than hand-edit damaged data |
| `images-index.json` | Authoritative image identity, hash, path, and metadata index | Valid supported schema required; image-directory contents alone do not reproduce stable identities |
| `images/` | Authoritative image bytes addressed by the image index | Include in full backups; omitted from Labello snapshots |
| `annotations/<image-id>/events.jsonl` | Authoritative append-only audit and workflow history | Replay in sequence; never truncate, reorder, merge, or edit by hand |
| `annotations/<image-id>/state.json` | Derived, rebuildable cache | Rebuilt automatically when absent, stale by event sequence, on a supported older schema, or with an older review projection generation |
| `labello.schema.json` | Generated schema bundle | Regenerated during supported artifact migration and before publishing companion-link or captured review-assignment events; do not treat it as annotation authority |
| `users/<user-id>/keybindings.toml` | Authoritative keyboard and pan-drag user shortcuts, not workflow state | Back up separately from Labello snapshots; normalize missing current bindings through storage |
| `.labello/imports/<import-id>/manifest.json` | Authoritative committed import provenance | Must match the dataset and directory import ID |
| `.labello/imports/<import-id>/source-objects.jsonl` | Authoritative committed source-object audit record | Preserve with its manifest and event history |
| `.labello/migrations/...` | Durable migration journal and staged generation | Recovery state until migration completion; do not remove during an interrupted migration |
| `.labello/snapshots/` | Derived point-in-time annotation/audit packages | Downloadable but not directly restorable; retain according to operator policy |
| Statistics and process caches | Derived | Recompute or invalidate after authoritative writes |

Browser IndexedDB/local-storage drafts and availability caches are recoverable
client conveniences. They are outside the server root and never authoritative
workflow state.

## Write And Transaction Boundaries

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

## Schema Compatibility

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

## Recovery Behavior

### State Cache

Loading an image compares `state.json` with the image ID, supported schema, and
last event sequence. The current review projection generation must also match,
so a same-sequence cache from before review-round tracking is rebuilt. Missing or stale state is replayed from `events.jsonl` and
written back when appropriate. A malformed authoritative event prevents replay
and requires backup restore or maintainer-led forensic repair.

### Artifact Migration

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

## Repair Rules

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

## Contract Verification

The current contract is exercised by:

- `crates/labello-domain/src/v2_contract_tests.rs` and
  `v3_import_tests.rs` for wire compatibility and upcasting;
- `crates/labello-storage/src/repository/tests.rs` for event authority,
  stale/missing state rebuild, interrupted artifact migration, snapshot
  contents, and committed import records;
- `crates/labello-storage/src/import/tests.rs` for publication and startup
  recovery; and
- API tests for authorization and safe access to snapshots and import state.

Persistence-format changes must update this reference and the smallest fixture
or test that would fail if the stated compatibility or recovery rule regressed.

## Derived Preview Cache

Encoded previews are disposable derived artifacts outside `datasetsRoot` in the
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

## Discovered Migration Companions

Schema version 3 represents a discovered skeleton/box relationship with
`migration_companion_linked` events. The replayed `migrationCompanions` map is
keyed by the stable skeleton annotation ID and records both task IDs, class ID,
box ID, skeleton version and derived box version. The box revision source is
`migration_skeleton` with the exact source annotation ID and version. Neither
annotation receives an imported object group, and the frozen canonical target
set is never extended or reassigned.

Creation, automatic edits, withdrawal and explicit reconciliation use the
existing lock/reload/validate/simulate/append/replay transaction. A pair is
published in one event append; a failure does not publish a partial pair.
Deterministic command and companion IDs make retries durable across restart.
A linked box that is independently edited or reviewed cannot be overwritten by
an automatic skeleton update. Explicit reconciliation checks both current
versions and records a new box version and link event. Prior versions, deleted
versions and review history remain replayable. Each repaired object's link is
the durable progress record; unresolved objects remain unchanged and can be
retried independently after their conflict is resolved.

Companion edits invalidate migration confirmation and terminal task state.
Confirmation digests bind current discovered skeleton and companion versions.
Histories without companions retain their existing digest encoding. Legacy
version-2 and version-3 events remain readable; version-2 wire output rejects
new companion provenance instead of silently discarding it. Old state caches
without the additive map decode with an empty map and rebuild from events.
Snapshot states, event logs, generated schemas and offline bundles retain these
links. Offline mutations cannot forge companion events or derivation provenance.

## Direct Revisit Selection

The existing v3 `ManualSelection` migration dependency also records direct revisit
of a resolved canonical target. Replay prioritizes that selection until its exact
save/exclusion clears the marker, then derives the cursor from remaining work.
This adds no persisted field or event type. Older correction-required revisit
markers and historical global-pass events retain their original meanings. Revisit
and save retries reuse the committed command identity and return current replayed
state without duplicating markers or annotation versions.

## Historical Correction Pass Recovery

The current UI creates no global correction passes. Existing pass-start and
pass-item events still decode and replay without rewriting their audit history.
An assignment resumes its latest persisted pass, ordered by `started_at`, using
the existing exact task and assignment selection. Outstanding items require a
current guide/disposition decision through normal keep, edit or exclude commands.
Restarting the server or client preserves this work.

Submission requires the latest assignment pass to be complete. Earlier passes
retain decisions about the revisions that existed when those events were
recorded; later annotation edits do not make those historical passes new work.
The ordinary current-resolution, dependency, assignment-ownership and confirmation
digest checks still apply. A stale current guide or incomplete latest pass cannot
be bypassed by an older completed pass. No event shape or persisted schema changes.
## Review rounds and decision revisions

Review rounds bind to the authoritative submission event ID and sequence.
Ordinary submission, imported submitted initialization, and imported-task reopen
use the same round owner. A replacement decision that returns a task to
`Submitted` does not create a submission round. Historical review rows remain
immutable; effective projections filter by round and explicit superseded IDs.

Version 3 adds `ReviewAssignmentOpened`, `ReviewAssignmentFinished`, and
`ReviewRevisionCommitted`. Opening captures the current task definition, exact
targets and fingerprint, round, source assignment, and complete supersession
set. Finishing records the terminal transaction boundary by event sequence,
without treating equal timestamps as one transaction. Commit stores replacement
records, explicit superseded IDs, task state, and completed fresh assignment in
one replayable event. Replay validates the captured targets and supersession
set and simulates replacements before mutating state.

These events use the existing atomic event-log append transaction. A process
stop after event publication recovers the same outcome by replay. Derived state
stores the round mapping, contexts, terminal boundaries, and committed requests
for exact retries. Missing fields in older version-3 caches trigger rebuilding;
old version-2 and version-3 event histories remain readable. New review events
cannot be encoded as version 2. Snapshots, generated schemas, and offline bundle
states include the new data; offline clients cannot author these server events.
The persisted schema remains version 3.

A process-local dataset configuration read/write lock prevents configuration
publication racing review-context capture or revision commit. Per-image locks
still guard event validation, exclusive revision ownership, and publication.
A live revision excludes relevant annotation, review, migration, and assignment
mutations, including mutation paths used by offline synchronization.

## Missing-object review evidence

`MissingObjectEvidenceRecorded` is a server-owned version-3 event appended in
the same atomic transaction as a final rejection and assignment completion,
before its `ReviewAssignmentFinished` boundary. Ordinary review retains its
`ReviewRecorded` event; decision revisions retain `ReviewRevisionCommitted`.
The evidence event contains the immutable request for retry comparison and the
server-derived dataset, image, task, assignment, review, reviewer, annotation
type, authoritative event timestamp, submission round, and normalized locations.
Marker IDs are local to one review and never reserve annotation identity.

Replay checks the exact completed review assignment, final rejection, round,
target set, actor, location validation, and terminal boundary before adding
`missingObjectEvidence` and `missingObjectSubmissions` to derived state. A
revision's evidence must equal its committed `missingObjects`. Existing records
have no evidence by default. Version 2 cannot encode the new event. Snapshots,
schemas, and offline bundle state preserve evidence; raw and offline mutation
paths cannot author it.

Active guidance comes from the latest effective rejected Task review in the
current authoritative submission round. Correction does not clear it; a true
resubmission does. An empty later rejection has no active locations. Superseded
rejections remain in history, and an effective replacement approval removes
that review's active guidance. Creating an annotation near a marker neither
resolves nor deletes evidence. Evidence does not count as an additional completed review.

## Export capture and recovery

Export captures replay exact per-image event cuts under the existing image
locks and copies original bytes after releasing those locks. Source event
logs, caches, and configuration are not modified. Fresh configuration and
index digests, root identity, and original-image hashes are checked before
artifact publication. Captured event sequences remain fixed if later source
events are appended.

Private mode-0700 job directories contain `job.json`, staged payloads, and
an unpublished archive. A verified archive is linked without replacement as
`dataset.zip`, synced, and then recorded as succeeded. Only succeeded jobs
are downloadable; each download verifies size and BLAKE3. A crash before the
durable succeeded status makes the job interrupted on restart and removes
its payload. Completed archives survive restart until retention expiry.
Orphan reservations and expired entries are cleaned before retained capacity
is checked. See [export](export.md) for the artifact contract.
