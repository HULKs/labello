use std::{collections::BTreeMap, sync::atomic::Ordering};

use labello_domain::{DailyActivityCounts, UserId, UtcActivityWindow, daily_activity_from_events};

use crate::{DatasetRepository, StorageError, StorageResult};

#[derive(Debug)]
pub(super) struct CachedActivity {
    generation: u64,
    window: UtcActivityWindow,
    users: BTreeMap<UserId, DailyActivityCounts>,
}

impl DatasetRepository {
    pub async fn daily_activity(
        &self,
        user_id: &UserId,
        window: UtcActivityWindow,
    ) -> StorageResult<DailyActivityCounts> {
        // One day and one generation are cached, independently of the number of
        // callers. The same commit invalidation owns both statistics projections.
        let _refresh = self.stats_cache.activity_refresh.lock().await;
        let generation = self.stats_cache.generation.load(Ordering::Acquire);
        if let Some(cached) = self.stats_cache.activity.lock().await.as_ref()
            && cached.generation == generation
            && cached.window == window
        {
            return Ok(cached.users.get(user_id).copied().unwrap_or_default());
        }
        let metadata = self.load_dataset().await?;
        let mut image_ids = metadata.images.keys().cloned();
        let mut workers = tokio::task::JoinSet::new();
        let scan = |image_id| {
            let repository = self.clone();
            async move {
                let events = repository.load_events(&image_id).await?;
                labello_domain::rebuild_state(image_id, &events)?;
                Ok::<_, StorageError>(daily_activity_from_events(&events, window))
            }
        };
        for image_id in image_ids.by_ref().take(32) {
            workers.spawn(scan(image_id));
        }
        let mut users = BTreeMap::<UserId, DailyActivityCounts>::new();
        while let Some(result) = workers.join_next().await {
            let image_counts = result.map_err(|error| {
                StorageError::BackgroundTask(format!("daily activity worker failed: {error}"))
            })??;
            for (user, counts) in image_counts {
                let total = users.entry(user).or_default();
                total.annotation_tasks_submitted += counts.annotation_tasks_submitted;
                total.final_task_reviews += counts.final_task_reviews;
            }
            if let Some(image_id) = image_ids.next() {
                workers.spawn(scan(image_id));
            }
        }
        let counts = users.get(user_id).copied().unwrap_or_default();
        *self.stats_cache.activity.lock().await = Some(CachedActivity {
            generation,
            window,
            users,
        });
        // Concurrent commits invalidate this generation. Return the bounded scan;
        // the next request refreshes instead of waiting indefinitely for quiet.
        Ok(counts)
    }
}

#[cfg(test)]
mod tests;
