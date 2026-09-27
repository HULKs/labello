use labello_domain::{
    EventLogEntry, EventPayload, FocusWindow, TaskOutcome, TaskStatus, Timestamp, rebuild_state,
    stats::scoring::FOCUS_SECONDS,
};
use serde::{Deserialize, Serialize};

use crate::{
    DatasetRepository, StorageError, StorageResult,
    error::PathIo,
    fsjson::{read_json, write_json_atomic},
};

const FOCUS_HISTORY_VERSION: u32 = 2;
const LEGACY_FOCUS_SECONDS: i64 = 20 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusHistory {
    version: u32,
    pub(super) windows: Vec<FocusWindow>,
    #[serde(default)]
    pub(super) review_windows: Vec<FocusWindow>,
}

impl DatasetRepository {
    pub(crate) async fn prepare_scoring_focus(
        &self,
        events: &[EventLogEntry],
    ) -> StorageResult<()> {
        let reviewing = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventPayload::ReviewRecorded { .. }
                    | EventPayload::ReviewerCorrectionRecorded { .. }
                    | EventPayload::ReviewCorrectionSubmitted { .. }
                    | EventPayload::ReviewRevisionCommitted { .. }
            )
        });
        let submitting = events.iter().any(|event| {
            matches!(&event.payload,
                EventPayload::TaskStateChanged { task_state }
                    if task_state.completed_by.is_some()
                        && (task_state.status == TaskStatus::Submitted
                            || (task_state.status == TaskStatus::Completed
                                && task_state.outcome == Some(TaskOutcome::AnnotationCompleted)))
            )
        });
        if reviewing {
            self.review_scoring_focus(events.last().expect("review batch").timestamp)
                .await?;
        } else if submitting {
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
        self.focus_for(timestamp, false).await
    }

    pub(crate) async fn review_scoring_focus(
        &self,
        timestamp: Timestamp,
    ) -> StorageResult<Vec<FocusWindow>> {
        // Upgrade any active legacy annotation period before writing the shared v2 file.
        self.scoring_focus(timestamp).await?;
        self.focus_for(timestamp, true).await
    }

    async fn focus_for(
        &self,
        timestamp: Timestamp,
        reviewing: bool,
    ) -> StorageResult<Vec<FocusWindow>> {
        let mut cached = self.scoring.lock().await;
        let path = self.root().join(".labello/scoring/focus-v1.json");
        if cached.is_none() {
            let history: FocusHistory = if tokio::fs::try_exists(&path).await.with_path(&path)? {
                read_json(&path).await?
            } else {
                FocusHistory {
                    version: FOCUS_HISTORY_VERSION,
                    windows: Vec::new(),
                    review_windows: Vec::new(),
                }
            };
            if !matches!(history.version, 1 | FOCUS_HISTORY_VERSION)
                || (history.version == 1 && !history.review_windows.is_empty())
                || history
                    .windows
                    .iter()
                    .chain(&history.review_windows)
                    .any(|window| {
                        window.starts_at >= window.ends_at
                            || (history.version == 1
                                && window.ends_at.timestamp().rem_euclid(LEGACY_FOCUS_SECONDS) != 0)
                            || (window.ends_at - window.starts_at)
                                .to_std()
                                .is_ok_and(|duration| {
                                    duration
                                        > std::time::Duration::from_secs(
                                            LEGACY_FOCUS_SECONDS as u64,
                                        )
                                })
                    })
                || history
                    .windows
                    .windows(2)
                    .chain(history.review_windows.windows(2))
                    .any(|pair| pair[0].ends_at > pair[1].starts_at)
            {
                return Err(StorageError::InvalidAssignment(
                    "invalid scoring focus history".into(),
                ));
            }
            *cached = Some(history);
        }
        let history = cached.as_ref().expect("focus history loaded");
        let windows = if reviewing {
            &history.review_windows
        } else {
            &history.windows
        };
        // Historical/offline timestamps must not rewrite already recorded selections.
        if windows
            .last()
            .is_some_and(|window| timestamp <= window.starts_at)
        {
            return Ok(windows.clone());
        }
        let metadata = self.load_dataset().await?;
        let enabled = metadata
            .tasks
            .iter()
            .filter(|task| task.enabled)
            .map(|task| task.task_id.clone())
            .collect::<Vec<_>>();
        let blocked =
            if let Some(imbalance) = metadata.imbalance.as_ref().filter(|config| config.enforce) {
                let counts = if reviewing {
                    self.task_completion_counts().await?
                } else {
                    self.task_annotation_counts().await?
                };
                imbalance.blocked_tasks(&enabled, &counts)
            } else {
                Default::default()
            };
        let eligible = enabled
            .into_iter()
            .filter(|task_id| {
                !blocked.contains(task_id)
                    && (!reviewing
                        || metadata.task(task_id).is_some_and(|task| {
                            task.review.workflow == labello_domain::ReviewWorkflow::Approval
                        }))
            })
            .collect::<Vec<_>>();
        if history.version == FOCUS_HISTORY_VERSION
            && windows.last().is_some_and(|window| {
                timestamp < window.ends_at
                    && window
                        .task_id
                        .as_ref()
                        .is_none_or(|task| eligible.contains(task))
            })
        {
            return Ok(windows.clone());
        }
        let mut pending = eligible
            .into_iter()
            .map(|task| (task, 0_usize))
            .collect::<std::collections::BTreeMap<_, _>>();
        for image in metadata.images.keys() {
            let events = self.load_events(image).await?;
            // Replay a sequence prefix, never reorder/filter individual events.
            let cut = events
                .iter()
                .position(|event| event.timestamp >= timestamp)
                .unwrap_or(events.len());
            let state = rebuild_state(image.clone(), &events[..cut])?;
            for (task, count) in &mut pending {
                let status = state.task_states.get(task).map(|task| &task.status);
                let awaiting = if reviewing {
                    status == Some(&TaskStatus::Submitted)
                } else {
                    !matches!(status, Some(TaskStatus::Submitted | TaskStatus::Completed))
                };
                if state.included_in_completion_denominator(task) && awaiting {
                    *count += 1;
                }
            }
        }
        let maximum = pending.values().copied().max().unwrap_or_default();
        let previous = windows.last().and_then(|window| window.task_id.clone());
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
        next.version = FOCUS_HISTORY_VERSION;
        let windows = if reviewing {
            &mut next.review_windows
        } else {
            &mut next.windows
        };
        if let Some(previous) = windows.last_mut()
            && previous.ends_at > timestamp
        {
            // Early imbalance switches and v1 upgrades preserve earned past focus.
            previous.ends_at = timestamp;
        }
        windows.push(FocusWindow {
            starts_at: timestamp,
            ends_at: timestamp + std::time::Duration::from_secs(FOCUS_SECONDS as u64),
            task_id,
        });
        let windows = windows.clone();
        write_json_atomic(&path, &next).await?;
        *cached = Some(next);
        Ok(windows)
    }
}

#[cfg(test)]
mod tests;
