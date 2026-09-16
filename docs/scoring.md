# Contribution scoring

> **Status:** Normative current reference
> **Owner:** Domain and UI maintainers
> **Audience:** Contributors, operators, maintainers
> **Last verified:** 2026-09-09, scoring-v1 implementation and regression tests

Scores belong to one dataset and user. Existing contributor activity counts remain
available and retain their earlier meaning: `labeled` counts submitted image/task
combinations, whereas the score's daily `labels` counts individual annotation
objects. An empty submission advances activity but earns no label points.

## Rewards

Version 1 uses integer hundredths of a point. A one-keypoint skeleton earns 10
base points, each additional keypoint adds 5, and a bounding box earns 20. A
skeleton counts once, including its coordinate-free keypoint outcomes; autosaves
and editing individual points do not create additional labels.

The first submission of a label earns its base reward immediately. A manual
label adds 10% of base, and a label in the focus workflow adds 25% of base.
Those percentages add together before the daily multiplier. Accepted prelabels
earn the ordinary base reward. Their recorded provenance survives later human
edits for scoring purposes. Automatic/import-only geometry and unchanged imported
acceptances earn no labeling reward. Attributable human edits of imports can earn
credit when submitted, without the manual-creation bonus.

Every 100 labels previously credited that UTC day increases subsequent labeling
rewards by 10%: labels 1–100 earn ×1.00, 101–200 earn ×1.10, and so on, with
×1.50 for label 501 onward. A box or whole skeleton advances the counter by one.
The counter resets at 00:00 UTC. Earlier rewards never change tier retroactively;
rejections do not reduce the daily counter. Deterministic submission timestamp,
image ID, event sequence, and annotation ID ordering resolves simultaneous work.

Reviewing an annotation earns 30% of its geometry's base value, once per reviewer
and annotation, regardless of approval or rejection. Task-wide final checks and
missing-object reports do not earn per-label review points.
Guided migration disposition reviews resolve to their recorded skeleton version;
object exclusions without a skeleton earn no label-based reward.

An effective label rejection deducts 50% of its original credited base value,
at most once per label. Multiple reviewers and correction attempts do not stack
deductions. A subsequently approved geometry correction earns its author 20% of
that base value, at most once; it does not refund the deduction. Current reviewer
corrections start a fresh review round and earn the correction reward only after
approval. Their task-wide rejection penalizes edited/removed labels, not unchanged
labels or missing objects added by the reviewer. Historical
`ReviewerCorrectionRecorded` events retain their immediate acceptance and correction
reward. Review and correction
rewards receive no bonuses and do not advance the daily label counter. Ordinary
acceptance earns no additional points. A task rejected for missing objects does
not penalize its valid labels.

Explicitly superseded erroneous rejection decisions are omitted when rebuilding
deductions. Correcting a real error does not supersede that rejection, so its
deduction remains. Revisions of the same annotation cannot earn a second labeling
reward. Linked object-group identities also prevent repeat label credit within a
task. Unlinked objects with newly generated IDs cannot be recognized reliably as
recreations of the same physical object; no geometry-based identity heuristic is
used.

## Focus workflow

Each fixed UTC 20-minute interval selects the enabled task with the most images
still needing annotation. Submitted/completed tasks and excluded import coverage
are excluded from its backlog. The calculation replays event prefixes preceding
the interval boundary using the current task definition and image membership.
All classes in a task share its bonus. A tie retains the previous winner if it
is still tied, otherwise task ID order decides. Zero backlog selects no task.

The first statistics request or annotation submission needing an interval records
its selection durably. The first activation starts at that request/submission
timestamp, never before scoring was enabled. Subsequent selections expire at the
next `:00`, `:20`, or `:40` boundary. Selection persists across restart and stays
fixed during the interval even when backlog or configuration changes. Unused
intervals require no background worker. The annotation workflow selector marks
the current focus with its +25% bonus and remaining minutes. Failed statistics
refresh hides the marker, and an expired marker is never shown as active.

The bonus encourages balanced completion across tasks. It does not change
assignment order or guarantee that nearly complete images are selected first.

## Existing datasets and persistence

Historical label and review events are replayed under the same version-1 policy,
including historical daily tiers, rejection deductions, and attributable accepted
corrections. Missing author/source evidence is not invented. Old source annotations
must not be credited to the administrator who imported them. Focus bonuses begin
at activation; past focus selections are never inferred from today's backlog.

Per-image event logs remain untouched. Score totals are derived, cached with
statistics, and rebuildable. The private `.labello/scoring/focus-v1.json` file
is authoritative bonus history with its own policy version. Its replacement uses
the repository's synced atomic publication, serialized across repository clones.
Submission and offline-sync transactions publish it before events using a new
selection can commit; raw event writes and historical policy upgrades do not
activate scoring. Interrupted
publication may leave an unused selection or temporary file, but cannot commit a
submission whose selection was lost. Malformed or unsupported history fails
closed; restore it from backup, never delete it as a cache. Snapshots include this
file, and full-root backups preserve it. No root schema-version bump or historical
event migration is required.

## Leaderboard

Score is the primary leaderboard metric: its full-width podium leads Statistics,
stays expanded on mobile, and precedes rankings and daily activity. Other metric
highlights sit in a secondary disclosure, with their chart cards stacked on
mobile and explicit empty states for categories without ranked activity.
Score comes first in the sorting and
history metric choices and remains the default. Compact rankings use a sort picker
and direction button, with score-first summaries wrapping under contributor names.
The mobile podium stays visible with less vertical space. Its displayed value is
`floor(10 * sqrt(points))`; sorting and ties use exact underlying points so display
rounding cannot reorder contributors. Negative selected-period totals use the same
compression with a negative sign. Period totals include rewards and deductions
on their event dates; history graphs retain the existing cumulative-through-day
semantics. The UI shows one score, without a pending balance, plus today's label
count, next daily tier, and multiplier. The existing activity and acceptance
metrics remain available. Older servers without scoring support are explicitly
identified rather than displayed as zero scores.
