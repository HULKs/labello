# Model prelabels

Prelabels are editable suggestions. They do not claim work, complete a workflow,
or enter ground-truth export until an annotator accepts them and completes the
normal annotation and review workflow.

## Supply a model

The tensor contract supports Ultralytics YOLO detection and pose ONNX exports,
plus other exports with the same preprocessing and output semantics below.
Export one static float32 input with shape `[1, 3, S, S]` and raw output with
shape `[1, 4 + classes + 3 * keypoints, candidates]`. Detection has no keypoints.
Use `batch=1`, `dynamic=False`, `half=False`, `nms=False`, and an input size
between 32 and 1280 divisible by 32. YOLO11 detection and pose exports with
opset 17 and input size 320 have been exercised. End-to-end NMS exports,
segmentation, dynamic shapes, external tensor files, and other output layouts
are unsupported.

For example, in an environment with Ultralytics installed:

```python
from ultralytics import YOLO

YOLO("yolo11n.pt").export(
    format="onnx", imgsz=320, batch=1, dynamic=False,
    half=False, nms=False, opset=17, simplify=False, device="cpu",
)
```

Use a pose model such as `yolo11n-pose.pt` for a skeleton workflow. Python is an
export tool, not a production inference dependency.

The server operator places the ONNX file in a dedicated directory readable by
the server account and enables the optional server section:

```toml
[prelabel]
modelsRoot = "/srv/labello-models"
timeoutSeconds = 120
```

Add this section to `labello.server.toml` or the file selected by `LABELLO_CONFIG`.
Create the model directory before starting the server. Restart the server and
reload the web app after changing this configuration. Without the section,
prelabel controls are replaced by a server-configuration notice and no hint
requests or management polling run. Existing model settings remain stored.

There is no model upload endpoint. Dataset administrators select a managed
basename such as `people.onnx` in **Admin > Automation**. Paths, symlinks, URLs,
and executable commands are rejected. Existing configurations without a YOLO
profile remain readable but cannot run until configured. The historical
server `command` field must be empty.

Enter the model filename and select **Check model**. The server checks the
managed file before a configuration or class mapping has been saved. Inspection
runs in a bounded worker without an image and reports the static input dimensions,
output tensor names, data types and shapes, class count, and available class names.
Task metadata is optional. Without `task` or `kpt_shape`, inspection treats the
output as detection: four box-coordinate channels followed by class scores.
`kpt_shape = [keypoints, 3]` identifies pose and remains required for pose outputs;
an explicit `task` must be `detect` or `pose` and agree with the keypoint metadata.
Class names are optional and may use either `names` or `classes`, as a dictionary
of contiguous IDs starting at zero. If both keys are present, their mappings must
agree. Class metadata is cross-checked against output dimensions; malformed or
inconsistent metadata and unsupported layouts produce an explanation. Numeric
output IDs remain authoritative.

Input and output dimensions must still be declared statically in the ONNX graph.
Symbolic dimensions are rejected even when a particular inference would resolve
them to fixed values. For example, a seven-class detector with 300 candidates
must declare `[1, 11, 300]`. The exporter must also ensure the documented image
preprocessing, box-coordinate units and score semantics; shape inspection alone
cannot establish these meanings.

Select the **Output tensor** from the discovered outputs. A sole compatible output
is selected automatically; unsupported outputs are shown with their reason. The
selected output name is saved and used by both execution environments. Each dataset
class has a selector for model class IDs `0` through `classCount - 1`. For example,
map `person` to `0` and `ball` to `32` in the standard 80-class COCO detector.
Unmapped outputs produce no hints. Multiple model outputs may map to the same
dataset class, but a model output ID cannot map to conflicting dataset classes.
The total model class count stays independent of the selected mappings.

Changing the filename invalidates the check. Changing the selected tensor
revalidates mappings; invalid saved mappings remain visible until removed.
The checked model's BLAKE3 digest pins the profile. Replacing the file requires
checking the model and saving its configuration again before generation can resume.
Historical positional `classIds` arrays remain readable and keep their exact
meaning; checking them converts their mappings to explicit `classMappings` entries
with `modelClassId` and `classId`. Configuration changes use normal staged Admin save.

