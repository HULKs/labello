# Contribution scoring

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
label adds 10% of base, and a label in the focus workflow adds 50% of base.
Those percentages add together before the daily multiplier: manual plus focus
is ×1.60, not ×1.65. Accepted prelabels
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

Reviewing an annotation earns 80% of its geometry's base value, once per reviewer
and annotation, regardless of approval or rejection. This rate also applies when
rebuilding historical reviews; a bounding-box review earns 16 points.
Reviewing a label during its recorded reviewer-focus window multiplies that
review reward by ×1.5: a focused bounding-box review earns 24 points.
Task-wide final checks and missing-object reports do not earn per-label review points.
Guided migration disposition reviews resolve to their recorded skeleton version;
object exclusions without a skeleton earn no label-based reward.

An effective label rejection deducts 80% of its original credited base value,
at most once per label. Multiple reviewers and correction attempts do not stack
deductions. A subsequently approved geometry correction earns its author 80% of
that base value, at most once; it does not refund the deduction. Current reviewer
corrections start a fresh review round and earn the correction reward only after
approval. Their task-wide rejection penalizes edited/removed labels, not unchanged
labels or missing objects added by the reviewer. Historical
`ReviewerCorrectionRecorded` events retain their immediate acceptance and correction
reward. Corrections receive no bonuses. Reviews receive only the reviewer-focus
bonus, never the manual bonus or daily multiplier. Neither reviews nor corrections
advance the daily label counter. Ordinary
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

Annotation and review have independent focus selections and ten-minute timers.
Each selects the enabled, non-blocked task with the largest backlog for its stage.
Annotation backlog excludes submitted/completed tasks and excluded import coverage;
review backlog counts submitted work awaiting approval review. All classes in a
task share its stage's bonus. A tie retains the previous winner if it remains eligible and
tied; otherwise task ID order decides. No eligible backlog selects no task.

Statistics and image-score requests, annotation submissions, and review decisions
check the relevant selections. If that stage's imbalance enforcement blocks its
focused task, focus switches to an eligible task and resets the full ten-minute timer. Per-user assignment leases
and prefetch reservation limits are not category-wide imbalance blocks. The
switch is recorded durably: the old window ends at the switch, and the new one
starts then. Selection and remaining time survive restart; unused intervals do
not require a background worker.

Annotation and review workflow selectors show their own `×1.5` and remaining minutes. A short,
right-edge Balatro-inspired popup announces each new focus selection with its
category and multiplier, using the same pop, tilt, upward drift and fade as score
feedback. Reduced motion uses static feedback. A reported imbalance block hides
the old marker and requests a statistics refresh immediately. Failed refreshes
and expired selections are never advertised as an active bonus.

The bonus encourages balanced completion across tasks. It does not change
assignment order or guarantee that nearly complete images are selected first.

## Existing datasets and persistence

Historical label and review events are replayed under the same version-1 policy,
including historical daily tiers, rejection deductions, and attributable accepted
corrections. Review rewards, approved corrections and rejection deductions use
the current 80% rates throughout history. Recorded historical focus periods use
the current +50% labeling bonus. Reviewer focus starts at its own activation;
historical reviewer-focus periods are not invented from annotation focus history.
Missing author/source evidence is not invented. Old source annotations
must not be credited to the administrator who imported them. Focus bonuses begin
at activation; past focus selections are never inferred from today's backlog.

Per-image event logs remain untouched. Score totals are derived, cached with
statistics, and rebuildable. The private `.labello/scoring/focus-v1.json` file
is authoritative bonus history with its own policy version. Version 2 stores
separate rolling annotation and review windows. Legacy version-1 twenty-minute history is preserved and upgraded
on access; an active legacy window is closed at the upgrade time and replaced by
a new ten-minute selection without changing its past coverage. Its replacement uses
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
the raw point total, abbreviated with `k` for thousands and `m` for millions
(for example, 45,071.29 points displays as `45k`). Hover and accessibility details
retain exact points. Sorting and ties use exact underlying points so display
rounding cannot reorder contributors. Negative selected-period totals retain
their negative sign. Period totals include rewards and deductions
on their event dates; history graphs retain the existing cumulative-through-day
semantics. The UI shows one score, without a pending balance, plus today's label
count, next daily tier, and multiplier. The existing activity and acceptance
metrics remain available. Older servers without scoring support are explicitly
identified rather than displayed as zero scores.

After successful annotation, review, reviewer-correction, or migration completion,
a brief score-gain popup appears at the right edge when the next image loads.
It uses the server's points attributed to the current user and the completed
image's event-sequence window, including object reviews made before the final
image check. It does not estimate rewards from geometry or subtract dataset-wide
totals. Deferred correction rewards appear only when actually awarded; they are
not promised by a correction submission.

The popup never blocks pointer input or delays navigation. It pops, floats upward,
and fades; reduced motion uses a static acknowledgement instead. Skips, failed
saves, zero/negative gains, obsolete replies, and unavailable score receipts do
not celebrate. Pending feedback is discarded when the account/workspace changes
or it would arrive too late to belong to the next image. Older servers without
the receipt endpoint continue to work without the effect.
