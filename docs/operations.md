# Operations

> **Status:** Normative current reference
> **Owner:** Server maintainers
> **Audience:** Operators and maintainers
> **Last verified:** 2026-07-30 at `5f10153`

## Logging

The server logs Labello application targets at `INFO` by default. Override the
filter with `RUST_LOG`:

```bash
RUST_LOG=labello_server=debug,labello_api=debug,labello_storage=debug \
  cargo run -p labello-server
```

Logs are human-readable text by default. Set `LABELLO_LOG_FORMAT=json` for
structured JSON output:

```bash
LABELLO_LOG_FORMAT=json cargo run -p labello-server
```

Invalid `RUST_LOG` filters or `LABELLO_LOG_FORMAT` values stop startup rather
than silently disabling logs.

Every HTTP response includes `x-request-id`. Request completion logs contain
the same ID, HTTP method, matched route template, status, and latency. Browser
API errors include the request ID in their displayed message.

The WASM development build writes startup and API diagnostics to the browser
console at `DEBUG` and above. Release builds report only warnings and errors.

## Event Levels

- `ERROR`: dependency and internal API failures, corrupt authentication state,
  failed or panicked background jobs, poisoned-lock recovery.
- `WARN`: authorization denials, enforced API resource limits, skipped corrupt
  datasets, unreadable images, cache recovery, browser persistence failures.
- `INFO`: server lifecycle, HTTP completion, successful authentication,
  dataset administration, ingest, upload, import lifecycle, snapshots, and
  offline sync, validation and conflict rejections, missing routes and methods.
- `DEBUG`: assignment, annotation, review, correction, adjudication, and
  expected unauthenticated browser requests.

## Request failure diagnostics

`logging::observe_response` owns the request failure diagnostic and the separate
`http.request.completed` lifecycle event. Each response emits one INFO completion
record, including successes, redirects, extractor rejections, and unmatched routes.
Every 4xx/5xx response also emits at most one diagnostic. `ApiError` attaches a
bounded category to response extensions; it does not log independently. Responses
without that extension use the status mapping below, never a body or error string.

Both events share the `http.request` span's request ID, bounded HTTP method and
matched route template. Unmatched routes use `<unmatched>` and extension methods
use `<other>`. Dataset-role denials add validated user and dataset IDs to that
span; bootstrap-administrator denials add a validated user ID. Missing or invalid
identifiers are omitted. No actor is inferred from a rejected cookie or header.

| Failure path | Diagnostic event and level | Category / logging input owner |
| --- | --- | --- |
| Application validation, invalid IDs, invalid assignments/corrections and conflicts | `api.request.rejected`, INFO | `ApiError` variant or bounded `StorageError::kind()` |
| JSON, path/query, content-type and multipart extractor rejection | `api.request.rejected`, INFO | Status fallback such as `bad_request`, `unprocessable_entity`, `unsupported_media_type` |
| Authentication, dataset roles, bootstrap administrator, import-root denial, OAuth flow validation, CSRF/origin middleware | `api.request.rejected`, WARN | `unauthorized`, `forbidden`, or `storage_unauthorized`; auth helpers attach safe IDs without a second event |
| Import ownership denial hidden as HTTP 404 | `api.request.rejected`, WARN | `forbidden`; public not-found status and body are preserved |
| Body-size and mapped import size limits | `api.request.rejected`, WARN | `payload_too_large`; either extractor fallback or `ApiError` |
| Multipart body reads and annotation batch count | `api.request.rejected`, WARN | `resource_limit`; public legacy HTTP 400 response is preserved |
| Import reservation, upload/build concurrency, descriptor inspection and parser-time budgets | `api.request.rejected`, WARN | `resource_limit`; public 409/422 responses are preserved |
| Import descriptor, parser structure, image decode, annotation/category/task/coverage, generated-artifact and staging budgets | `api.request.rejected`, WARN | `resource_limit`; preserve existing 422 or generic 500 responses, including legacy limit mappings |
| Expected unauthenticated `GET /me` | `api.request.rejected`, DEBUG | `unauthorized`; the INFO completion remains visible with the default filter |
| Missing resources/routes and unsupported methods | `api.request.rejected`, INFO | `not_found`, `storage_not_found`, `method_not_allowed`; no raw path fallback |
| HTTP dependency, serialization, storage or internal application failures | `api.error`, ERROR | `http_client`, `serialization`, bounded storage kind, or `internal` |
| Readiness 503 or another 502/503/504 without an application category | `api.error`, ERROR | `dependency_unavailable` from status |