Set the model ID and version, execution mode, confidence/IoU thresholds, task
associations, and annotator availability. Every class in a linked workflow must be
mapped. For pose, keypoint names remain in model order and must exactly match the
workflow's ordered skeleton specification. Detection models link to box workflows;
pose models link to skeleton workflows.

## Execution and coordinates

The shared Rust adapter decodes the original image to RGB, fits it into the
square input with bilinear resizing, centers it with RGB 114 padding, and
normalizes channels to `[0, 1]` in NCHW order. It chooses the highest-scoring
model class per candidate and reverses the actual resize and padding to return
clipped normalized coordinates in the original image. It rejects nonfinite
outputs, invalid scores, negative dimensions, and unsupported tensor layouts.
Empty or fully clipped boxes produce no hint.

Pose uses its object box for confidence-ordered IoU suppression, then returns
the named keypoints. A keypoint score below 0.5 becomes hidden when the workflow
allows hidden points; otherwise it remains visible for human correction.

Server inference runs on Linux through Rust `ort`. It tries native ONNX Runtime
CUDA, then native WebGPU, then CPU. Provider initialization, model loading, execution
failure, crashes, and timeouts fall through to the next provider in a fresh worker.
The configured timeout is shared by the whole request; each GPU attempt gets at
most one quarter of it, reserving time for CPU fallback. Native providers may use
CPU kernels for unsupported operators. Provenance records the successful provider
as `server_cuda`, `server_web_gpu`, or `server_cpu`.

Native ONNX Runtime is loaded from the system library search path, or from an
operator-configured absolute path. A separate WebGPU plugin is optional when
WebGPU is already built into that runtime:

```toml
[prelabel.runtime]
onnxLibrary = "/srv/labello-runtime/libonnxruntime.so"
webgpuLibrary = "/srv/labello-runtime/libonnxruntime_providers_webgpu.so"
```

The runtime must provide ONNX Runtime API 24 or later. CUDA also needs compatible
NVIDIA drivers and the runtime's CUDA/cuDNN dependencies. Native WebGPU needs a
compatible provider and Vulkan drivers on Linux. Place dependent shared libraries
beside `onnxLibrary` or install them in the system loader paths. These paths are
operator configuration, never dataset-admin input. If native ONNX Runtime cannot
load, the included Tract runtime retains CPU inference without an external
runtime installation. No production inference path invokes Python.

Each attempt gets a fresh child process with a cleared environment, bounded binary
input/output, a 300-second CPU limit, and no core dumps. CPU and inspection workers
have a 4 GiB address-space limit. GPU workers instead limit their data allocations
to 4 GiB, allowing the large virtual address reservations used by GPU drivers;
CUDA's device memory arena is limited to 1 GiB. Driver/device allocations are not
covered by the host allocation limit. Dropping or timing out an attempt kills and
reaps its child. Workers execute a fixed protocol, never a dataset-configured
command. The default global concurrency is one, shared with model checks.

Browser inference uses self-hosted ONNX Runtime Web in a dedicated worker.
WebGPU-preferred configurations try WebGPU, then retry on WASM CPU if execution
fails. CPU-only configurations use WASM directly. Each attempt times out after
120 seconds; cancelling the request terminates its worker. Shared Rust code
performs preprocessing and output decoding. Model and image downloads use the
authenticated API and are checked against the server's BLAKE3 identities.
Models selected for browser execution are therefore disclosed to authorized
dataset users. Browser results are explicitly recorded as `browser_reported`;
the server cannot attest that an untrusted browser ran a particular model.

Model bytes are limited to 256 MiB, original images to 32 MiB, decoded dimensions
to 16384 per axis, image decoder allocation to 128 MiB, output to eight million
float32 values, and candidates to 35000. Unsupported models, exhausted workers,
timeouts, and download failures leave manual annotation available.

## Annotator controls and filtering

The Prelabels selector offers compatible, available configurations and
**No prelabels**. The initial choice is the first available configuration.
An explicit selection, including none, persists per account, API origin,
dataset, and workflow. A removed or unavailable saved selection becomes none.
Hints load independently of the image and are prefetched for prepared images.
Changing selection cancels obsolete work and clears queued hints while keeping
annotation drafts. **Refresh hints** retries a failed request.

