# Browser verification

> **Status:** Normative current reference
> **Owner:** UI maintainers
> **Audience:** Contributors and reviewers
> **Last verified:** 2026-09-05 during issue #42 verification

## Run the gate

The canonical entry point is `./scripts/verify.sh changed <comparison-base>`.
Browser-affecting changes select the release Trunk build and Chromium gate;
`./scripts/verify.sh all` selects them unconditionally. CI runs the same browser
stage through `./scripts/verify.sh ci <comparison-base> browser`. A failed
selected browser job fails the required `Testing` check.

The compiler and Trunk prerequisites are in [verification](verification.md).
Browser automation uses Node 24.19.0 and the tracked npm lockfile, which pins
Playwright 1.63.0, its Chromium revision, pixelmatch 7.1.0, and pngjs 7.0.0.
Install Chromium's system libraries once after npm installation:

```sh
npm --prefix scripts/browser ci
./scripts/browser/node_modules/.bin/playwright install-deps chromium
```

The canonical browser stage builds the server and `browser_fixture` example,
tests the fixture, installs the locked npm dependencies, runs support tests,
installs the pinned browser, and invokes `node scripts/browser/run.mjs`.
After the canonical build, a focused development run is:

```sh
node scripts/browser/run.mjs --workflow=migration
```

Supported selectors are `annotation`, `review`, `migration`, `admin`, `display`,
`zoom`, `recovery`, and `skeletons`. Focused success does not replace the full
selected gate. Do not run against a user server, override a fixture with live
data, or reuse a browser profile. Parallel worktrees have separate generated
assets, targets, artifact directories, and automatically selected loopback ports.

## Isolation, diagnostics, and failures

Each workflow creates a new private temporary directory. The storage example
refuses an existing destination, builds four repository-owned procedural images
and a closed task configuration, and writes `labello-browser-v1` provenance.
The gate launches its own production server and serves its own release WASM
assets with an exact credentialed cross-origin configuration. The local
administrator login is loopback-only test behavior.

Server and WASM readiness have 15-second deadlines. Each workflow has a
120-second deadline. Inputs cross separate animation frames so egui receives
press and release events. Assertions wait for explicit durable outcomes; they
do not retry whole workflows until one passes. Finally blocks close owned
browser contexts, terminate the owned server, stop the static server, remove
only the owned temporary directory, and report cleanup outcomes. Compilation
and distribution outputs remain ordinary ignored build outputs.

Console errors, page errors, failed requests, and HTTP failures become bounded
categories and counts. Expected failures must be declared with exact route and
status counts and consumed. Unexpected or missing expected errors fail the run.
No raw browser/server logs, request bodies, headers, or response dumps are kept.

The full gate deliberately runs these rejected cases after positive workflows:

```sh
node scripts/browser/run.mjs --inject=startup
node scripts/browser/run.mjs --inject=network
node scripts/browser/run.mjs --inject=visual
```

Each command must exit nonzero. The parent validates its bounded failure category
and successful cleanup. Startup aborts the WASM asset; network aborts local
login; visual mutates an in-memory copy of a fixture screenshot. They cannot
update baselines. These checks prove failure detection, not just happy paths.

## Display and workflow coverage

[`matrix.json`](../scripts/browser/matrix.json) is the shared size authority.
Deterministic tests and inspector `--display` read it too. Chromium launches
with matching native and context device scale factors because egui sizes the
canvas from `ResizeObserver.devicePixelContentBoxSize`. Context DPR emulation
alone does not faithfully exercise that path.

| Workflow/state | Chromium coverage | Complementary evidence |
| --- | --- | --- |
| Login, dataset discovery, task entry | Fresh authenticated entry for every workflow | API cookie/auth tests; native navigation and focus |
| Box create, edit, save, submit, skip, keybindings and reload draft recovery | 1440×1000; annotation/settings at 390×844 | Shared UI request, persistence, keyboard and layout tests |
| Ordinary object approval, full-image approval, reviewer correction | 1440×1000 and 390×844 review states | Domain review policy; deterministic correction and focus tests |
| Guided skeleton object save, final confirmation and submission | 1440×1000 and 390×844 migration states | Domain/storage migration policy; native object/full-image presets |
| Normal skeleton gestures, keypoint outcomes and edits | 1440×1000 | Deterministic skeleton and save-boundary regressions |
| Admin task-name save and reload | 1440×1000 | Admin owner and validation tests |
| Delayed image load, injected failure, retry, stale admin response | 1440×1000 | Epoch/request ownership tests |
| Workspace and settings overlay | All six sizes at DPR 1/2, 390×844 at DPR 3 | Actual clip/target/overflow assertions and native display controls |
| Real 200% browser zoom and submission | 1440×1000 browser viewport, effective 720×500 layout | Extension verifies `chrome.tabs.getZoom()` equals 2 |

