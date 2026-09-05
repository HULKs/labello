# Dataset export

> **Status:** Normative current reference
> **Owner:** Storage and API maintainers
> **Audience:** Dataset administrators, operators, and contributors
> **Last verified:** Backend and production reader checked 2026-09-05; full UI verification pending

Dataset administrators can export authoritative ground truth as
`ultralytics_yolo_detect_v1` or `ultralytics_yolo_pose_v1`. Each export is a
private, retained job with preflight, explicit start, cancellation, status,
and a streamed ZIP download. Export does not modify the source dataset.

## Selection and completeness

Select task/class identities compatible with the profile and an explicit
fallback split. Class indices follow stable task/class ID order. Distinct
class IDs with equal display names remain separate; selecting one class ID
through multiple tasks blocks the export. Pose classes must have equal,
nonempty keypoint counts and unique names within each class. Per-class names
are preserved even when their order or spelling differs between classes.

An image is included only when every selected task establishes a complete,
current label set. Pending, submitted, rejected, incomplete, excluded, stale
review-policy, unresolved migration, and unverified suggestion states are
omitted with a reason. Effective committed review decisions count; a staged
revision does not. Deleted annotations are omitted. Known objects belonging
to a selected task but missing from the selected class mappings block the
artifact instead of silently producing an incomplete label set.

An empty label file is emitted only for a complete image with no selected
objects. It contains one newline so ordinary browser-folder import can upload
it. Preflight reports inclusion, empty-image and object counts, omission
counts, and up to 100 omission and blocker examples. The manifest records all
omissions. A blocked or wholly omitted selection cannot start.

A single recognized `train`, `val`, or `test` membership is preserved. Images
without such membership use the selected fallback, initially `train` in the
UI. Conflicting recognized memberships require an explicit per-image choice.
There is no random split or silent reassignment of an unambiguous split.

## Geometry and files

Detection rows contain class index and normalized box center, width, and
height. Pose rows add normalized keypoint coordinates and visibility values:
Visible is `2`, Occluded is `1`, and Not present is `0` with zero coordinates.
Numbers use nine decimal places. Labels that collapse to duplicate float32
rows block the export because the target reader would drop objects.

A pose uses its current valid linked box when available. An effective rejection
of that exact box version makes it unusable; a superseded rejection does not.
An unselected box task need not establish complete coverage for other objects.
Otherwise the pose's placed
keypoints define a clipped envelope with at least one original pixel of width
and height. Derived bounds are recorded in the manifest. An all-absent pose
requires a valid linked box. Multiple current linked boxes, unusable bounds,
and stale migration companions block the affected artifact.

The archive includes original image bytes, labels, `data.yaml`, split lists,
`labello-export.json`, and `checksums.json`. Portable hash-based paths avoid
source-name collisions and platform-specific filenames. Images are decoded
under memory limits and checked against their indexed dimensions and BLAKE3
hashes. Supported original encodings are static PNG, JPEG, WebP, and BMP;
animated images and nonidentity EXIF orientation are rejected. Export never
resizes or reencodes an image. YAML uses relative split lists, records class
and keypoint names, and does not invent `flip_idx`.

The manifest records the profile, mappings, options, source configuration and
index digests, image hashes and dimensions, split provenance, captured event
sequences, annotation identities and versions, origins, and linked or derived
box provenance. It describes the captured data; it is not a native restore
package.

## Preservation and loss

| Field | Exported representation | After production re-import |
| --- | --- | --- |
| Image bytes, hashes and dimensions | Original bytes and manifest mapping | Preserved; filenames and native image IDs are newly assigned |
| Bounding boxes | Normalized detection rows or pose bounds | Preserved within `1e-6`; pose bounds become native box annotations |
| Pose coordinates and states | Ordered keypoints with visibility `2/1/0` | Placed coordinates, names and states preserved; all-absent rows require explicit policy |
| Classes | Stable indices, names and task/class IDs in manifest | Distinct source categories stay distinct; native class IDs are new |
| Tasks | Selected identities in manifest | Import creates new tasks; native task configuration/history is not restored |
| Keypoint specifications | Per-class names/order and shape in YAML; selected skeleton spec in manifest | Names/order/count preserved; optionality, edges and workflow constraints are not a native restore contract |
| Splits | Relative lists and source memberships in manifest | Selected train/val/test assignment preserved; arbitrary original memberships are not restored |
| Native annotation IDs and versions | Row traces and provenance in manifest | New imported identities and versions |
| Reviews and uncertainty/missing-object evidence | Effective review affects eligibility; review records and evidence are not exported | Not restored |
| Import and migration provenance | Selected annotation origins, revision sources, object groups and linked boxes in manifest | New import provenance; original import audit and migration history are not restored |
| Excluded or incomplete coverage | Image omitted with reason | No imported image or negative example for that omitted image |
| Event history and users | Captured sequence and selected provenance only | New import-initialization events; original events, authors and review history are not restored |

## Capture and job lifecycle

Preflight reloads configuration and the image index from disk. Each image's
state and event sequence are captured together under the existing image lock.
Image copying and archive construction happen after releasing that lock.
Later event edits do not change the capture. Configuration, index, root
identity, or original-image changes before publication abort the job.

The normal lifecycle is `capturing -> ready -> building -> succeeded`.
Preflight can instead become `blocked` or `failed`. Cancellation requests
transition active work through `cancelling` and remove private payloads after
the worker releases them. Retry creates a new preflight and a new capture.
Only succeeded jobs are downloadable.

The writer bounds source, metadata, file counts, decoded memory, and archive
bytes. It verifies ZIP entry paths, sizes, CRCs, and hashes before atomic
no-replace publication. Downloads verify the completed archive hash and hold
a concurrency permit for the stream. Authentication and dataset DataAdmin
access are checked on every route and again after download checksum I/O.
Revocation prevents a new download; an already authorized stream is not
continuously reauthorized.

Startup marks unpublished captures and builds as interrupted and removes
their payloads. Verified completed artifacts remain downloadable until expiry.
Orphan reservations and expired jobs are removed before retained-job capacity
is enforced. Retention cleanup runs on requests and every 60 seconds. See
[configuration](configuration.md#export-limits),
[API routes](api.md#dataset-export-routes), and
[operations](operations.md#dataset-export).

## Round-trip contract

Extract the ZIP locally and import the extracted folder using the matching
explicit YOLO profile and `data.yaml`. Archive import remains unsupported.
Review discovered categories and mappings, attest authoritative ground truth
and exhaustive coverage for those categories, and acknowledge compatibility
warnings before committing to a new dataset.

For pose data that contains all-absent objects, explicitly select
`yoloZeroKeypoints = preserve_absent` and acknowledge
`yolo_absent_pose_preserved`. The default remains `incomplete` because an
external all-zero pose may mean unannotated keypoints.

The production round-trip regression checks original image bytes, distinct
classes, object counts, box geometry, keypoint states and names, verified empty
coverage, and train/val/test membership at normalized tolerance `1e-6`.
Pose import materializes boxes, including exported derived envelopes. Native
IDs, users, review history, workflow history, task identities, and migration
history are not restored. The new dataset has import provenance and freshly
created native identities.

The isolated Ultralytics `8.4.125` dataset reader also checks production output
for both profiles without constructing a model or permitting network access.
This proves reader compatibility for the tested fixtures, not model training
quality or compatibility with every future reader version. See the
[verification recipe](verification.md#export-round-trip-verification).