Before display, the shared filtering policy combines the current candidate set
and compares it with current persisted and draft boxes. Existing nondeleted
boxes win. Remaining hints are ordered by descending confidence with stable
suggestion-ID ties. A hint is suppressed only when IoU is **greater than** the
configured threshold against a kept hint or existing box in the same image,
class, and workflow. The default threshold is **0.5**. Different classes and
workflows may overlap. Editing or deleting a draft box immediately refilters
the retained candidates. Changing model or processing configuration invalidates
the generation identity and requires refreshed hints.

## Dataset generation and removal

In **Admin > Automation > Dataset hints**, check remaining box workflows,
choose a compatible server model for each ambiguous workflow, then start the
prepared run. A workflow with one compatible server model is mapped
automatically. Missing mappings or unavailable files block start. Preflight
captures missing/Pending, InProgress, and NeedsCorrection image/workflow pairs.
Disabled, submitted, completed, and import-excluded work is omitted.
Preflight shows eligible pairs, matching reusable results, and ineligible pairs.
Start reuses compatible retained results and generates the remaining items.
If new eligible work appears before start, check remaining workflows again to
include it. Work added after start belongs to a later run.

Runs continue after the browser disconnects. The UI reports pending, generated,
empty, skipped, and failed item counts. Each item is revalidated before execution
and publication; changed or completed work is skipped. Results bind the image
hash, task/configuration digest, model digest, and reset generations. Interactive
requests for that exact binding reuse retained results without inference.

Cancellation preserves successful results and pending work. Retry processes
failed and pending items; it preserves successful results unless a reset removed
them. A server restart exposes a running job as interrupted on the next dataset
prelabel access. It requires explicit retry. Configuration/model changes require
a fresh preflight. There is one active batch per dataset, sharing the global
worker limit with interactive requests.

Removal can target a workflow, a configuration, their intersection, or the whole
dataset. Confirm **Remove hints and pause** to remove retained results, cancel
affected runs, and advance durable generation markers. Repeating a removal is
idempotent. Affected browser caches and in-flight results cannot authorize a new
acceptance. Clients check generation status while annotating. Annotation history,
accepted annotations, user edits, drafts, and model configuration remain intact.
Unsaved acceptances from an old generation fail on save and require refreshed
hints. **Resume hints in this scope**, or explicitly starting/retrying a run,
reenables generation. Ordinary polling cannot resume it.

Private state lives below `.labello-server/prelabels/<dataset-id>/`.
`control.json` holds the signing secret, durable generations, pause scopes, run
progress, and result index. Result files are derived hints. Publication writes
the complete result before durably indexing it; recovery removes orphan results.
Reset commits invalidation before deleting files. Preserve control state in full
backups and use the API for removal rather than hand-editing private files.

The default retention is seven days, with 16 runs, 100000 captured work items,
100000 result files, 8 MiB per result, and 2 GiB of indexed results per dataset.
Administrative access/preflight prunes expired results and runs; interactive
requests do not reuse expired results. Limits are configurable in
`[prelabel.limits]`, documented in the server example. Quota or I/O interruption
leaves pending work retryable after the operator resolves the limit or storage
failure. Model storage is operator-managed and outside these result quotas.

## Acceptance and history

The API signs exact prediction evidence. Acceptance verifies dataset, image,
workflow, configuration/model digests, generation, and signature while holding
the dataset prelabel guard through the annotation transaction. Under the image
lock, replay checks confidence, current-box suppression, and duplicate acceptance.
An exact committed retry remains safe after reset; it creates no second annotation.

Accepted annotations use immutable `prelabel` origin with the original prediction,
model ID/version/digest, configuration digest, processing settings, confidence,
execution mode, trust classification, and suggestion identity. The revision is
human accepted-unchanged or human edited. Later human/reviewer edits preserve
the original prediction. They follow normal submission, review, completion,
statistics, and export rules. Unaccepted hints never become ground truth.

Schema version 3 events, states, generated schemas, snapshots, and offline bundles
retain accepted provenance. Historical version-2 and version-3 annotations remain
readable; version-2 output rejects new prelabel origins instead of dropping them.
New prelabel acceptance requires the online evidence endpoint. Historical
`prelabel_suggestion` revisions remain readable but cannot be newly authored by
ordinary event or offline synchronization requests.

Dataset-wide pose generation and external prediction-file import are outside
this feature's scope.
