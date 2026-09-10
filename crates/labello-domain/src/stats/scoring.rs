//! Version-one contribution scoring. All amounts are integer hundredths of a point.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationGeometry, AnnotationId, AnnotationVersion, ContributorDay, ContributorStats,
    EventLogEntry, EventPayload, HumanRevisionKind, ImageId, ImageState, ReviewCorrectionChange,
    ReviewDecision, ReviewRecord, ReviewTarget, RevisionSource, TaskId, TaskOutcome, TaskStatus,
    Timestamp, UserId,
};

pub const SCORING_VERSION: u32 = 1;
pub const FOCUS_SECONDS: i64 = 20 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FocusWindow {
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
    pub task_id: Option<TaskId>,
}

impl FocusWindow {
    pub fn contains(&self, timestamp: Timestamp) -> bool {
        self.starts_at <= timestamp && timestamp < self.ends_at
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDay {
    pub labels: u64,
    pub labeling: i64,
    pub reviewing: i64,
    pub deductions: i64,
    pub corrections: i64,
}

impl ScoreDay {
    pub fn total(&self) -> i64 {
        self.labeling + self.reviewing + self.corrections - self.deductions
    }

    pub fn add(&mut self, other: &Self) {
        self.labels += other.labels;
        self.labeling += other.labeling;
        self.reviewing += other.reviewing;
        self.deductions += other.deductions;
        self.corrections += other.corrections;
    }
}

/// Percentage applied to the next label; exactly 100 submitted labels unlock 110%.
pub fn daily_multiplier(previous_labels: u64) -> i64 {
    100 + (previous_labels / 100).min(5) as i64 * 10
}

/// Signed for period deltas: a period containing only penalties remains visible.
pub fn displayed_score(hundredths: i64) -> i64 {
    (hundredths.unsigned_abs() as f64).sqrt().floor() as i64 * hundredths.signum()
}

fn base_value(geometry: &AnnotationGeometry) -> i64 {
    match geometry {
        AnnotationGeometry::BoundingBox(_) => 2_000,
        AnnotationGeometry::Skeleton(skeleton) => {
            1_000 + 500 * skeleton.keypoints.len().saturating_sub(1) as i64
        }
    }
}

struct LabelAward {
    timestamp: Timestamp,
    image: ImageId,
    sequence: u64,
    annotation: AnnotationId,
    user: UserId,
    task: TaskId,
    base: i64,
    manual: bool,
}

#[derive(Default)]
pub struct ScoringProjection {
    labels: Vec<LabelAward>,
    days: BTreeMap<UserId, BTreeMap<String, ScoreDay>>,
}

impl ScoringProjection {
    fn day(&mut self, user: &UserId, timestamp: Timestamp) -> &mut ScoreDay {
        self.days
            .entry(user.clone())
            .or_default()
            .entry(timestamp.date_naive().to_string())
            .or_default()
    }

    pub fn record_image(&mut self, state: &ImageState, events: &[EventLogEntry]) {
        let mut current = BTreeMap::<AnnotationId, &AnnotationVersion>::new();
        let mut prelabels = BTreeSet::new();
        let mut credited = BTreeMap::<AnnotationId, (UserId, i64)>::new();
        let mut logical_labels = BTreeSet::new();
        let mut reviews = Vec::<(&ReviewRecord, ReviewTarget, Timestamp, u64)>::new();
        let mut reviewer_corrections = Vec::new();
        let mut dispositions = BTreeMap::new();
        for event in events {
            match &event.payload {
                EventPayload::MigrationDispositionChanged {
                    task_id,
                    object_group_id,
                    disposition,
                }
                | EventPayload::MigrationDispositionReopened {
                    task_id,
                    object_group_id,
                    disposition,
                } => {
                    if let crate::MigrationDispositionStatus::Annotated {
                        skeleton_annotation_id,
                        skeleton_version,
                    } = &disposition.status
                    {
                        dispositions.insert(
                            (task_id, object_group_id, disposition.disposition_version),
                            ReviewTarget::AnnotationVersion {
                                annotation_id: skeleton_annotation_id.clone(),
                                version: *skeleton_version,
                            },
                        );
                    }
                }
                EventPayload::AnnotationVersionCreated { annotation, .. } => {
                    if matches!(
                        annotation.revision_source,
                        RevisionSource::PrelabelSuggestion { .. }
                            | RevisionSource::Import { .. }
                            | RevisionSource::MigrationSkeleton { .. }
                    ) {
                        prelabels.insert(annotation.annotation_id.clone());
                    }
                    current.insert(annotation.annotation_id.clone(), annotation);
                }
                EventPayload::ImportInitialized { annotations, .. } => {
                    for annotation in annotations {
                        prelabels.insert(annotation.annotation_id.clone());
                        current.insert(annotation.annotation_id.clone(), annotation);
                    }
                }
                EventPayload::AnnotationDeleted { annotation_id, .. } => {
                    current.remove(annotation_id);
                }
                EventPayload::ReviewerCorrectionRecorded {
                    annotation, review, ..
                } => {
                    current.insert(annotation.annotation_id.clone(), annotation);
                    reviews.push((
                        review,
                        review.target.clone(),
                        event.timestamp,
                        event.event_sequence,
                    ));
                    reviewer_corrections.push((
                        annotation.as_ref(),
                        event.timestamp,
                        event.event_sequence,
                    ));
                }
                EventPayload::ReviewRecorded { review } => {
                    reviews.push((
                        review,
                        review.target.clone(),
                        event.timestamp,
                        event.event_sequence,
                    ));
                }
                EventPayload::ReviewCorrectionSubmitted {
                    assignment,
                    submission,
                    review,
                    ..
                } => {
                    // The receipt rejects the whole task, but only edited/removed
                    // labels earned the original author a per-label reward.
                    for change in &submission.changes {
                        let target = match change {
                            ReviewCorrectionChange::Edit {
                                annotation_id,
                                expected_version,
                                ..
                            }
                            | ReviewCorrectionChange::Remove {
                                annotation_id,
                                expected_version,
                            } => ReviewTarget::AnnotationVersion {
                                annotation_id: annotation_id.clone(),
                                version: *expected_version,
                            },
                            ReviewCorrectionChange::MigrationObject {
                                object_group_id,
                                expected_disposition_version,
                                ..
                            } => ReviewTarget::MigrationDisposition {
                                task_id: assignment.task_id.clone(),
                                object_group_id: object_group_id.clone(),
                                disposition_version: *expected_disposition_version,
                            },
                            ReviewCorrectionChange::Add { .. } => continue,
                        };
                        reviews.push((review, target, event.timestamp, event.event_sequence));
                    }
                }
                EventPayload::ReviewRevisionCommitted { replacement, .. } => {
                    reviews.extend(replacement.reviews.iter().map(|review| {
                        (
                            review,
                            review.target.clone(),
                            event.timestamp,
                            event.event_sequence,
                        )
                    }));
                }
                EventPayload::TaskStateChanged { task_state }
                    if task_state.completed_by.is_some()
                        && (task_state.status == TaskStatus::Submitted
                            || (task_state.status == TaskStatus::Completed
                                && task_state.outcome
                                    == Some(TaskOutcome::AnnotationCompleted))) =>
                {
                    for annotation in current.values().filter(|annotation| {
                        annotation.task_id == task_state.task_id
                            && !annotation.deleted
                            && matches!(
                                annotation.revision_source,
                                RevisionSource::Human {
                                    action: HumanRevisionKind::Authored | HumanRevisionKind::Edited
                                } | RevisionSource::PrelabelSuggestion { .. }
                            )
                    }) {
                        let logical = (
                            annotation.task_id.clone(),
                            annotation.object_group_id.as_ref().map_or_else(
                                || format!("annotation:{}", annotation.annotation_id),
                                |group| format!("group:{group}"),
                            ),
                        );
                        if credited.contains_key(&annotation.annotation_id)
                            || !logical_labels.insert(logical)
                        {
                            continue;
                        }
                        let base = base_value(&annotation.geometry);
                        credited.insert(
                            annotation.annotation_id.clone(),
                            (annotation.author_user_id.clone(), base),
                        );
                        self.labels.push(LabelAward {
                            timestamp: event.timestamp,
                            image: event.image_id.clone(),
                            sequence: event.event_sequence,
                            annotation: annotation.annotation_id.clone(),
                            user: annotation.author_user_id.clone(),
                            task: annotation.task_id.clone(),
                            base,
                            manual: !prelabels.contains(&annotation.annotation_id),
                        });
                    }
                }
                _ => {}
            }
        }

        let label_target = |target: &ReviewTarget| match target {
            ReviewTarget::MigrationDisposition {
                task_id,
                object_group_id,
                disposition_version,
            } => dispositions
                .get(&(task_id, object_group_id, *disposition_version))
                .unwrap_or(target)
                .clone(),
            _ => target.clone(),
        };
        for (_, target, _, _) in &mut reviews {
            *target = label_target(target);
        }
        let annotation_at = |id: &AnnotationId, version: u32| {
            state.annotations.get(id).and_then(|versions| {
                versions
                    .iter()
                    .find(|annotation| annotation.version == version)
            })
        };
        let mut reviewed = BTreeSet::new();
        let mut rejected = BTreeMap::new();
        let mut rejected_versions = BTreeSet::new();
        for (review, target, timestamp, sequence) in &reviews {
            let ReviewTarget::AnnotationVersion {
                annotation_id,
                version,
            } = target
            else {
                continue;
            };
            let Some(annotation) = annotation_at(annotation_id, *version) else {
                continue;
            };
            // Decision revisions still represent one piece of review work per person/object.
            if reviewed.insert((review.reviewer_user_id.clone(), annotation_id.clone())) {
                self.day(&review.reviewer_user_id, *timestamp).reviewing +=
                    base_value(&annotation.geometry) * 30 / 100;
            }
            if review.decision == ReviewDecision::Rejected
                && !state.superseded_review_ids.contains(&review.review_id)
            {
                rejected_versions.insert((annotation_id.clone(), *version));
                if rejected.contains_key(annotation_id) {
                    continue;
                }
                let base = credited
                    .get(annotation_id)
                    .map_or_else(|| base_value(&annotation.geometry), |(_, base)| *base);
                if let Some((author, _)) = credited.get(annotation_id) {
                    self.day(author, *timestamp).deductions += base * 50 / 100;
                }
                rejected.insert(annotation_id.clone(), (annotation, *sequence, base));
            }
        }
        let mut corrected = BTreeSet::new();
        let mut approvals = BTreeMap::<(AnnotationId, u32), BTreeSet<UserId>>::new();
        let mut required_approvals = BTreeMap::<(AnnotationId, u32), usize>::new();
        for context in state.review_assignment_contexts.values() {
            for target in &context.targets {
                let target = label_target(target);
                if let ReviewTarget::AnnotationVersion {
                    annotation_id,
                    version,
                } = &target
                {
                    let required = required_approvals
                        .entry((annotation_id.clone(), *version))
                        .or_insert(1);
                    *required = (*required).max(
                        context
                            .task
                            .review
                            .legacy
                            .as_ref()
                            .map_or(1, |legacy| legacy.required_reviews.max(1) as usize),
                    );
                }
            }
        }
        for (review, target, timestamp, sequence) in &reviews {
            let ReviewTarget::AnnotationVersion {
                annotation_id,
                version,
            } = target
            else {
                continue;
            };
            let Some((rejection, rejection_sequence, base)) = rejected.get(annotation_id) else {
                continue;
            };
            let Some(annotation) = annotation_at(annotation_id, *version) else {
                continue;
            };
            let key = (annotation_id.clone(), *version);
            if review.decision == ReviewDecision::Approved
                && !state.superseded_review_ids.contains(&review.review_id)
            {
                approvals
                    .entry(key.clone())
                    .or_default()
                    .insert(review.reviewer_user_id.clone());
            }
            if review.decision != ReviewDecision::Approved
                || state.superseded_review_ids.contains(&review.review_id)
                || sequence <= rejection_sequence
                || annotation.version <= rejection.version
                || annotation.geometry == rejection.geometry
                || !matches!(
                    annotation.revision_source,
                    RevisionSource::Human { .. } | RevisionSource::ReviewerCorrection { .. }
                )
                || rejected_versions.contains(&key)
                || approvals.get(&key).map_or(0, BTreeSet::len)
                    < required_approvals.get(&key).copied().unwrap_or(1)
                || !corrected.insert(annotation_id.clone())
            {
                continue;
            }
            self.day(&annotation.author_user_id, *timestamp).corrections += base * 20 / 100;
        }
        // Historical correction events accepted their replacement atomically.
        // Current correction submissions instead require a fresh approval above.
        for (annotation, timestamp, sequence) in reviewer_corrections {
            if let Some((rejection, rejection_sequence, base)) =
                rejected.get(&annotation.annotation_id)
                && sequence >= *rejection_sequence
                && annotation.version > rejection.version
                && annotation.geometry != rejection.geometry
                && corrected.insert(annotation.annotation_id.clone())
            {
                self.day(&annotation.author_user_id, timestamp).corrections += base * 20 / 100;
            }
        }
    }

    pub fn finish(
        mut self,
        contributors: &mut BTreeMap<UserId, ContributorStats>,
        focus: &[FocusWindow],
    ) {
        self.labels.sort_by(|a, b| {
            (a.timestamp, &a.image, a.sequence, &a.annotation).cmp(&(
                b.timestamp,
                &b.image,
                b.sequence,
                &b.annotation,
            ))
        });
        for label in std::mem::take(&mut self.labels) {
            let focused = focus
                [..focus.partition_point(|window| window.starts_at <= label.timestamp)]
                .last()
                .is_some_and(|window| {
                    window.task_id.as_ref() == Some(&label.task) && window.contains(label.timestamp)
                });
            let day = self.day(&label.user, label.timestamp);
            let bonus = 100 + if label.manual { 10 } else { 0 } + if focused { 25 } else { 0 };
            day.labeling += label.base * bonus * daily_multiplier(day.labels) / 10_000;
            day.labels += 1;
        }
        for (user, days) in self.days {
            let contributor =
                contributors
                    .entry(user.clone())
                    .or_insert_with(|| ContributorStats {
                        display_name: user.to_string(),
                        ..Default::default()
                    });
            for (date, score) in days {
                if let Some(day) = contributor.history.iter_mut().find(|day| day.day == date) {
                    day.score = score;
                } else {
                    contributor.history.push(ContributorDay {
                        day: date,
                        score,
                        ..Default::default()
                    });
                }
            }
            contributor.history.sort_by(|a, b| a.day.cmp(&b.day));
        }
    }
}

#[cfg(test)]
mod tests;
