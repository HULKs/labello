use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    EventLogEntry, EventPayload, ReviewTarget, TaskOutcome, TaskStatus, Timestamp, UserId,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivityCounts {
    pub annotation_tasks_submitted: u64,
    pub final_task_reviews: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UtcActivityWindow {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl UtcActivityWindow {
    pub fn containing(timestamp: Timestamp) -> Self {
        let start = timestamp
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        Self {
            start,
            end: start + chrono::Duration::days(1),
        }
    }

    pub fn contains(self, timestamp: Timestamp) -> bool {
        self.start <= timestamp && timestamp < self.end
    }
}

/// Projects committed activity, independently of the final task state. Image
/// identity participates in deduplication even when several logs are supplied.
pub fn daily_activity_from_events(
    events: &[EventLogEntry],
    window: UtcActivityWindow,
) -> BTreeMap<UserId, DailyActivityCounts> {
    let mut submissions = BTreeSet::new();
    let mut reviews = BTreeSet::new();
    for event in events
        .iter()
        .filter(|event| window.contains(event.timestamp))
    {
        match &event.payload {
            EventPayload::TaskStateChanged { task_state }
                if task_state.completed_by.as_ref() == Some(&event.actor_user_id)
                    && task_state.completed_at.is_some()
                    && (task_state.status == TaskStatus::Submitted
                        || (task_state.status == TaskStatus::Completed
                            && task_state.outcome == Some(TaskOutcome::AnnotationCompleted))) =>
            {
                submissions.insert((
                    event.actor_user_id.clone(),
                    event.image_id.clone(),
                    task_state.task_id.clone(),
                ));
            }
            EventPayload::ReviewRecorded { review }
                if review.reviewer_user_id == event.actor_user_id =>
            {
                if let ReviewTarget::Task { task_id }
                | ReviewTarget::MigrationConfirmation { task_id, .. } = &review.target
                {
                    reviews.insert((
                        event.actor_user_id.clone(),
                        event.image_id.clone(),
                        task_id.clone(),
                    ));
                }
            }
            EventPayload::ReviewRevisionCommitted { replacement, .. } => {
                // The compound commit timestamp owns the day; staged review timestamps
                // and later supersession do not erase this committed activity.
                for review in &replacement.reviews {
                    if review.reviewer_user_id != event.actor_user_id {
                        continue;
                    }
                    if let ReviewTarget::Task { task_id }
                    | ReviewTarget::MigrationConfirmation { task_id, .. } = &review.target
                    {
                        reviews.insert((
                            event.actor_user_id.clone(),
                            event.image_id.clone(),
                            task_id.clone(),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    let mut counts = BTreeMap::<UserId, DailyActivityCounts>::new();
    for (user_id, ..) in submissions {
        counts
            .entry(user_id)
            .or_default()
            .annotation_tasks_submitted += 1;
    }
    for (user_id, ..) in reviews {
        counts.entry(user_id).or_default().final_task_reviews += 1;
    }
    counts
}

#[cfg(test)]
mod tests;
