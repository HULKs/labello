# Labello

<img src="assets/labello-icon.svg" alt="Labello icon" width="96" />

Labello is a browser-based image annotation system for bounding boxes and
skeletons. It combines a Rust/egui WebAssembly client with an Axum API and stores
images, annotations, reviews, and audit history on the filesystem.

[Documentation](docs/README.md) · [GitHub Wiki](https://github.com/HULKs/labello/wiki) ·
[Contributing](CONTRIBUTING.md) · [Issues](https://github.com/HULKs/labello/issues)

## What it does

- Annotate boxes and keypoints with autosave, undo/redo, configurable shortcuts,
  prepared image queues, and account-scoped browser draft recovery.
- Review each object and the full image. Reviewers can correct geometry or add
  missing objects; one reviewer approves the resulting round to complete it.
- Browse datasets without claiming work, inspect overlays, and return completed
  workflows to review with an audited reason.
- Manage classes, workflows, instructions, roles, images, and assignment balance.
  Copy an existing dataset's schema when creating another dataset.
- Import explicit YOLO/COCO ground-truth profiles into new datasets. Export
  complete detection or pose datasets with original images and documented
  [round-trip guarantees](docs/export.md).
- Convert imported boxes into skeletons through guided migration, with recorded
  exclusions and linked boxes for newly discovered objects.
- Track task completion, contributor activity, scores, streaks, and leaderboards. Download
  annotation snapshots and retain replayable event history.

## Run locally

Install [Rustup](https://rustup.rs/), then run these commands from the checkout.
The repository pins Rust 1.98.0, its components, and the WASM target.

```sh
rustup show
cargo install --locked trunk --version 0.21.14
cargo run --locked -p labello-server
```

The first server start creates `labello.server.toml` and listens on
`127.0.0.1:8080`. In another terminal:

```sh
cd apps/labello-wasm
trunk serve --locked --address 127.0.0.1 --port 8081
```

Open <http://127.0.0.1:8081> and choose **Continue as local admin**. Create a
dataset, configure its classes and workflows, add images, and start annotating.
Local admin login is for loopback development only.

See [getting started](docs/getting-started.md) for the first dataset, login,
connection setup, and troubleshooting. For a hosted installation, use the
[configuration](docs/configuration.md), [deployment](docs/deployment.md), and
[operations](docs/operations.md) guides. The API does not serve the browser build.

## Documentation

| Task | Guide |
| --- | --- |
| Label and review images | [Annotation and review](docs/annotation.md) |
| Configure datasets and inspect results | [Dataset administration](docs/administration.md) |
| Convert existing ground truth | [Import](docs/import.md), [guided migration](docs/migration.md), [export](docs/export.md) |
| Understand work allocation and scores | [Assignment](docs/assignment.md), [scoring](docs/scoring.md) |
| Run and recover a server | [Configuration](docs/configuration.md), [operations](docs/operations.md), [persistence](docs/persistence.md) |
| Change the implementation | [Architecture](docs/architecture.md), [API](docs/api.md), [verification](docs/verification.md) |

The [documentation index](docs/README.md) covers the full suite. Current pages
are the source for GitHub Wiki. Plans, feature drafts, the target product
specification, and historical records are retained in [docs/archive](docs/archive/README.md)
and excluded from wiki publication.

## Development

The workspace separates domain policy, filesystem storage, client transport,
HTTP handlers, shared UI, and executable apps. The standalone
[native inspector](apps/egui-mcp-inspector/README.md) supports UI development;
Chromium is required to verify browser behavior.

Run changed-path verification from the repository root:

```sh
./scripts/verify.sh changed origin/main
```

It selects documentation checks or the locked Rust baseline, plus the release
browser build when applicable. [Contributing](CONTRIBUTING.md) describes the
review workflow and [verification](docs/verification.md) defines additional
checks. Build the browser distribution with `trunk build --release --locked`
from `apps/labello-wasm`.

## Current limitations

Labello is under active development. These boundaries matter when choosing it:

- Browser drafts are best-effort recovery. Offline annotation and conflict
  resolution are unavailable; there is no supported native desktop client.
- Independent multi-annotator labeling is unavailable.
- [Prelabels](docs/prelabels.md) support the documented static Ultralytics YOLO
  ONNX contract, with model inspection, named outputs and explicit class mappings.
  Server inference tries CUDA/WebGPU when native providers are installed, with CPU
  fallback. Server generation requires Linux and an operator-managed model
  directory. Dataset-wide generation covers box workflows; external prediction
  import and dataset-wide pose generation are unavailable.
- Tutorials render text only. Review has no swipe controls. Pen events have
  focused Chromium/WebKit coverage, but [iPadOS Safari/Firefox and physical stylus
  devices](docs/stylus-input.md) still need device validation. Named
  screen-reader/browser combinations have no verified support claim.
- Import creates new datasets and accepts only the documented ground-truth
  profiles. It does not merge datasets, import segmentation or predictions, or
  fetch archives or remote sources.
- Dataset configuration and keybindings use TOML. Schema version 3 is current;
  version 2 is the only supported legacy version.
- Snapshots omit images, authentication, keybindings, and private job state.
  They have no native restore operation. Back up the complete data root.
- One server process must own each data root. Assignment balance compares
  enabled tasks, with no separate class-level aggregation. Large-scale imports
  require capacity validation, and retained import cleanup is not scheduled.

See [all current limitations](docs/limitations.md) for operational, browser,
compatibility, and recovery details.