There is no request-rate limiter. A future HTTP 429 response uses WARN category
`rate_limit`; this fallback is not a claim of implemented rate limiting. Other
4xx responses use INFO `request_rejected`; other 5xx responses use ERROR `internal`.
CORS preflight responses that omit permission headers can still be HTTP 200, so
only completion is logged. A browser CORS refusal is not a server error response.
Disabled import is an expected capability conflict at INFO, not a failed upstream
request. Import plan diagnostics returned successfully inside job/plan JSON are
not HTTP failures and receive only the completion record. These exceptions avoid
treating every 4xx as a warning.

Startup failures, background job failures, poisoned-lock recovery, skipped corrupt
datasets and failed import cleanup remain separate operational events. They describe
work or recovery, not another diagnostic for the final HTTP response. API events
use bounded categories and omit parser/I/O diagnostic text even when a helper calls
that text "safe". Direct calls to `ApiError::into_response` outside the router do
not emit request logs because they lack request context.

## Redaction

Logs must never contain:

- Cookies, OAuth codes or state, access tokens, client secrets, or authorization
  headers.
- Raw URLs, query strings, request or response bodies, multipart content, or
  uploaded file names.
- Image bytes, annotation geometry, review comments, event payloads, or browser
  drafts.
- Import source paths or names, raw labels, parser excerpts, source URLs,
  exclusion notes, or CSRF and idempotency values.

Request logs use matched route templates instead of raw URLs. Internal server
errors return a generic message to clients; safe error categories and bounded
diagnostics remain in server logs.

## Health And Availability

`GET /health` is an unauthenticated liveness check. A healthy response is
HTTP 200 with:

```json
{"ok":true,"service":"labello"}
```

It proves that the Axum process can accept and answer a request. It does not
read the configuration, authentication store, dataset root, a dataset, import
staging, or available disk space on each request. It is therefore not a
readiness or durability check.

`GET /deployment/readiness` is an unauthenticated, non-mutating admission
probe for the rootless deployment manager. It checks that the configured
dataset root is a traversable directory and that the authentication store
loaded, then returns bounded `ok` or `failed` states plus the compiled release
tag, source commit, and schema version. It does not test write capacity, free
space, representative dataset reads, import filesystem capabilities, OAuth,
or browser networking. The production API remains loopback-bound behind Caddy.

Startup is fail closed for
configuration parsing, authentication-store initialization, bind failure, and
I/O errors while initializing the import service. Startup probes secure import
publication; a safely detected unsupported platform leaves import unavailable
instead of failing the whole server. A process that has emitted
`server.started` has passed those startup checks, but later filesystem or disk
failures remain request-time failures. Use `/health` for liveness. Deployment
uses `/deployment/readiness` for its bounded admission contract. Combine both
with external checks for dataset-root mount presence, write capacity, and free
space before routing production traffic. Never probe readiness by modifying a
dataset.

## Graceful Shutdown

The server installs a Ctrl-C handler and passes it to Axum graceful shutdown.
After Ctrl-C, `server.shutdown.started` is emitted, Axum stops accepting new
connections, and the process waits for active connections before emitting
`server.stopped`. There is no application-level drain deadline and no
documented SIGTERM handler. A process supervisor must therefore send the
supported interrupt signal and allow enough time for the longest accepted
request.

Import preflight and commit work is owned by the request performing it, not a
detached durable worker. Do not force-kill the process while a write or import
is active. If termination is unavoidable, restart against the same complete
dataset root: import recovery reconciles durable phases and event/state writes
use recoverable persistence boundaries. Recovery does not make a partially
copied filesystem backup consistent.

## Dataset Import

