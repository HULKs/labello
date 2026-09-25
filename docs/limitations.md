# Current limitations

These boundaries describe current behavior. See [the documentation index](README.md)
for the supported workflows.

## Product and workflow

- Session recovery retains account-scoped drafts but does not enable offline
  work. Browser recovery remains a best-effort local cache; reloading or an OAuth
  round trip relies on the existing persisted draft and assignment checks.

- Offline bundle and synchronization APIs exist, but the browser UI cannot
  download an offline workspace, author against it without a network
  connection, retain versioned offline mutations, synchronize them, or
  present merge conflicts. Browser draft recovery is not offline mode.
- Independent multi-annotator labeling and agreement calculation are not operational.
- Prelabel configuration, task association, queued loading, display,
  acceptance, and discard controls exist, but annotators cannot choose among
  the available configurations: every configuration associated with the task
  is requested. No model is executed. The server returns fixed placeholder
  geometry; browser-local WebGPU and CPU/WASM fallback execution are not
  implemented. Accepted placeholders currently record a generic model identity
  rather than the configured model's exact identity.
- Task tutorials display configured title and text only. Administrators can
  enter example-image paths, but those images are not loaded or shown to
  annotators.
- Approval review supports object decisions through buttons and configurable
  shortcuts plus a final full-image check. Swipe-to-approve or reject is not
  implemented.
- The browser normalizes primary pen events and suppresses compatibility-event
  duplication. [Stylus validation](stylus-input.md) covers Chromium emulation and
  scripted WebKit input; iPadOS Safari/Firefox, physical Linux tablets, and
  Android styluses remain unverified. Pressure, tilt tools, erasers, and
  device-level palm rejection are unsupported.
- Assignment balance can enforce an absolute completion-count window across
  enabled tasks. It does not separately aggregate and enforce class-level
  balance when multiple tasks share a class.
  See [Assignment](assignment.md#completion-balance) for exact count and
  boundary semantics.
- There is no supported native desktop client. The native inspector is a
  development tool, not an offline or production client.

## Persistence and compatibility

- Current dataset configuration and keybindings are versioned TOML, while image
  indexes, state, events, schemas, snapshots, and import records use JSON or
  JSONL.
- Persisted schema version 3 is current and version 2 is the only supported
  legacy version. Version 1 artifacts are rejected; no `1 -> 2` migration is available.
- Snapshots are downloadable annotation/audit packages, not complete backups.
  They omit image bytes, authentication state, user keybindings, and private
  import/export control state, and there is no native snapshot-restore operation.

## Operations

- There is no general browser end-to-end test suite. The focused stylus check
  exercises production WASM/API input with disposable data. `egui_kittest` and
  the native inspector do not validate WASM networking, cookies, IndexedDB,
  browser input, or deployed responsive behavior.
- Ingest jobs and some derived caches are process-local and do not survive
  restarts as durable jobs.
- Configured cleanup of retained import jobs is not invoked or scheduled by the
  production server, and import API control/idempotency records have no complete
  retention lifecycle.
- `GET /health` is liveness only. `GET /deployment/readiness` checks
  dataset-root traversal and authentication-store loading for deployment
  admission, but it does not cover write capacity, free space, representative
  dataset reads, OAuth, or browser networking.
- Graceful shutdown is wired to Ctrl-C, but there is no application drain
  deadline or documented SIGTERM handler.
- Import format support is tested under configured limits, but official
  COCO-scale operation remains a separate performance gate.
- Import does not merge into existing datasets and does not support prediction
  or prelabel import, segmentation, remote sources, or archive sources.
  [Detection and pose export](export.md) supports explicit ground-truth
  round trips into new datasets; it does not restore native identities or history.
- Import publication, assignment locking, and in-memory caches assume one
  Labello server process per datasets root. Multi-process coordination is not
  supported, including on a shared network filesystem.

AccessKit and Chromium accessibility checks do not establish certification for a
named screen-reader/browser combination. Inspector return-to-review reasons are
audited but are not yet integrated into saved-reason notices or notifications.