The six sizes are 320×568, 390×844, 600×800, 1288×820, 1440×1000 and 320×320.
The DPR 3 case starts and signs in at its required 390×844 size. An exploratory
1288×820 DPR 3 bootstrap intermittently missed the initial login interaction;
that wider DPR 3 case is outside this matrix and remains unverified. The gate
does not retry failed workflows or reuse authenticated state across cases.
Matrix runs check actual canvas backing dimensions and painted content, open
settings by keyboard, exercise scrolling at short sizes, and dismiss with
Escape. Pixel density is not browser zoom. The zoom workflow uses a local
extension to call Chrome's zoom API on the actual app tab.

Chromium font preferences are controllable and are set/read back to default
24 and minimum 20. **Unsupported:** those preferences do not enlarge egui's
canvas text. Use native `--scale` inspection for shared logical scaling and
record the browser limitation. Browser accessibility inspection retains only
role counts. **Unsupported:** the current eframe web adapter discards widget
AccessKit updates, so Chromium exposes the canvas rather than named application
widgets. Deterministic/native AccessKit evidence does not imply browser screen
reader support. Offline annotation, stylus support, and Adjudication are excluded.

## Visual artifacts and review

The narrowly scoped synthetic exception in [operations](operations.md#synthetic-ui-verification-artifacts)
and [verification](verification.md#synthetic-ui-verification-artifacts) applies.
The capture function admits only its own fixture objects, exact app origin, and
allowlisted states. Support tests exercise rejection before screenshot access.
The canonical build records a source digest and WASM digest in the ignored
`target/browser-build.json`. Runs reject a missing or stale stamp. After a manual
Trunk rebuild, run `node scripts/browser/build.mjs` to record that build before
a focused check. The report identifies the build revision and digests, dirty
state, workflow/state, viewport/DPR,
zoom where relevant, safe diagnostics, and cleanup. CI uploads only the explicit
PNG/report allowlist for 14 days. It never uploads profiles, runtime datasets,
traces, network archives, or arbitrary directories.

Tracked baselines cover annotation, settings, review, and migration at wide and
compact sizes. These target paint-sensitive layouts; the wider display matrix
uses structural/paint checks and review artifacts instead of fragile snapshots
for every pixel density. Pixelmatch uses a 0.15 per-pixel threshold and rejects
more than 0.3% differing pixels or any dimension mismatch. UI maintainers own
baseline review. To prepare an intentional update:

```sh
node scripts/browser/run.mjs --update
```

CI rejects baseline updates. Inspect each changed image and the production diff;
do not refresh a baseline merely to make a failing comparison green. Record
before/after evidence for reproducible defects and inspect an adjacent breakpoint.
Fixture-only changes must be described separately from product fixes.

Before initial handoff, retain three consecutive full runs on the same final
revision with fresh fixtures. A source change requires new evidence. This is
bounded reproducibility evidence and does not establish an absence of flakes.
Independent review still owns the final visual judgment.

## Inspector and workflow parity

The inspector shares `LabelloApp` rendering but constructs synthetic preset
state. It is not a supported native client. Compare `annotation`, `review`,
`migration-object`, `migration-full-image`, and `dialog-settings` with their
live browser counterparts. Native presets intentionally use different sample
labels, image patterns, queue counts, and identities; inspect layout and
interaction semantics rather than requiring pixel identity. Native `--scale`
changes egui logical zoom. Its screenshot density parameter only changes output
resolution. Follow the [inspector guide](../apps/egui-mcp-inspector/README.md).

The comparison below concerns current behavior, not full product parity:

| Feature | Classification and evidence/limit |
| --- | --- |
| Pan, zoom and fit | Verified equivalent shared canvas owner; both browser skeleton workflows exercise gestures and return to fitted paint |
| Keypoint visible, occluded and absent outcomes | Verified equivalent outcome policy, browser durable state and deterministic tests |
| Keypoint selection/editing | Supported through shared pointer tools; deterministic tests cover selection and drag, browser normal workflow edits a saved point and migration edits its guided draft |
| Task/class context and image overlays | Intentionally different: migration shows a canonical guide and coverage context, ordinary annotation shows editable workflow objects |
| Object sequencing and editing | Intentionally different: migration resolves a frozen canonical sequence and saves each object; ordinary annotation edits the image's object set |
| Shortcuts | Shared configurable action map; normal save/submit and modal operations, migration uses a configured Enter binding for save/confirm and keyboard point outcomes; migration-specific restrictions remain explicit |
| Draft preservation | Shared requirement for modal dismissal and save/failure boundaries, with deterministic and live browser coverage; normal browser draft reload is exercised |
| Migration browser reload recovery | Identified product gap: the ordinary IndexedDB draft owner does not persist migration drafts; no reload-recovery claim for migration |
| Save, submit and skip/exclude | Intentionally different: normal image save/submit/release versus migration object save/exclusion followed by full-image confirmation |
| Panels, overflow and responsive primary actions | Shared layout owner and executable matrix; compact normal/review/migration artifacts and deterministic reachability tests |
| Focus and accessibility | Shared native focus/AccessKit contract; browser keyboard and modal checks apply, but browser widget accessibility is unsupported in both |
| Uncertainty | Identified product gap, planned by #59; not included in the initial parity claim |
| Offline annotation and Adjudication | Unsupported in both production workflows |

Defects in the initial shared contracts block handoff. Broader product gaps in
this inventory remain unresolved and must not be described as full parity.