Current explicit lifecycle logs are `import.created`, `import.sealed`,
`import.preflight.completed`, `import.preflight.phases`, `import.committed`,
and `import.recovery.completed`. Import failures can currently appear only as
the generic `api.error` or `api.request.rejected` events, and cancellation does
not emit a dedicated lifecycle event. Do not build alerts that assume
`import.failed` or `import.cancelled` exists until
[issue #29](https://github.com/HULKs/labello/issues/29) is completed.

Safe import fields are limited to import and destination IDs, actor ID,
profile, phase, aggregate counts, elapsed time, and a bounded error category.

Persistent jobs and reservations live below `.labello-server/imports`. Startup
recovery validates staged generations, resumes schema migrations, reconciles a
publication completed before its job update, expires abandoned non-protected
jobs, and releases reservations that no active job owns. `building`,
`verifying`, and `committing` jobs are never expired mid-operation.

The storage service contains configured cleanup for retained failed,
cancelled, and successful job metadata, but the production server does not
currently invoke or schedule it. The retention settings are therefore not an
operational cleanup guarantee. Terminal job metadata and API control records
can grow until [issue #27](https://github.com/HULKs/labello/issues/27) and
[issue #28](https://github.com/HULKs/labello/issues/28) are completed.

Import is available only when the configured filesystem passes secure
beneath-open, file/directory sync, and atomic no-replace publication probes.
There is no best-effort publication fallback.

Labello currently exposes no metrics endpoint. `import.preflight.phases`
provides preflight phase durations plus aggregate source and output counts;
`import.preflight.completed` provides a total diagnostic count. Diagnostic
severity totals, cleanup failures, inactive-job age, and staged-byte gauges are
not complete production signals. Monitor free space and dataset-root
availability externally, and treat the richer import alert set in
[issue #30](https://github.com/HULKs/labello/issues/30) as unavailable until
its instrumentation is implemented.

## Production Deployment

The supported production release and rootless guest transaction are documented
in [Release and deployment](deployment.md). The deployment runner lives inside
the Debian 12 guest, verifies an immutable attested GitHub release, and submits
it to the local Rust transaction manager. Automated SSH deployment, remote
polling, sudo, job-level deployment containers, and Python are not part of the
production path.

Run the service under a dedicated, unprivileged account. That account needs:

- read/write/create/rename/sync access to `datasetsRoot` and all managed
  descendants;
- read access to configured server import roots;
- read access to the server configuration and access to injected OAuth
  secrets;
- no permission to traverse unrelated source trees.

The dataset root must remain on a filesystem that preserves ordinary file
contents and permissions and supports same-filesystem atomic rename. Import
additionally probes Linux beneath-open behavior, file and directory sync, and
atomic no-replace directory publication. Do not place one dataset root behind
multiple server processes: filesystem locking and in-memory caches are
process-local. A shared network filesystem does not change this constraint.

Deploy the browser and API behind TLS. Set `sessionCookieSecure = true`, disable
local development login, restrict `browserOrigins` to exact HTTPS origins, and
keep the public browser hostname consistent with the OAuth callback hostname
through cookie flows. The API does not serve the WASM distribution.

Deploy the complete Trunk browser distribution. The Git-ignored
`labello.client.json` is copied into the distribution when present and may set
the deployment's default `apiBaseUrl`; an absent file uses the fallback.
Replace it after the Trunk build or as part of the atomic deployment rather
than rebuilding the WASM bundle. It is fetched with `no-store` on each page
load, so a reload adopts a replacement. Never place OAuth secrets or other
credentials in it. Configure the static host to return 404 for a missing
runtime file instead of an SPA fallback to `index.html`. Trunk development
serving disables its SPA fallback for the same reason.

Retain logs according to local audit policy while preserving the redaction
rules above. Restrict access to logs because safe identifiers and aggregate
activity still reveal operational metadata.

## Capacity Planning

Measure actual data; configured limits are rejection ceilings, not reserved
capacity.

- **Steady disk:** budget the sum of image bytes, event logs, rebuildable state
  caches, indexes, schema/configuration, keybindings, snapshots, and committed
  import audit records. Event logs are append-only authority and normally grow
  with every workflow mutation.
- **Import disk:** add the staged source, spool, and generated output for every
  concurrently retained import workspace. A conservative upper bound is
  `active import workspaces × import.limits.stagedBytes`; the build concurrency
  limit does not prevent multiple uploaded or awaiting-decision workspaces.
- **Snapshots:** each snapshot duplicates dataset metadata, image indexes,
  import audit records, event logs, and rebuilt states, but excludes images,
  authentication state, and keybindings.
- **Backup disk:** reserve at least one complete additional copy of
  `datasetsRoot`, plus archive overhead and temporary restore-test space.
- **Memory:** add normal server and request overhead to the shared
  `decodedImageMemoryBytes` image-validation pool. That pool must also satisfy
  the cross-field formula in
  [Import Limits](configuration.md#import-limits).
- **Safety headroom:** alert before the filesystem reaches its operational
  reserve. Labello has no built-in free-space threshold or automatic
  backpressure based on remaining disk.

Also bound external log storage. Retention cleanup for import metadata is not
currently scheduled, so include `.labello-server/imports` in growth monitoring.

## Backup And Restore

Labello snapshots are downloadable annotation/audit packages, not restorable
server backups. They omit image bytes, authentication state, and user
keybindings, and there is no snapshot-restore endpoint.

The supported operational backup unit is the complete `datasetsRoot`, including
the top-level `.labello-server` directory and every dataset directory. Back up
the server configuration and externally managed secrets separately. Never put
those secrets into the backup command line, archive name, or logs.

### Create A Consistent Backup

There is no online full-root snapshot coordination. Use a filesystem/storage
snapshot with documented atomic consistency semantics, or use this maintenance
procedure:

1. Stop new user traffic.
2. Confirm no import, ingest, snapshot, or workflow write is active.
3. Send Ctrl-C and wait for `server.stopped`; do not copy merely after
   `server.shutdown.started`.
4. Copy or snapshot the complete dataset root while the server is stopped.
5. Record the Labello version, configuration checksum, backup timestamp, and
   backup-tool verification result outside the archive.
6. Restart the single server process and confirm `/health`, authentication, and
   representative dataset reads.

File-by-file copying while the server is running is not a consistent backup:
an image index, event log, cache, authentication record, or import publication
can change between files.

### Restore

Restore only into an empty destination while the server is stopped:

1. Preserve the failed destination separately for investigation.
2. Restore the complete root, including dot-directories, ownership,
   permissions, and image bytes.
3. Restore the matching configuration and secret injection without copying
   secrets into tracked files.
4. Point exactly one server process at the restored root.
5. Start the server and inspect startup and migration logs before admitting
   traffic.
6. Verify `/health`, login, dataset listing, representative images, event-backed
   state, import manifests, and snapshot listings.
7. Exercise one read-only workflow per dataset role before reopening writes.

Do not overlay a backup on an existing root. Do not combine dataset
directories, `.labello-server` state, or authentication files from different
backup times.

### Reproducible Restore Drill

At each release and on the operator's normal backup cadence:

1. Create a maintenance-mode backup using the procedure above.
2. Restore it to a disposable root owned by a disposable service account.
3. Start the same Labello version with a dedicated loopback bind and copied
   non-production configuration.
4. Perform every restore verification step.
5. Compare dataset/image counts and selected event-log hashes with the source
   backup manifest.
6. Delete the disposable environment through the operator's approved
   recoverable process and record the drill result.

This drill verifies the backup procedure, not snapshot restore, which remains
unsupported.

## Login and session recovery

The browser starts on a dedicated login page while checking sign-in options and
the current session. Authentication controls appear only after discovery
finishes and only for methods enabled by the server. Options-fetch and session
check failures provide separate retry actions; a server with no interactive
method shows an explicit empty state. `About` remains available without a
session. `Advanced connection` contains the API URL override and reconnect
control; ordinary deployments supply that URL through the public browser
configuration.

Changing the endpoint immediately invalidates the account, authentication
options, datasets, imports, and pending request ownership. Use the same hostname
through cookie-based login; `localhost` and `127.0.0.1` have different cookie
hosts. Local administrator login remains loopback-only development behavior.

A current API 401 prompts a session recheck before further work commands run.
A role denial with a valid session returns to the current work and preserves its
error. An expired session shows sign-in recovery and retains the current draft
for its original account. Returning as that account can continue the draft;
signing in as another account clears the previous work from the UI. Browser
drafts remain scoped to server, user, dataset and assignment and retain their
existing expiry and recovery checks. Explicit logout keeps its existing staged
edit and browser-draft policy. Recovery does not grant offline mutation support.

Raw browser upload failures also prompt session revalidation because those
adapters return an error message without a structured HTTP status. Local folder
selection failures do not trigger a session check.

## OAuth cookie-path rollout

The OAuth flow cookie follows the public callback path configured in
[`githubOauth.redirectUri`](configuration.md#github-oauth), including any API
prefix. Labello sets and expires that cookie at the callback's parent path.
The proxy still strips the public prefix when forwarding API requests, and
session-cookie paths remain unchanged.

When upgrading a deployment that rewrites the flow-cookie Path in Caddy:

1. Deploy the fixed server using the normal deployment transaction. Keep the
   existing proxy workaround until the fixed server is running.
2. Remove the production Caddy flow-cookie-path rewrite and validate/reload the
   proxy configuration. Do not replace it with a broader cookie scope.
3. Start a fresh login through the public prefixed URL. In browser developer
   tools, verify that the flow cookie's Path includes that prefix, the callback
   sends it, and login succeeds. The successful callback must expire the flow
   cookie with exactly the same Path; verify that only the session remains.
4. Verify authenticated API access and logout through that public URL. Retain
   only redacted outcomes, never cookie values, OAuth state, codes, or raw URLs
   with query strings in logs or evidence.

Code acceptance covers the regression tests and these instructions. Removing
and verifying the live workaround is an operator's post-deployment step.

## Upgrades And Rollback

The current persistence schema is version 3. The code accepts supported version
2 artifacts and migrates dataset configuration, image indexes, generated
schema, keybindings, and state caches through a durable migration journal.
Event logs remain authoritative and are upcast during replay.

For a manual upgrade outside the supported release workflow:

1. Read release notes and confirm the supported source schema and version hop.
2. Complete and verify a full-root backup.
3. Stop the old server gracefully.
4. Replace the server and separately built WASM assets.
5. Start one server process and allow migrations to complete.
6. Inspect logs, then verify authentication and representative datasets before
   admitting traffic.

Do not run old and new versions concurrently against one root. After a schema
migration, do not point an older binary at the upgraded data unless that exact
reverse compatibility is documented. The safe rollback is to stop the new
binary, restore the pre-upgrade full-root backup into an empty location, and
restart the old binary with its matching configuration and assets.

The supported transaction manager automates this maintenance, backup, matching
configuration generation, readiness, admission, and pre-admission restore
sequence. From its flushed `admission_started` barrier onward, it forbids
automatic data restoration and requires operator-led recovery.

## Corruption And Repair

On malformed JSON/TOML, unsupported schema, hash mismatch, or partial-write
errors:

1. Stop traffic and preserve a full copy of the root and relevant redacted
   logs.
2. Identify whether the damaged artifact is authoritative or rebuildable.
3. A missing, stale, or older supported `state.json` can be rebuilt from the
   image's valid `events.jsonl` on access. Statistics and in-memory caches are
   also derived.
4. Do not edit or truncate `events.jsonl`, `images-index.json`,
   `labello.dataset.toml`, authentication state, import manifests, source audit
   records, or migration journals by hand.
5. If an authoritative artifact is damaged, restore a consistent full-root
   backup or stop for maintainer-led forensic repair. A snapshot is not a
   substitute for the omitted files.
6. After recovery, repeat the restore verification steps before admitting
   writes.

Unknown temporary files or interrupted migration/import directories must not be
deleted solely because their names look stale; recovery may require their
journals or sealed artifacts.

## Derived Preview Cache

The configured preview directory is private (0700 directory, 0600 entries on
Unix), derived, and excluded from dataset backups. Do not serve it directly
through a static host. Every API cache hit still checks authorization and source
identity; delivered pixels cannot be revoked from a client that already saw
them. Normal browser image/prefetch references are cleared on session/endpoint
changes and obsolete replies are ignored.

Cache storage is finite and evicts old entries. A corrupt entry regenerates;
unavailable storage or worker capacity produces a bounded error. Working-image
loads use Data Saver and never automatically fetch a larger representation.
Logs contain only static preview failure categories, never encoded bytes,
source paths, or decoder text.

To discard or relocate previews, stop the server, remove only the configured
cache directory, adjust `previews.cacheRoot` if needed, and restart. Never remove
an active cache lock or edit cache entries while the service runs. A second
owner, unexpected files, or unsafe paths fail closed; fix the directory while
stopped. Derived cache loss never requires restoring authoritative dataset data.

### Working-image Data saver previews

Working views always use the versioned 1280-pixel quality-80 WebP cache profile,
including annotation, review, migration, retries and upcoming-image prefetch.
There is no quality selector or original-detail action. Previous browser quality
preferences are ignored. A failed load exposes Retry image load; it never
transfers Standard, legacy RGBA or original bytes as a fallback.

Original dimensions remain authoritative for annotation coordinates. Superseded
image transfers are cancelled where possible; bytes already transferred still
count against data use. Assignment claims finish independently so unused claims
can be released, and already-started server workers finish within their configured
bounds. Network loss retains the documented draft-recovery limitations and does
not add offline annotation or conflict resolution.

## Dataset Export

Exports use private `.labello-server/exports` storage on Linux and require
one server process per datasets root. Monitor free disk for retained archives
plus each active job's copied originals, metadata, and archive being built.
A conservative bound is retained jobs times `maxArchiveBytes`, plus active
workers times `maxSourceBytes + maxMetadataBytes + maxArchiveBytes`.
Limits bound allocations; they do not reserve disk space.

Cleanup runs on export requests and every 60 seconds. Startup removes expired
jobs and orphan reservations, marks unpublished jobs interrupted, and retains
completed artifacts. Retry an interrupted, failed, or cancelled operation with
a new preflight. Do not hand-edit job state or expose staged files as downloads.
Graceful shutdown requests cancellation of active export workers; restart
recovery reconciles any unfinished cleanup.

Downloaded artifacts contain original images and annotation geometry. Keep
artifacts and private captures out of logs, public web roots, and diagnostic
attachments. Operational export failures use safe bounded categories and job
IDs. Every API route checks current dataset administration access, including
retained downloads. Revocation prevents new streams but does not terminate an
already authorized stream. See [configuration](configuration.md#export-limits)
and [the export contract](export.md).
