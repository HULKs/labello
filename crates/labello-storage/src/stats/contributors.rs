use std::collections::{BTreeMap, BTreeSet};

use labello_domain::{
    ContributorDay, ContributorStats, EventLogEntry, EventPayload, ImageState, ReviewDecision,
    ReviewTarget, RevisionSource, TaskOutcome, TaskStatus, UserId,
};

use super::aggregation::StatsAggregation;

#[derive(Default)]
pub(super) struct ContributorAggregation(BTreeMap<UserId, BTreeMap<String, ContributorDay>>);

impl ContributorAggregation {
    fn day(&mut self, user: &UserId, timestamp: labello_domain::Timestamp) -> &mut ContributorDay {
        let day = timestamp.date_naive().to_string();
        self.0
            .entry(user.clone())
            .or_default()
            .entry(day.clone())
            .or_insert_with(|| ContributorDay {
                day,
                ..Default::default()
            })
    }

    pub(super) fn finish(self) -> BTreeMap<UserId, ContributorStats> {
        self.0
            .into_iter()
            .map(|(user, days)| {
                let stats = ContributorStats {
                    display_name: user.to_string(),
                    history: days.into_values().collect(),
                    ..Default::default()
                };
                (user, stats)
            })
            .collect()
    }
}

impl StatsAggregation {
    pub(super) fn record_contributors(&mut self, state: &ImageState, events: &[EventLogEntry]) {
        let mut submissions = BTreeMap::new();
        let mut labeled = BTreeSet::new();
        let mut reviews = BTreeSet::new();
        let mut dispositions = BTreeMap::new();
        let mut confirmations = BTreeMap::new();
        for event in events {
            let (event_reviews, timestamp) = match &event.payload {
                EventPayload::TaskStateChanged { task_state }
                    if task_state.status == TaskStatus::Submitted
                        || (task_state.status == TaskStatus::Completed
                            && task_state.outcome == Some(TaskOutcome::AnnotationCompleted)) =>
                {
                    if let Some(user) = &task_state.completed_by {
                        submissions.insert(&task_state.task_id, user);
                        if labeled.insert((&task_state.task_id, user)) {
                            self.contributors.day(user, event.timestamp).labeled += 1;
                        }
                    }
                    continue;
                }
                EventPayload::MigrationDispositionChanged {
                    task_id,
                    object_group_id,
                    disposition,
                } => {
                    dispositions.insert(
                        (task_id, object_group_id, disposition.disposition_version),
                        &event.actor_user_id,
                    );
                    continue;
                }
                EventPayload::MigrationFullImageConfirmed { confirmation } => {
                    confirmations.insert(
                        (&confirmation.task_id, &confirmation.confirmation_hash),
                        &confirmation.actor_user_id,
                    );
                    continue;
                }
                EventPayload::ReviewRecorded { review }
                | EventPayload::ReviewerCorrectionRecorded { review, .. } => {
                    (std::slice::from_ref(review), review.timestamp)
                }
                EventPayload::ReviewRevisionCommitted { replacement, .. } => {
                    // Staged decisions become activity when the revision is committed.
                    (replacement.reviews.as_slice(), event.timestamp)
                }
                _ => continue,
            };
            for review in event_reviews {
                if !reviews.insert(&review.review_id) {
                    continue;
                }
                self.contributors
                    .day(&review.reviewer_user_id, timestamp)
                    .reviewed += 1;
                let author = match &review.target {
                    ReviewTarget::Task { task_id } => submissions.get(task_id).copied(),
                    ReviewTarget::AnnotationVersion {
                        annotation_id,
                        version,
                    } => state
                        .annotations
                        .get(annotation_id)
                        .and_then(|versions| {
                            versions
                                .iter()
                                .find(|annotation| annotation.version == *version)
                        })
                        .filter(|annotation| {
                            matches!(annotation.revision_source, RevisionSource::Human { .. })
                        })
                        .map(|annotation| &annotation.author_user_id),
                    ReviewTarget::MigrationDisposition {
                        task_id,
                        object_group_id,
                        disposition_version,
                    } => dispositions
                        .get(&(task_id, object_group_id, *disposition_version))
                        .copied(),
                    ReviewTarget::MigrationConfirmation {
                        task_id,
                        confirmation_hash,
                    } => confirmations.get(&(task_id, confirmation_hash)).copied(),
                    // Image-wide records do not identify one contributor's work.
                    ReviewTarget::Image { .. } => None,
                };
                if let Some(author) = author {
                    let day = self.contributors.day(author, timestamp);
                    match review.decision {
                        ReviewDecision::Approved => day.accepted += 1,
                        ReviewDecision::Rejected => day.rejected += 1,
                    }
                }
            }
        }
    }
}
