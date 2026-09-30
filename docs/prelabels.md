# Model prelabels

The UI uses the [glossary](glossary.md) term **prelabel** for model predictions.
Retained prelabels are cached predictions awaiting confirmation.

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
administrative model controls show a server-configuration notice. Existing model
settings remain stored. Work requiring preparation reports unavailable; annotators
cannot bypass a configured model.

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
The task value accepts plain text or a JSON string, so `detect` and `"detect"`
have the same meaning. Unsupported tasks, malformed values, and contradictory
keypoint metadata remain invalid.
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
selected output name is saved and used by server inference. Each dataset
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

Set the model ID and version, confidence/IoU thresholds, workflow associations,
and availability. Every class in a linked workflow must be
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
CUDA, then native WebGPU, then CPU for a new worker. A warm worker reuses its
successful provider. Provider initialization, model loading, execution failure,
crashes, and timeouts fall through to another provider in a fresh worker.
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

Workers start lazily, one per active dataset/account and one per dataset generation
run, subject to a global cap. Each retains up to two compiled model sessions, keyed
by model content. Subsequent images and workflows reuse the session but apply their
current output selection, class mapping, and processing settings. Model bytes are
sent to the child only on a cache miss. Sessions are volatile execution caches;
retained hints and their validity still follow the durable rules below.

The defaults retain at most four workers, expire idle workers after 120 seconds,
and use one native inference thread per worker. Idle cleanup runs at most every
30 seconds. A new owner can evict the least recently used idle worker immediately.
Batch completion or cancellation releases its worker. The Tract CPU backend remains
single-threaded. Configure these bounds separately from execution concurrency:

```toml
[prelabel.workers]
maxWorkers = 4
idleTimeoutSeconds = 120
threadsPerWorker = 1
```

Each child has a cleared environment, bounded binary input/output, a renewed
300-second CPU budget per request, and no core dumps. CPU and inspection workers
have a 4 GiB address-space limit. GPU workers instead limit their data allocations
to 4 GiB, allowing the large virtual address reservations used by GPU drivers;
CUDA's device memory arena is limited to 1 GiB. Driver/device allocations are not
covered by the host allocation limit. Cancelling or timing out an active request
kills and reaps its child; replacement waits for the old process to exit. Workers
execute a fixed protocol, never a dataset-configured command.

The default global execution concurrency is one, shared with model checks.
Interactive requests take priority between batch items; they do not interrupt
an image already running. At most 64 interactive requests wait for admission,
for up to 30 seconds before returning busy. After admission, `timeoutSeconds`
bounds worker acquisition and all provider attempts together. Resource limits
apply to each process, so size the worker cap for the host's available memory.

Browser inference and model-byte delivery are no longer exposed. Dataset-managed
jobs run detection and pose on the server. Historical `browser_reported` evidence
remains readable and distinct from server execution evidence.

Model bytes are limited to 256 MiB, original images to 32 MiB, decoded dimensions
to 16384 per axis, image decoder allocation to 128 MiB, output to eight million
float32 values, and candidates to 35000. Preparation failures are administrative
state, surfaced as a workflow availability reason.

## Annotation and filtering

The dataset administrator chooses one active compatible configuration per task.
Annotators have no model picker, No prelabels option, generation trigger or saved
model preference. Historical lists resolve to the first available compatible
configuration in their stored order; new or changed bindings accept at most one.

Before claiming work, the server prepares the task's focusable sources. Nonempty
predictions enter Objects as individually leased items; empty results use normal
Overview annotation. A skeleton is one item. The UI displays only the leased
prediction and zooms to it. Confirm & next keeps its geometry and advances to the
next Objects item across image boundaries. Delete is a pending decision for both
boxes and skeletons; confirm it explicitly before advancing. Undo can restore it.
Autosave and Skip preserve partial geometry without accepting the prediction.

Overview is a separate image queue, available only after all Objects work is
finished. It handles missing annotations directly. Objects never automatically
ends in Overview. Fit and Refocus remain available without changing the queue.

Preparation applies confidence and overlap filtering against the current image
and task. Existing nondeleted boxes win. Remaining hints use descending confidence
with stable suggestion-ID ties; same-class/task IoU greater than the configured
threshold suppresses a hint. The default hint threshold is 0.5. Once displayed,
an item remains available for an explicit decision even if another edit changes
overlap visibility. Its identity and signed original geometry stay fixed.

Displaying an item records a durable seen event. Config/model changes and resets
replace unseen unused predictions, including prefetched or claimed items never
displayed. Previously displayed objects retain their original evidence across
Skip, partial saves, history and restart. An already displayed Overview rejects
late predictions. Completed work never reopens merely because generation finishes.

## Dataset generation and removal

The server discovers configured workflows at startup and on a five-second
maintenance interval. Administrative configuration paths also synchronize
preparation. Availability and claim requests do not wait for dataset-wide
preparation; configured workflows show pending preparation until maintenance
publishes their sources. Model-free workflows prepare sources in the claim
transaction. Durable bounded jobs cover both boxes and poses;
large datasets continue in subsequent jobs. Work is revalidated before execution
and publication against image, task/configuration, model and reset identities.
Obsolete results cannot publish into unused work. Compatible retained results are
reused without rerunning inference.

**Admin > Automation > Dataset prelabels** shows job state and administrative
preflight, start, retry, cancellation and removal controls. Explicit preflight
continues to support box batches. Automatic workflow preparation also supports
poses. Pending and failed preparation are visible as unavailable reasons to users.
Jobs continue after the browser disconnects. Interrupted or cancelled work remains
administrative retry work; polling does not silently retry a failed generation.
Cancellation preserves completed results and marks unfinished managed items failed.

**Remove prelabels and pause** advances durable generation markers, cancels
matching jobs and removes unused retained results. It preserves displayed source
predictions, partial work, saved annotations and provenance. An older displayed
prediction remains saveable using its captured evidence. A paused scope uses normal
image annotation for untouched work. **Resume prelabels in this scope** enables
new preparation. Late results cannot interrupt a displayed Overview.

Private state lives below `.labello-server/prelabels/<dataset-id>/`.
`control.json` holds the signing secret, durable generations, pause scopes, run
progress, and result index. Result files are derived hints. Publication writes
the complete result before durably indexing it; recovery removes orphan results.
Reset commits invalidation before deleting files. Preserve control state in full
backups and use the API for removal rather than hand-editing private files.

The default retention is seven days, with 16 runs, 100000 captured work items,
100000 result files, 8 MiB per result, and 2 GiB of indexed results per dataset.
Administrative access/preflight prunes expired results and runs; preparation
does not reuse expired unused results. Limits are configurable in
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
New prelabel acceptance requires signed evidence in an online annotation batch. Historical
`prelabel_suggestion` revisions remain readable but cannot be newly authored by
ordinary event or offline synchronization requests.

External prediction-file import is outside this feature's scope.
