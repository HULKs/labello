# HTTP API Contract

> **Status:** Normative current reference; internal and unversioned
> **Owner:** API maintainers
> **Audience:** Client, API, and UI maintainers
> **Last verified:** 2026-07-30 at `4f9c332`

This is the current route, access, transport, and error contract. The API is an
internal contract between the bundled Labello clients and server. It has no URL
version prefix and does not promise compatibility for independent external
clients. Coordinate contract changes across `labello-client`, `labello-api`,
all UI callers, and tests.

## Representation Sources

JSON field names and enum wire values are defined by:

- `crates/labello-client/src/dto/` for access, workflow, media, and offline
  request/response DTOs;
- `crates/labello-client/src/import.rs` for import and migration DTOs; and
- versioned `labello-domain` types for persisted workflow, dataset, snapshot,
  and offline artifacts.

Type names in the route tables refer to those definitions. JSON uses each
type's Serde contract; callers must not infer field names from Rust identifiers.
Binary image/preview and snapshot-file responses are identified explicitly.

## Authentication, Authorization, And CSRF

The browser authenticates with the HttpOnly session cookie created by local
development login or GitHub OAuth. `Session` below means a valid cookie.

Dataset access is role-based:

- **Any role:** at least one role in the dataset.
- **Data admin:** the dataset's `data_admin` role.
- **Assigned actor:** the authenticated user owns the exact live assignment;
  its kind requires annotator, reviewer, or adjudicator authority.
- **Bootstrap admin:** a server-configured bootstrap administrator. Import
  additionally limits job access to the creating owner and filters server roots
  by configured owner.

