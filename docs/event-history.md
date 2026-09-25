# Event history and compatibility

Current writes use schema version 3. These rules preserve historical replay,
server-owned commands, and exact retries. Read [persistence](persistence.md) for
the shared transaction and recovery rules and [assignment](assignment.md) for
current workflow behavior.

## Discovered migration companions

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

## Direct revisit selection

The existing v3 `ManualSelection` migration dependency also records direct revisit
of a resolved canonical target. Replay prioritizes that selection until its exact
save/exclusion clears the marker, then derives the cursor from remaining work.
This adds no persisted field or event type. Older correction-required revisit
markers and historical global-pass events retain their original meanings. Revisit
and save retries reuse the committed command identity and return current replayed
state without duplicating markers or annotation versions.

## Historical correction pass recovery

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

## Reviewer correction rounds

Version 3 also supports `ReviewCorrectionSubmitted`. A correction transaction
appends version/delete/disposition/companion events, renewed migration confirmation
where needed, cancelled competing leases, and this receipt in one atomic log
replacement. The receipt stores the immutable submission, rejected old-round
review, completed assignment and `Submitted` task state with no final outcome.
Replay applies the rejection to the captured round before creating the fresh round.
Derived `reviewCorrectionSubmissions` supports exact retries; missing historical
fields default empty. Existing `ReviewerCorrectionRecorded` events retain their
historical immediate-completion meaning. No old event is rewritten.

All annotation and migration changes remain independently replayable at every
event boundary. The publication transaction simulates the complete batch before
renaming the event log; caches and statistics are then rebuilt/invalidated through
the existing owner. Schema bundles, snapshots and offline wire states include
the receipt, but raw/offline commands cannot author it. Schema version remains 3;
version 2 cannot encode the new event.

## Historical missing-object review evidence

Active commands no longer create this evidence. Historical
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

## Review policy upgrade

Dataset configuration records `reviewPolicyVersion = 1`. Older configurations
without the marker upgrade under the repository's artifact-migration gate,
after any version-2 artifact migration and before ordinary commands can proceed.
The persisted schema remains version 3.

The upgrade preserves existing event bytes. It appends task-state and assignment
changes, rebuilds state caches, updates the generated schema, and publishes the
normalized configuration last. An interrupted upgrade resumes from the original
configuration and already committed image histories. Repeating it does not add
another copy of the same state changes.

A submitted task with a completed approval of every current object and its final
full-image target becomes `Completed`. A historical final rejection returns it
to `NeedsCorrection`. A final row lacking the current object decisions starts a
fresh submitted round, allowing the same reviewer to finish the work. Object-only
partial decisions remain in their original round. Retired pending states return
to `NeedsCorrection`; their old decisions are not treated as approvals.
Previously completed outcomes remain historical outcomes.

Outstanding leases from the old review configuration and retired assignment
kinds are cancelled as expired maintenance leases. Current authorized reviewers
can claim unfinished work with fresh identities. Retired role memberships are
removed without granting replacement roles. Other role memberships and tasks
that complete without review remain unchanged.

Captured historical review configurations retain their original serialization
for event replay and target-fingerprint validation. They never control current
scheduling or new task configuration. Completed historical reviews can be
revisited when the ordinary ownership, history, target and configuration checks
still hold after normalization. Snapshots and offline bundles retain audit
history and the replayed current state.

## Inspector return-to-review history

The server-owned version-3 `work_returned_to_review` event stores the complete
request and captured workflow definitions. The event envelope identifies the
image, actor, role and server timestamp; the request retains the reason and
explicit workflow selection. This is the audit source for later reason
notifications. The inspector does not implement those notifications.

One image transaction validates the complete selection and appends one event.
Replay checks the exact prior sequence, completed states, approval configuration,
lease boundary and authorized actor role, then starts fresh review rounds for
all selected tasks. Existing annotations, decisions and assignment history are
preserved. The transaction follows the configuration guard, image lock,
reload/validate/simulate/append/replay/cache-invalidation order. Retries compare
the persisted request and actor before checking current eligibility.

No new state-cache field or schema version is introduced. Older version-2 and
version-3 histories still replay; version-2 output rejects this new event.
Generated schemas, snapshot event logs and offline bundle event fragments retain
it. Raw event and offline mutation interfaces cannot author it. A missing or
interrupted state cache rebuilds from the committed event log.

## Accepted model predictions

Schema 3 annotations may have immutable `prelabel` origin. It captures the exact
prediction and [execution provenance](prelabels.md#acceptance-and-history).
The accepting revision is human accepted-unchanged or edited; later versions
retain origin. Replay rejects duplicate suggestion acceptance and applies the
recorded confidence/IoU policy against existing boxes at the acceptance event.
Historical v2/v3 histories keep their existing meaning. V2 output rejects new
prelabel origins rather than dropping provenance. Schemas, snapshots and offline
bundles preserve it. New acceptance requires online signed-evidence validation;
ordinary/offline mutations cannot author a new `prelabel_suggestion` revision.
