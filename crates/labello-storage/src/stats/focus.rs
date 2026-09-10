use labello_domain::{
    EventLogEntry, EventPayload, FocusWindow, TaskOutcome, TaskStatus, Timestamp, rebuild_state,
    stats::scoring::{FOCUS_SECONDS, SCORING_VERSION},
};
use serde::{Deserialize, Serialize};

use crate::{
    DatasetRepository, StorageError, StorageResult,
    error::PathIo,
    fsjson::{read_json, write_json_atomic},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusHistory {
    version: u32,
    windows: Vec<FocusWindow>,
}

impl DatasetRepository {
    pub(crate) async fn prepare_scoring_focus(
        &self,
        events: &[EventLogEntry],
    ) -> StorageResult<()> {
        if events.iter().any(|event| {
            matches!(&event.payload,
                EventPayload::TaskStateChanged { task_state }
                    if task_state.completed_by.is_some()
                        && (task_state.status == TaskStatus::Submitted
                            || (task_state.status == TaskStatus::Completed
                                && task_state.outcome == Some(TaskOutcome::AnnotationCompleted)))
            )
        }) {
            self.scoring_focus(events.last().expect("submission batch").timestamp)
                .await?;
        }
        Ok(())
    }

    /// Shared by statistics and submission commits. Publication precedes any reward
    /// using this window; cancellations can leave an unused window, never lost credit.
    pub(crate) async fn scoring_focus(
        &self,
        timestamp: Timestamp,
    ) -> StorageResult<Vec<FocusWindow>> {
        let mut cached = self.scoring.lock().await;
        let path = self.root().join(".labello/scoring/focus-v1.json");
        if cached.is_none() {
            let history: FocusHistory = if tokio::fs::try_exists(&path).await.with_path(&path)? {
                read_json(&path).await?
            } else {
                FocusHistory {
                    version: SCORING_VERSION,
                    windows: Vec::new(),
                }
            };
            if history.version != SCORING_VERSION
                || history.windows.iter().any(|window| {
                    window.starts_at >= window.ends_at
                        || window.ends_at.timestamp().rem_euclid(FOCUS_SECONDS) != 0
                        || (window.ends_at - window.starts_at).num_seconds() > FOCUS_SECONDS
                })
                || history
                    .windows
                    .windows(2)
                    .any(|pair| pair[0].ends_at > pair[1].starts_at)
            {
                return Err(StorageError::InvalidAssignment(
                    "invalid scoring focus history".into(),
                ));
            }
            *cached = Some(history);
        }
        let history = cached.as_ref().expect("focus history loaded");
        if history
            .windows
            .last()
            .is_some_and(|window| timestamp < window.ends_at)
        {
            return Ok(history.windows.clone());
        }
        let interval_start = Timestamp::from_timestamp(
            timestamp.timestamp().div_euclid(FOCUS_SECONDS) * FOCUS_SECONDS,
            0,
        )
        .expect("current UTC interval is representable");
        let metadata = self.load_dataset().await?;
        let mut pending = metadata
            .tasks
            .iter()
            .filter(|task| task.enabled)
            .map(|task| (task.task_id.clone(), 0_usize))
            .collect::<std::collections::BTreeMap<_, _>>();
        for image in metadata.images.keys() {
            let events = self.load_events(image).await?;
            // Replay a sequence prefix, never reorder/filter individual events.
            let cut = events
                .iter()
                .position(|event| event.timestamp >= interval_start)
                .unwrap_or(events.len());
            let state = rebuild_state(image.clone(), &events[..cut])?;
            for (task, count) in &mut pending {
                if state.included_in_completion_denominator(task)
                    && !state.task_states.get(task).is_some_and(|task| {
                        matches!(task.status, TaskStatus::Submitted | TaskStatus::Completed)
                    })
                {
                    *count += 1;
                }
            }
        }
        let maximum = pending.values().copied().max().unwrap_or_default();
        let previous = history
            .windows
            .last()
            .and_then(|window| window.task_id.clone());
        let task_id = if maximum == 0 {
            None
        } else {
            previous
                .filter(|task| pending.get(task) == Some(&maximum))
                .or_else(|| {
                    pending
                        .into_iter()
                        .find_map(|(task, count)| (count == maximum).then_some(task))
                })
        };
        let mut next = history.clone();
        next.windows.push(FocusWindow {
            // The first activation never invents bonuses for pre-deployment work.
            starts_at: if next.windows.is_empty() {
                timestamp
            } else {
                interval_start
            },
            ends_at: interval_start + std::time::Duration::from_secs(FOCUS_SECONDS as u64),
            task_id,
        });
        write_json_atomic(&path, &next).await?;
        let windows = next.windows.clone();
        *cached = Some(next);
        Ok(windows)
    }
}

#[cfg(test)]
mod tests;
