use std::collections::{BTreeMap, BTreeSet};

use labello_domain::{
    ContributorDay, ContributorStats, EventLogEntry, EventPayload, ImageState, ReviewDecision,
    ReviewTarget, RevisionSource, TaskOutcome, TaskStatus, UserId, WorkflowEvent, WorkflowItem,
};

use super::aggregation::StatsAggregation;

#[derive(Debug, Default)]
pub(super) struct ContributorAggregation(BTreeMap<UserId, BTreeMap<String, ContributorDay>>);

impl ContributorAggregation {
    pub(super) fn extend(&mut self, other: &Self) {
        for (user, days) in &other.0 {
            for (date, day) in days {
                let target = self
                    .0
                    .entry(user.clone())
                    .or_default()
                    .entry(date.clone())
                    .or_insert_with(|| ContributorDay {
                        day: date.clone(),
                        ..Default::default()
                    });
                target.labeled += day.labeled;
                target.reviewed += day.reviewed;
                target.accepted += day.accepted;
                target.rejected += day.rejected;
            }
        }
    }

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
        // A receipt and its compatibility events describe one completed item.
        // Keep legacy submissions in the Overview slot, including histories that
        // later acquire item receipts, and deduplicate across history leases.
        let workflow_reviews: BTreeMap<_, _> = state
            .workflow_confirmations
            .values()
            .filter_map(|confirmation| {
                Some((
                    &confirmation.review.as_ref()?.review_id,
                    (
                        &confirmation.task_id,
                        &state
                            .workflow_assignments
                            .get(&confirmation.assignment_id)?
                            .item,
                    ),
                ))
            })
            .collect();
        let mut reviewed_items = BTreeSet::new();
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
                        if labeled.insert((&task_state.task_id, user, None)) {
                            self.contributors.day(user, event.timestamp).labeled += 1;
                        }
                    }
                    continue;
                }
                EventPayload::Workflow { event: workflow } => {
                    let WorkflowEvent::ItemConfirmed { confirmation } = workflow.as_ref() else {
                        continue;
                    };
                    if let Some(review) = &confirmation.review {
                        (std::slice::from_ref(review), event.timestamp)
                    } else {
                        if let Some(context) =
                            state.workflow_assignments.get(&confirmation.assignment_id)
                        {
                            let object = match &context.item {
                                WorkflowItem::Object { object } => Some(object),
                                WorkflowItem::Overview => None,
                            };
                            if labeled.insert((&confirmation.task_id, &event.actor_user_id, object))
                            {
                                self.contributors
                                    .day(&event.actor_user_id, event.timestamp)
                                    .labeled += 1;
                            }
                        }
                        continue;
                    }
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
                EventPayload::ReviewCorrectionSubmitted { review, .. }
                | EventPayload::ReviewRecorded { review }
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
                let credit = workflow_reviews
                    .get(&review.review_id)
                    .is_none_or(|(task, item)| {
                        let object = match item {
                            WorkflowItem::Object { object } => Some(object),
                            WorkflowItem::Overview => None,
                        };
                        reviewed_items.insert((*task, &review.reviewer_user_id, object))
                    });
                if credit {
                    self.contributors
                        .day(&review.reviewer_user_id, timestamp)
                        .reviewed += 1;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use labello_domain::{
        AssignmentId, DatasetId, DatasetMetadata, DatasetRole, ImageId, TaskId, TaskState,
        Timestamp, WorkflowAssignmentContext, WorkflowConfirmation, WorkflowObject,
    };

    #[test]
    fn item_activity_keeps_utc_dates_and_legacy_credit_without_recounting_history() {
        let before: Timestamp = "2026-09-30T23:59:59Z".parse().unwrap();
        let after: Timestamp = "2026-10-01T00:00:00Z".parse().unwrap();
        let image = ImageId::from("image");
        let task = TaskId::from("task");
        let user = UserId::from("author");
        let mut state = ImageState::new(image.clone());
        let mut events = vec![EventLogEntry::new(
            1,
            image.clone(),
            user.clone(),
            DatasetRole::Annotator,
            before,
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: task.clone(),
                    status: TaskStatus::Submitted,
                    outcome: None,
                    assigned_to: None,
                    completed_by: Some(user.clone()),
                    completed_at: Some(before),
                    updated_at: before,
                },
            },
        )];
        // Existing submission, then an Overview receipt, two distinct objects,
        // and a later history lease for the first object. Receipts without
        // geometry also represent valid exclusions/dismissed predictions.
        for (index, item, at) in [
            (0, WorkflowItem::Overview, after),
            (
                1,
                WorkflowItem::Object {
                    object: WorkflowObject::Prelabel {
                        suggestion_id: "first".into(),
                    },
                },
                before,
            ),
            (
                2,
                WorkflowItem::Object {
                    object: WorkflowObject::Migration {
                        object_group_id: "second".into(),
                    },
                },
                after,
            ),
            (
                3,
                WorkflowItem::Object {
                    object: WorkflowObject::Prelabel {
                        suggestion_id: "first".into(),
                    },
                },
                after,
            ),
        ] {
            let assignment_id = AssignmentId::from(format!("assignment-{index}"));
            state.workflow_assignments.insert(
                assignment_id.clone(),
                WorkflowAssignmentContext {
                    item,
                    task_fingerprint: String::new(),
                    overview_fingerprint: None,
                    review_target: None,
                    review_exception: false,
                    source_assignment_id: None,
                },
            );
            events.push(EventLogEntry::new(
                events.len() as u64 + 1,
                image.clone(),
                user.clone(),
                DatasetRole::Annotator,
                at,
                EventPayload::Workflow {
                    event: Box::new(WorkflowEvent::ItemConfirmed {
                        confirmation: WorkflowConfirmation {
                            assignment_id,
                            task_id: task.clone(),
                            annotation: None,
                            review: None,
                            reviewed_targets: vec![],
                        },
                    }),
                },
            ));
        }
        let mut aggregation = StatsAggregation::new(&DatasetMetadata::new(
            DatasetId::from("dataset"),
            "Dataset",
            before,
        ));
        aggregation.record_contributors(&state, &events);
        let contributors = aggregation.contributors.finish();
        let history = &contributors[&user].history;
        assert_eq!(history.len(), 2);
        assert_eq!((&*history[0].day, history[0].labeled), ("2026-09-30", 2));
        assert_eq!((&*history[1].day, history[1].labeled), ("2026-10-01", 1));
    }
}
