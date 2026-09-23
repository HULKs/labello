# Dataset administration

A bootstrap administrator creates datasets. Within a dataset, the data-admin
role manages configuration and membership; annotator and reviewer are separate
permissions. Any member can inspect images and view statistics.

## Create or reuse a schema

Setup creates a fresh dataset. **Copy schema from** lists datasets where you
have DataAdmin access and previews classes, workflows, and ordered keypoints.
Creation also requires bootstrap-administrator authority. A missing, invalid,
or inaccessible source fails creation without silently switching to an empty schema.

| Copied | Omitted |
| --- | --- |
| Class and task IDs, names, relationships | Images, annotations, reviews, assignment history |
| Workflow settings and instructions text | Source users, roles, ingestion paths, balance settings |
| Skeleton specifications and ordered keypoints | Tutorial image references and prelabel bindings/models |
| Migration guide links | Original dataset identity |

The datasets are independent after creation. Matching IDs, unchanged definitions,
and matching [export selections](export.md#compatible-exports-from-copied-schemas)
produce compatible class indices and keypoint layouts. Later edits or different
selections can break that compatibility.

## Configure a dataset

Administration groups changes into Overview, People, Images, Schema, Automation,
Backups, and Export. Configuration edits remain staged while switching sections.
Resolve validation errors and save before starting work that requires saved
configuration.

| Section | Use |
| --- | --- |
| Overview | Dataset identity and configuration overview |
| People | Assign annotator, reviewer, and data-admin roles; bootstrap-admin protections still apply |
| Images | Configure relative filesystem roots, upload folders, run ingestion, inspect job results |
| Schema | Define classes and box/skeleton tasks, keypoint order, workflow settings, and instructions |
| Automation | Configure assignment balance and prelabels; model execution is currently a placeholder |
| Backups | Create and download annotation snapshots |
| Export | Preflight, build, inspect, cancel, and download private detection/pose jobs |

An approval workflow requires one reviewer. Without approval review, annotation
submission completes the task. The legacy `allowReviewerCorrections` setting
remains readable but no longer disables corrections. Instructions display title
and text; configured example images are not rendered.

Assignment balance uses an absolute `maxDifference` between enabled tasks.
Disabled tasks are excluded, and there is no independent class-level balance.
See [assignment](assignment.md#completion-balance) for count and boundary rules.

## Add images

Use relative filesystem image roots or upload a browser folder, then ingest.
Ingestion indexes image bytes and deduplicates by BLAKE3 identity. Existing image
IDs and known duplicate paths survive reconciliation. A filename alone is not
image identity. Ingest job status is process-local and does not survive restart
as a durable job.

Raw image ingestion does not import external labels. Use [dataset import](import.md)
for a new dataset from supported YOLO or COCO ground truth.

## Inspect and return work to review

Inspect is a read-only gallery for every dataset member. Search and filter by
workflow, class, or status. The matching image list loads automatically in batches;
thumbnails load as they become visible. The gallery distinguishes the total match
count from the loaded list and labels previous results while a new filter loads.
Previous/next navigation follows those filters. Gallery filters stay in memory
and reset on reload.

A workflow filter includes unannotated images with Pending status, so selecting
a workflow alone may leave the count unchanged. **All workflows → Completed**
requires every configured workflow, including disabled ones, to be completed;
datasets without workflows match no completed images. Selecting one workflow
checks only its status. Other status filters under All workflows match any
workflow. Class filters match active annotations.

Toggle workflow, status, and geometry overlays without claiming or editing an
assignment. Boxes and skeletons use class colors and labels. A visible box supplies
the label for its object group; hiding it restores labels on the visible keypoints.

Reviewers and data admins use **Return to review** in the Overlays panel or drawer
to select completed approval workflows and enter a nonblank reason of at most
2000 UTF-8 bytes. Selection is independent of overlay visibility. All selected
workflows must be enabled, completed, free of a live assignment, and have valid
review targets. Disabled choices explain their exclusion through the adjacent
info control. Imported ground truth has review disabled by default; enable
approval review before returning its completed workflows. A conflict leaves the
whole selection unchanged.

Successful return preserves geometry and historical decisions, clears the final
outcomes, and starts fresh review rounds. Every current object and full-image
check needs a new decision. A failed request keeps the reason and exact retry
identity. Discard or success clears the draft while keeping the controls available.
This action records audit history but currently sends no notification.

## Statistics and scores

Statistics opens as a dataset-scoped modal without releasing current work.
It reports five exclusive task states: Pending, In progress, Awaiting review,
Needs correction, and Completed. Imported excluded coverage is outside the
completion denominator until explicitly included.

The score podium and rankings lead the view, followed by daily activity, dataset
counts, assignment balance, task/class breakdowns, and throughput. Period and
history controls compare contributors. Empty or unavailable data is distinct
from zero activity. Acceptance percentages accompany review counts.
[Scoring](scoring.md) defines per-object rewards, daily tiers, focus bonuses,
rejections, and the compressed displayed score.

Daily streaks require 20 distinct image/task submissions or 30 reviews per dataset
per UTC day. Flames and day counts appear beside contributors in the leaderboard
and in the top bar. The top-bar flame opens Statistics. Streaks are derived from
contributor history; reduced-motion settings suppress the goal animation.

## Snapshots and exports

Snapshots contain metadata and replayable annotation/audit history, including
committed import provenance and scoring focus history. They omit images,
authentication, user keybindings, and private import/export jobs. There is no
native snapshot restore. Use the full-root [backup procedure](operations.md#backup-and-restore)
for recovery.

[Dataset export](export.md) produces verified archives with original images and
complete labels. It has explicit completeness and round-trip boundaries and
requires current DataAdmin access, including when downloading retained jobs.