All unsafe methods (`POST`, `PUT`, `PATCH`, and `DELETE`) pass CSRF middleware.
When a session cookie is present, the request must have an allowed `Origin` and
exactly one `x-csrf-token` matching the session. `/auth/local-admin` is the
middleware exception, but independently requires an allowed `Origin`. OAuth
state and its flow cookie are validated at the callback. The flow cookie is
created and expired at the parent path of the validated public OAuth callback
URI, including any proxy prefix. See
[callback configuration](configuration.md#github-oauth) for the accepted path
syntax. Internal authentication route paths remain unchanged.

CORS allows only configured origins, credentials, the implemented HTTP method
set, and these non-simple request headers: `content-type`, `x-csrf-token`,
`idempotency-key`, `upload-offset`, `upload-length`, and `digest`.

## Common Limits And Responses

The default body limit is 128 MiB. Import control JSON is limited to 1 MiB,
browser file registration JSON to 8 MiB, and an import chunk to the advertised
`uploadChunkBytes` limit. These nested limits replace the default for their
routes.

Every response carries `x-request-id`. A supplied ID is retained only when it is
a single, nonempty value of at most 128 ASCII letters, digits, hyphens or
underscores. Missing or invalid IDs are replaced with a generated UUID before
logging and response propagation. Successful JSON responses use the status
selected by the handler, normally 200. Logout returns 204. OAuth endpoints
redirect. Binary endpoints return their media type instead of JSON.

Errors have this JSON shape:

```json
{"error":"safe public message"}
```

The stable status mapping is:

| Status | Meaning |
| ---: | --- |
| 400 | Malformed request, invalid ID/path/assignment, or domain validation failure mapped as bad input |
| 401 | Missing/invalid session, CSRF/origin failure, dataset-role denial, create-dataset bootstrap denial, or identity mismatch |
| 403 | Authenticated actor lacks bootstrap-administrator authority for import |
| 404 | Route-owned resource or storage artifact was not found |
| 409 | Existing destination, state/assignment conflict, unavailable import capability, or keybinding conflict |
| 413 | Request exceeds the applicable body or storage limit |
| 422 | Structurally valid import input cannot be processed |
| 500 | Internal, unexpected storage, upstream HTTP, or serialization failure |

Internal failures return `internal server error`; diagnostics remain in
redacted logs. Clients must display the `x-request-id`, not raw internal state.

## Session And Dataset Routes

| Method and path | Access | Input → output |
| --- | --- | --- |
| `GET /health` | Public | No input → `{"ok":true,"service":"labello"}` |
| `GET /build-information` | Public, no session or CSRF token | No input → compiled artifact `releaseTag` and `sourceCommit`, independently of readiness; `Cache-Control: no-store` |
| `GET /deployment/readiness` | Public; production API is loopback-bound | No input → bounded release identity, schema version, dataset-root traversal, and authentication-store load state; HTTP 503 when a probe fails |
| `GET /auth/options` | Public | No input → `AuthOptions` |
| `POST /auth/local-admin` | Public, allowed origin, feature enabled | No body → `SessionInfo` plus session cookie |
| `GET /auth/github/login` | Public, OAuth enabled | `OAuthLoginRequest` query → temporary provider redirect plus flow cookie |
| `GET /auth/github/callback` | Public, valid OAuth state/flow cookie | `OAuthCallbackRequest` query → configured browser redirect plus session cookie |
| `GET /me` | Session | No input → `SessionInfo`, `Cache-Control: no-store` |
| `POST /logout` | Session optional; CSRF when cookie present | No body → 204 plus expired session cookie |
| `GET /datasets` | Session | No input → `DatasetSummary[]` filtered to accessible datasets |
| `POST /datasets` | Bootstrap admin | `CreateDatasetRequest` → `DatasetMetadata` |
| `GET /datasets/{dataset_id}` | Any role | No input → role-sanitized `DatasetMetadata` |
| `GET /datasets/{dataset_id}/users` | Data admin | No input → `DatasetUser[]` |
| `PUT /datasets/{dataset_id}/roles` | Data admin | `SetDatasetRolesRequest` → `DatasetUser` |
| `GET /datasets/{dataset_id}/admin` | Data admin | No input → full configuration `DatasetMetadata` |
| `PUT /datasets/{dataset_id}/admin` | Data admin | `UpdateDatasetConfigRequest` → full configuration `DatasetMetadata` |
| `POST /datasets/{dataset_id}/ingest` | Data admin | No body → `IngestReport` |
| `POST /datasets/{dataset_id}/ingest-jobs` | Data admin | No body → `IngestJob` |
| `GET /datasets/{dataset_id}/ingest-jobs/{job_id}` | Data admin | No input → `IngestJob` for the same dataset |
| `POST /datasets/{dataset_id}/uploads` | Data admin | `multipart/form-data`, `root` and `ingest` query → `IngestReport` |
| `GET /datasets/{dataset_id}/snapshots` | Data admin | No input → `DatasetSnapshot[]` |
| `POST /datasets/{dataset_id}/snapshots` | Data admin | No body → `DatasetSnapshot` |
| `GET /datasets/{dataset_id}/snapshots/{snapshot_id}/files/{*file_path}` | Data admin | No input → attachment bytes listed by the snapshot manifest |
| `GET /datasets/{dataset_id}/tasks` | Any role | No input → `TaskDefinition[]` |
| `POST /datasets/{dataset_id}/tasks` | Data admin | `TaskDefinition` → `TaskDefinition` |
| `GET /datasets/{dataset_id}/prelabels` | Any role | No input → `PrelabelConfig[]` |
| `POST /datasets/{dataset_id}/prelabels` | Data admin | `PrelabelConfig` → `PrelabelConfig` |
| `GET /datasets/{dataset_id}/images` | Data admin | `ImageExplorerQuery` → `ImageExplorerPage` |
| `GET /datasets/{dataset_id}/stats` | Any role | No input → `DatasetStats` |
| `GET /datasets/{dataset_id}/keybindings` | Any role | No input → authenticated user's `KeybindingSet` |
| `PUT /datasets/{dataset_id}/keybindings` | Any role, same user | `KeybindingSet` → normalized `KeybindingSet` |
| `POST /datasets/{dataset_id}/prelabel-suggestions` | Annotator; enabled config | `PrelabelSuggestionRequest` → `PrelabelSuggestion[]` |

`KeybindingSet.bindings` contains the primary chord for every active action.
`panDragModifier` selects the modifier used with primary-button drag to pan and
defaults to `control`. Middle-button drag is always available. Older requests
that omit `panDragModifier` remain valid and receive the default during
deserialization.

Role mutation retains bootstrap-administrator protections implemented by the
handler; a data administrator cannot use this route to bypass those rules.

`DatasetMetadata.imbalance` and `UpdateDatasetConfigRequest.imbalance` use the
direct `maxDifference` representation documented in
[Server configuration](configuration.md#dataset-assignment-balance). The
administration update route rejects ratio and tagged-policy shapes. Assignment
statistics include the current annotation and review counts plus the task IDs
blocked by the enforced window.

## Assignment And Image Routes

| Method and path | Access | Input → output |
| --- | --- | --- |
| `GET /datasets/{dataset_id}/assignments/availability` | Session; requested kind authorized by storage | `AssignmentAvailabilityRequest` query → `AssignmentAvailability` |
| `POST /datasets/{dataset_id}/images/next` | Session; requested kind authorized by storage | `AssignNextRequest` → `Assignment?` |
| `POST /datasets/{dataset_id}/images/{image_id}/assignments/revalidate` | Owner of exact active assignment | `AssignmentActionRequest` → `AssignmentRevalidation?` |
| `POST /datasets/{dataset_id}/assignments/release` | Assigned actor | `AssignmentActionRequest` → `Assignment` |
| `POST /datasets/{dataset_id}/assignments/complete` | Assigned annotator | `AssignmentActionRequest` → `Assignment` |
| `POST /datasets/{dataset_id}/assignments/reopen` | Owner of exact prior annotation or eligible review assignment | `AssignmentActionRequest` → `Assignment` |
| `GET /datasets/{dataset_id}/images/{image_id}` | Any role | No input → `ImageState` |
| `GET /datasets/{dataset_id}/images/{image_id}/record` | Any role | No input → `ImageRecord` |
| `GET /datasets/{dataset_id}/images/{image_id}/file` | Any role | No input → original image bytes and stored media type |
| `GET /datasets/{dataset_id}/images/{image_id}/preview` | Any role | `max` query clamped to 256–4096 → raw RGBA bytes (`application/octet-stream`) plus `x-image-width` and `x-image-height`; bounded legacy fallback |
| `GET /datasets/{dataset_id}/images/{image_id}/encoded-preview` | Any role | `profile=standard_v1` (default) or `data_saver_v1` → bounded `image/webp`, `x-image-width`, `x-image-height`, `x-original-width`, `x-original-height`, `x-preview-profile` |
| `GET /datasets/{dataset_id}/images/{image_id}/detail` | Any role | Explicit bounded original-detail display; original encoded bytes and decoder-format MIME, `private, no-store` |
| `POST /datasets/{dataset_id}/images/{image_id}/events` | Assigned annotator; role also derived from allowed payload | `AssignmentActionRequest` query plus `AppendEventRequest` → `EventLogEntry` |
| `POST /datasets/{dataset_id}/images/{image_id}/annotation-batch` | Assigned annotator | `AssignmentActionRequest` query plus `AnnotationBatchRequest` → `ImageState` |
| `POST /datasets/{dataset_id}/images/{image_id}/admin/events` | Data admin | `AppendEventRequest` with permitted repair payload → `EventLogEntry` |
| `POST /datasets/{dataset_id}/images/{image_id}/rebuild` | Any role | No body → replayed `ImageState` |
| `POST /datasets/{dataset_id}/images/{image_id}/reviews` | Assigned reviewer | `AssignmentActionRequest` query plus `ReviewRecord` → `ImageState` |
| `POST /datasets/{dataset_id}/images/{image_id}/missing-object-rejections` | Owner of active ordinary final-review assignment; reviewer role | `AssignmentActionRequest` query plus `MissingObjectRejection` → `ImageState` |
| `POST /datasets/{dataset_id}/images/{image_id}/review-revisions` | Owner of active decision-revision lease; reviewer role | `AssignmentActionRequest` query plus `ReviewRevisionCommit` → `ImageState` |
| `POST /datasets/{dataset_id}/images/{image_id}/corrections` | Assigned reviewer | `AssignmentActionRequest` query plus `CorrectionRequest` → `EventLogEntry` |
| `POST /datasets/{dataset_id}/images/{image_id}/adjudications` | Assigned adjudicator | `AssignmentActionRequest` query plus `AdjudicationRecord` → `EventLogEntry` |
| `GET /datasets/{dataset_id}/offline-bundle` | Annotator | `OfflineBundleRequest` query → `OfflineBundle` |
| `POST /datasets/{dataset_id}/offline-sync` | Annotator; same authenticated user and dataset | versioned `OfflineSyncRequest` → `OfflineSyncResult` |

The assignment ID, image ID, task ID, actor, kind, current sequence, and live
state are validated at the transaction boundary. Possessing an ID is not
authorization.

Availability, direct claims, and prepared queue claims apply the same
completion-balance decision. The complete count, denominator, disabled-peer,
zero-count, and exact-boundary contract is maintained in
[Assignment](assignment.md#completion-balance).

Review reopening and replacement follow the strict
[previous-review contract](assignment.md#previous-review-and-decision-revisions).
`ReviewRevisionCommit` contains `reviews`, from 1 to 10001 `ReviewRecord` values.
Each record must belong to the caller, have a unique ID and captured exact target,
and contain at most 2000 bytes of comment. The final record is the task or
migration-confirmation decision. Approval must include every captured target
without a rejected object. Foreign actors or missing reviewer authority return
401. Invalid syntax or assignment-kind input returns 400; changed context,
expired ownership, malformed replacement targets, and conflicting retries return
409. Neither ordinary reviews nor correction endpoints can mutate a task held
by an exclusive decision-revision lease.

`ReviewAssignmentOpened`, `ReviewAssignmentFinished`, and
`ReviewRevisionCommitted`, and `MissingObjectEvidenceRecorded` are server-owned events. Raw event, annotation batch,
admin repair, and offline sync ingress cannot publish them. Clients submit
commands to the dedicated endpoints and never choose superseded review IDs.

Missing-object rejection accepts `review`, the captured `round`, and 1–64
`locations`. Each location has a nonzero `markerId` unique within that request,
a `classId` from the assigned task, and a finite normalized `position` within
`[0, 1]` on each axis. This command requires an ordinary rejected Task target,
the current submission round, all exact object targets already reviewed by the
caller, and an active lease. It rejects correction, migration, foreign targets,
and changed task configuration. Invalid locations return 400; stale phase,
round, ownership, or conflicting retries return 409. The assignment ID identifies
the immutable request for exact retries. Current reviewer authority is still
required on retry.

`ReviewRevisionCommit` also accepts optional `missingObjects`, defaulting to an
empty list. Nonempty locations require an ordinary final rejection and a full
replacement target set. The transaction publishes decisions, evidence, task
state, and assignment completion together. Repeating the same commit is safe;
changing locations on a retry is a conflict. `ImageState` exposes evidence and
history; markers do not create annotation versions or IDs.

## Manual Migration Routes

Every route requires a valid session, exact owned assignment, the role shown,
and exactly one `idempotency-key` containing 1–200 visible ASCII characters
other than comma or semicolon. Responses are
`ManualMigrationCommandResult`.

| Method and path | Role | Request |
| --- | --- | --- |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/skeleton` | Annotator | `SaveMigrationSkeletonRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/skeletons` | Annotator | `AddMigrationSkeletonRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/skeletons/edit` | Annotator | `EditMigrationSkeletonRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/skeletons/delete` | Annotator | `DeleteMigrationSkeletonRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/skeletons/reconcile` | Annotator | `ReconcileMigrationCompanionRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/exclude` | Annotator | `ExcludeMigrationTargetRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/reopen` | Annotator | `ReopenMigrationTargetRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/revisit` | Annotator | `RevisitMigrationTargetRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/passes` | Annotator | `StartMigrationPassRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/keep` | Annotator | `KeepMigrationTargetRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/confirm` | Annotator | `ConfirmMigrationRequest` |
| `POST /datasets/{dataset_id}/images/{image_id}/migration/review` | Reviewer | `ReviewMigrationRequest` |

Adding, editing, or removing a missing-object skeleton is allowed only at the
full-image cursor. Edit and delete requests identify an active, native,
group-less skeleton for the selected migration task and supply its exact
expected version; stale or non-migration annotations are rejected.

New save, add, and edit requests must contain at least one visible or hidden
keypoint with coordinates. A skeleton containing only absent keypoints returns
HTTP 400 with the safe invalid-assignment message
`manual migration skeleton requires at least one positioned keypoint`.
Idempotent replay is checked first, so an identical retry of a successfully
recorded historical all-absent command still replays without appending events.

Each newly discovered skeleton is saved with one ordinary bounding box in the
configured guide task, in the same per-image event transaction. The box bounds
include positioned visible and hidden keypoints and span at least one original
image pixel per axis. The server chooses its task, class and stable identity;
clients cannot supply another guide task. The guide task enters `needs_correction`
with its terminal outcome cleared. Existing history and review records remain.

Still-derived companions follow skeleton edits and deletion. An independent box
edit or review, active guide-task assignment, stale version, or changed guide
configuration rejects the whole operation. Explicit reconciliation requires the
same active migration annotation assignment and full-image cursor. Its request
contains `assignmentId`, `passId`, `taskId`, `annotationId`, `expectedVersion`,
and nullable `expectedBoxVersion`. A missing box uses null; regenerating an
existing box requires its exact current version. Reconciliation creates or
regenerates one box from the saved skeleton and returns the ordinary migration
command result. Identical command retries do not append or duplicate objects.

Historical reconciliation requires an active group-less native skeleton with a
recorded full-image discovery creation event. Ordinary skeletons without that
provenance, coordinate-less skeletons, inconsistent links and competing work
remain unresolved with an actionable conflict. Each committed link is a durable
unit of progress: after interruption, inspect current links and reconcile the
remaining objects. Reads never perform reconciliation.

Migration review visits canonical dispositions, then every active discovered
skeleton in annotation-ID order, then the full-image confirmation. A discovered
review target is `{ "targetType": "discovered", "annotationId": "…", "version": 1 }`;
it binds the exact current skeleton version. A rejected discovery must receive
a new version or be removed before submission. Companion boxes retain the
ordinary guide-task review flow and are never approved by migration review.

## Dataset Import Routes

All import routes require a bootstrap-administrator session. Job routes also
require ownership of the import. A missing/disabled import service returns
409. Mutating routes marked “Idempotent” require exactly one
`idempotency-key` with the same syntax as migration routes. Reusing a key for a
different operation or request is a conflict; a completed identical request
replays its response.

| Method and path | Additional contract | Input → output |
| --- | --- | --- |
| `GET /import-capabilities` | Authorized roots are filtered per actor | No input → `ImportCapabilities` |
| `POST /import-roots/{root_id}/browse` | Root must be configured for actor | `BrowseServerImportRootRequest` → `ImportBrowsePage` |
| `GET /imports` | Owned jobs only | No input → `ImportJob[]` |
| `POST /imports` | Idempotent | `CreateImportRequest` → `ImportJob` |
| `GET /imports/{import_id}` | Owned job | No input → `ImportJob` |
| `POST /imports/{import_id}/files/register` | Idempotent; 8 MiB body | `RegisterImportFilesRequest` → `RegisterImportFilesResult` |
| `POST /imports/{import_id}/files/{file_id}/chunks` | Idempotent; advertised chunk limit | Raw bytes plus `upload-offset`, `upload-length`, and full BLAKE3 `digest` headers → `ImportChunkResult` |
| `POST /imports/{import_id}/source/browse` | Owned job | `BrowseImportSourceRequest` → `ImportBrowsePage` |
| `POST /imports/{import_id}/yolo-descriptor/inspect` | Owned job | `InspectYoloDescriptorRequest` → `YoloDescriptorInspection` |
| `POST /imports/{import_id}/seal` | Idempotent | `SealImportRequest` → `SealImportResult` |
| `POST /imports/{import_id}/preflight` | Idempotent | `StartImportPreflightRequest` → `ImportJob` |
| `GET /imports/{import_id}/plan` | Current accepted/preflight plan required | No input → `ImportPlan` |
| `PUT /imports/{import_id}/plan` | Idempotent | `UpdateImportPlanRequest` → `ImportPlan` |
| `GET /imports/{import_id}/diagnostics` | Limit bounded by capabilities | `ImportDiagnosticsQuery` → `ImportDiagnosticsPage` |
| `POST /imports/{import_id}/commit` | Idempotent; reauthorizes every attempt | `CommitImportRequest` → `CommitImportResult` |
| `POST /imports/{import_id}/cancel` | Idempotent; not committing/succeeded | `CancelImportRequest` → `CancelImportResult` |

## Change Discipline

Route, role, middleware, body-limit, error, DTO, header, or status changes must
update this document in the same change. The router and focused API tests are
the route/access regression suite. The open documentation-automation issue
still tracks generated inventory or an explicit test that detects omissions in
this table.

## Public build information

`GET /build-information` returns only `releaseTag` and `sourceCommit`. Each is
nullable: absent or invalid compiled metadata is `null`, never the shared Cargo
package version. Tags contain at most 64 ASCII letters, digits, dots, underscores,
plus signs or hyphens; commits contain exactly 40 or 64 hexadecimal characters.
The reserved tag `development` represents missing release metadata and is exposed
as `null`. The response is under 200 bytes, performs no persistence or
authentication probe, and remains HTTP 200 when `/deployment/readiness` returns
503. Existing credentialed CORS rules also apply to this public route.

The client uses a ten-second request timeout, validates the two-field DTO and
rejects responses over 1024 bytes. The response must not expose readiness,
configuration, authentication state, paths, or other server details.

## Encoded Working Previews

Standard v1 resizes to at most 1600 pixels on the longest edge and encodes
lossless WebP. Data Saver v1 uses at most 1280 pixels and lossy WebP quality 80
(on libwebp's 0–100 scale). Neither profile upscales. Both preserve the existing
Triangle resize at the decoder's native channel depth followed by RGBA8
conversion, first-frame behavior, and no EXIF orientation or ICC conversion.
Standard decoded RGBA, including transparent RGB, is identical to the legacy
1600 preview. Original record dimensions remain authoritative for geometry.

Both routes authenticate and authorize each request, including cache hits, and
recheck session, roles, and image index after worker completion. Source content
is verified against its authoritative BLAKE3 hash before every cache read.
Responses use `Cache-Control: private, no-store`; encoded dimension and profile
headers are exposed by credentialed CORS. Encoded responses are at most 16 MiB.
Limits are configured under [previews](configuration.md#image-preview-limits).
Oversized source/pixel/decoder requests return 413; busy workers, exhausted cache
quota, or stale source identity return 409; unsupported/unavailable sources and
decoding failures return 422; unavailable cache/encoder failures return 500.
Errors never include source paths or decoder text.

`ImageApi::get_encoded_image_preview` returns `EncodedImagePreview`, separate
from `ImagePreview::rgba`. HTTP clients bound streaming response bytes and
validate MIME/profile/metadata; native and WASM use the same bounded Rust WebP
decoder. The UI always requests Data Saver v1 for working-image loads, reloads,
retries and prefetch. Generation, transfer and decode errors propagate without
requesting Standard, legacy RGBA or original bytes. The encoded route retains
its Standard v1 default for API callers that omit the profile. Image assignment,
annotation geometry and draft state are independent of the representation.
Existing request/auth/workspace epochs reject stale image replies and clear account-scoped texture/prefetch state.

### Explicit original detail

`ImageApi::get_original_detail` and `GET .../detail` remain explicit API
capabilities; the working UI does not call them or expose an original-detail
action. Ordinary original-file download keeps its existing separate contract.
Detail reads share preview source-byte, pixel, decoder-header/allocation
and worker limits, secure source opening/hash verification, and final
session/role/index checks. They return original source bytes without a derived
cache entry. The client bounds streaming bytes to 64 MiB and decoding to
32 million original pixels / 256 MiB decoder allocation, checks declared MIME
and authoritative original dimensions, and applies no EXIF/ICC transform. Larger
server configuration limits do not raise these browser detail limits.

Every working-image load and prefetch uses the encoded Data Saver v1 route.
Saved browser quality preferences are ignored; preview failure never chooses
original detail. Aborting superseded image transfers does not abort assignment
claims: their replies must still be received so obsolete reservations can be released. Server workers already
started retain their configured bounds through completion/cleanup.
