//! Process-local review history derived from committed per-image state.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use labello_domain::{
    Assignment, AssignmentId, AssignmentKind, AssignmentStatus, ImageId, ImageState, TaskId,
    Timestamp, UserId,
};
use parking_lot::Mutex;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, OwnedRwLockReadGuard, RwLock};

use crate::{DatasetRepository, StorageError, StorageResult};

type Key = (UserId, TaskId);
type Entry = (Timestamp, ImageId, AssignmentId);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    sequence: u64,
    finished: BTreeMap<Key, (Timestamp, AssignmentId)>,
}

impl Observation {
    fn from_state(state: &ImageState) -> Self {
        let mut finished = BTreeMap::new();
        for assignment in &state.assignments {
            if assignment.kind != AssignmentKind::Review
                || !(assignment.status == AssignmentStatus::Completed
                    || (assignment.status == AssignmentStatus::Cancelled
                        && assignment
                            .expires_at
                            .is_none_or(|expiry| expiry > assignment.updated_at)))
            {
                continue;
            }
            let key = (assignment.assigned_to.clone(), assignment.task_id.clone());
            let value = (assignment.updated_at, assignment.assignment_id.clone());
            finished
                .entry(key)
                .and_modify(|current| {
                    if value > *current {
                        *current = value.clone();
                    }
                })
                .or_insert(value);
        }
        Self {
            sequence: state.current_sequence,
            finished,
        }
    }
}

#[derive(Debug, Default)]
struct Latest {
    entries: BTreeSet<Entry>,
    // Two distinct images allow excluding the target without a search.
    first: Option<Entry>,
    second: Option<Entry>,
}

impl Latest {
    fn refresh(&mut self) {
        let mut entries = self.entries.iter().rev();
        self.first = entries.next().cloned();
        self.second = entries.next().cloned();
    }
}

#[derive(Debug, Default)]
struct Projection {
    images: HashMap<ImageId, Observation>,
    latest: HashMap<Key, Latest>,
}

impl Projection {
    fn observe(&mut self, image: ImageId, observation: Observation) {
        if self
            .images
            .get(&image)
            .is_some_and(|old| old.sequence > observation.sequence)
        {
            return;
        }
        if let Some(old) = self.images.remove(&image) {
            for (key, (timestamp, assignment)) in old.finished {
                if let Some(latest) = self.latest.get_mut(&key) {
                    latest
                        .entries
                        .remove(&(timestamp, image.clone(), assignment));
                    latest.refresh();
                    if latest.entries.is_empty() {
                        self.latest.remove(&key);
                    }
                }
            }
        }
        for (key, (timestamp, assignment)) in &observation.finished {
            let latest = self.latest.entry(key.clone()).or_default();
            latest
                .entries
                .insert((*timestamp, image.clone(), assignment.clone()));
            latest.refresh();
        }
        self.images.insert(image, observation);
    }
}

#[derive(Debug, Default)]
struct Inner {
    generation: u64,
    projection: Option<Projection>,
    pending: BTreeMap<ImageId, Observation>,
}

#[derive(Debug, Default)]
pub(crate) struct ReviewHistoryCache {
    inner: Mutex<Inner>,
    refresh: AsyncMutex<()>,
    keys: Mutex<BTreeMap<Key, Arc<AsyncMutex<()>>>>,
    pub(crate) membership: Arc<RwLock<()>>,
    #[cfg(test)]
    publish_pause: AsyncMutex<Option<Arc<ReviewHistoryPublishPause>>>,
    #[cfg(test)]
    commit_pause: AsyncMutex<Option<Arc<ReviewHistoryCommitPause>>>,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct ReviewHistoryPublishPause {
    pub(crate) scanned: Notify,
    pub(crate) resume: Notify,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct ReviewHistoryCommitPause {
    pub(crate) guards_acquired: Notify,
    pub(crate) resume: Notify,
}

impl ReviewHistoryCache {
    #[cfg(test)]
    pub(crate) async fn pause_before_publish(&self) -> Arc<ReviewHistoryPublishPause> {
        let pause = Arc::new(ReviewHistoryPublishPause::default());
        *self.publish_pause.lock().await = Some(pause.clone());
        pause
    }

    #[cfg(test)]
    pub(crate) async fn pause_after_commit_guards(&self) -> Arc<ReviewHistoryCommitPause> {
        let pause = Arc::new(ReviewHistoryCommitPause::default());
        *self.commit_pause.lock().await = Some(pause.clone());
        pause
    }

    pub(crate) fn invalidate(&self) {
        let mut inner = self.inner.lock();
        inner.generation = inner.generation.wrapping_add(1);
        inner.projection = None;
        inner.pending.clear();
    }

    fn observe(&self, image: ImageId, observation: Observation) {
        let mut inner = self.inner.lock();
        if let Some(projection) = inner.projection.as_mut() {
            if projection.images.contains_key(&image) {
                projection.observe(image, observation);
            }
        } else if inner
            .pending
            .get(&image)
            .is_none_or(|old| old.sequence <= observation.sequence)
        {
            inner.pending.insert(image, observation);
        }
    }

    fn check_previous(&self, source: &Assignment) -> StorageResult<()> {
        let inner = self.inner.lock();
        let projection = inner.projection.as_ref().ok_or_else(|| {
            StorageError::AssignmentConflict("review history is refreshing; retry Previous".into())
        })?;
        if !projection.images.contains_key(&source.image_id) {
            return Err(StorageError::AssignmentConflict(
                "previous review image is no longer in the dataset".into(),
            ));
        }
        if let Some(latest) = projection
            .latest
            .get(&(source.assigned_to.clone(), source.task_id.clone()))
        {
            let other = latest
                .first
                .as_ref()
                .filter(|entry| entry.1 != source.image_id)
                .or(latest.second.as_ref());
            if other.is_some_and(|entry| entry.0 > source.updated_at) {
                return Err(StorageError::AssignmentConflict(
                    "this is no longer the immediately previous review assignment".into(),
                ));
            }
        }
        Ok(())
    }
}

// Image locks precede these sorted key locks. No holder acquires another image
// lock or initializes the index. Keep the guard through publication/observation.
pub(crate) struct ReviewHistoryCommit {
    cache: Arc<ReviewHistoryCache>,
    image: ImageId,
    observation: Option<Observation>,
    _guards: Vec<OwnedMutexGuard<()>>,
    _membership: OwnedRwLockReadGuard<()>,
    uncertain: bool,
}

impl ReviewHistoryCommit {
    pub(crate) fn observe(mut self) {
        self.cache.observe(
            self.image.clone(),
            self.observation.take().expect("one observation"),
        );
        self.uncertain = false;
    }
}

impl Drop for ReviewHistoryCommit {
    fn drop(&mut self) {
        // An interrupted/erroring publication may already have renamed the log.
        // Never keep a possibly stale authorization index after that boundary.
        if self.uncertain {
            self.cache.invalidate();
        }
    }
}

impl DatasetRepository {
    pub(crate) async fn prepare_review_history(&self) -> StorageResult<()> {
        if self.review_history_cache.inner.lock().projection.is_some() {
            return Ok(());
        }
        let _refresh = self.review_history_cache.refresh.lock().await;
        loop {
            let generation = {
                let inner = self.review_history_cache.inner.lock();
                if inner.projection.is_some() {
                    return Ok(());
                }
                inner.generation
            };
            let index = self.load_images_index_shared().await?;
            let images = index
                .images_by_hash
                .values()
                .map(|record| record.image_id.clone())
                .collect::<BTreeSet<_>>();
            let mut remaining = images.iter().cloned();
            let mut workers = tokio::task::JoinSet::new();
            for image in remaining.by_ref().take(32) {
                let repo = self.clone();
                workers.spawn(async move {
                    let state = repo.load_image_state(&image).await?;
                    Ok::<_, StorageError>((image, Observation::from_state(&state)))
                });
            }
            let mut scanned = BTreeMap::new();
            while let Some(result) = workers.join_next().await {
                let (image, observation) = result.map_err(|_| {
                    StorageError::BackgroundTask("review history worker failed".into())
                })??;
                scanned.insert(image, observation);
                if let Some(image) = remaining.next() {
                    let repo = self.clone();
                    workers.spawn(async move {
                        let state = repo.load_image_state(&image).await?;
                        Ok::<_, StorageError>((image, Observation::from_state(&state)))
                    });
                }
            }
            #[cfg(test)]
            let publish_pause = self.review_history_cache.publish_pause.lock().await.take();
            #[cfg(test)]
            if let Some(pause) = publish_pause {
                pause.scanned.notify_one();
                pause.resume.notified().await;
            }
            let mut inner = self.review_history_cache.inner.lock();
            if inner.generation != generation {
                continue;
            }
            for (image, observation) in std::mem::take(&mut inner.pending) {
                if images.contains(&image)
                    && scanned
                        .get(&image)
                        .is_none_or(|old| old.sequence <= observation.sequence)
                {
                    scanned.insert(image, observation);
                }
            }
            let mut projection = Projection::default();
            for (image, observation) in scanned {
                projection.observe(image, observation);
            }
            inner.projection = Some(projection);
            return Ok(());
        }
    }

    pub(crate) async fn review_history_commit(
        &self,
        before: &ImageState,
        after: &ImageState,
        previous: Option<&Assignment>,
    ) -> StorageResult<ReviewHistoryCommit> {
        let membership = self
            .review_history_cache
            .membership
            .clone()
            .read_owned()
            .await;
        let old = Observation::from_state(before);
        let observation = Observation::from_state(after);
        let mut keys = old
            .finished
            .keys()
            .chain(observation.finished.keys())
            .filter(|key| old.finished.get(*key) != observation.finished.get(*key))
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(previous) = previous {
            keys.insert((previous.assigned_to.clone(), previous.task_id.clone()));
        }
        let locks = {
            let mut registry = self.review_history_cache.keys.lock();
            keys.into_iter()
                .map(|key| registry.entry(key).or_default().clone())
                .collect::<Vec<_>>()
        };
        let mut guards = Vec::new();
        for lock in locks {
            guards.push(lock.lock_owned().await);
        }
        #[cfg(test)]
        let commit_pause = self.review_history_cache.commit_pause.lock().await.take();
        #[cfg(test)]
        if let Some(pause) = commit_pause {
            pause.guards_acquired.notify_one();
            pause.resume.notified().await;
        }
        if let Some(previous) = previous {
            self.review_history_cache.check_previous(previous)?;
        }
        Ok(ReviewHistoryCommit {
            cache: self.review_history_cache.clone(),
            image: after.image_id.clone(),
            observation: Some(observation),
            uncertain: !guards.is_empty(),
            _guards: guards,
            _membership: membership,
        })
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use labello_domain::{
        Assignment, AssignmentId, AssignmentKind, AssignmentStatus, ImageId, ImageState, TaskId,
        UserId, now,
    };

    use super::*;

    fn assignment(
        image_id: &str,
        assignment_id: &str,
        task_id: &str,
        status: AssignmentStatus,
        updated_at: Timestamp,
        expires_at: Option<Timestamp>,
    ) -> Assignment {
        Assignment {
            assignment_id: AssignmentId::from(assignment_id),
            image_id: ImageId::from(image_id),
            task_id: TaskId::from(task_id),
            assigned_to: UserId::from("reviewer"),
            kind: AssignmentKind::Review,
            status,
            expires_at,
            created_at: updated_at,
            updated_at,
        }
    }

    fn observation(image_id: &str, sequence: u64, timestamp: Timestamp) -> (ImageId, Observation) {
        let mut state = ImageState::new(ImageId::from(image_id));
        state.current_sequence = sequence;
        state.assignments.push(assignment(
            image_id,
            &format!("assignment-{sequence}"),
            "task",
            AssignmentStatus::Completed,
            timestamp,
            None,
        ));
        (ImageId::from(image_id), Observation::from_state(&state))
    }

    #[test]
    fn observation_excludes_expiry_cleanup_but_keeps_intentional_cancellation() {
        let base = now();
        let mut state = ImageState::new(ImageId::from("img"));
        state.assignments = vec![
            assignment(
                "img",
                "completed",
                "task-completed",
                AssignmentStatus::Completed,
                base,
                None,
            ),
            assignment(
                "img",
                "expiry-cleanup",
                "task-completed",
                AssignmentStatus::Cancelled,
                base + std::time::Duration::from_secs(2),
                Some(base + std::time::Duration::from_secs(1)),
            ),
            assignment(
                "img",
                "intentional-skip",
                "task-skipped",
                AssignmentStatus::Cancelled,
                base + std::time::Duration::from_secs(3),
                Some(base + std::time::Duration::from_secs(60)),
            ),
        ];

        let observed = Observation::from_state(&state);
        assert_eq!(
            observed.finished[&(UserId::from("reviewer"), TaskId::from("task-completed"))].1,
            AssignmentId::from("completed")
        );
        assert_eq!(
            observed.finished[&(UserId::from("reviewer"), TaskId::from("task-skipped"))].1,
            AssignmentId::from("intentional-skip")
        );
    }

    #[test]
    fn projection_tracks_latest_two_images_and_allows_equal_timestamp_ties() {
        let base = now();
        let mut projection = Projection::default();
        for index in 0..1_000 {
            let timestamp = base + std::time::Duration::from_secs(index);
            let (image, observed) = observation(&format!("img-{index:04}"), index + 1, timestamp);
            projection.observe(image, observed);
        }

        let key = (UserId::from("reviewer"), TaskId::from("task"));
        let latest = projection.latest.get(&key).expect("observed key");
        assert_eq!(
            latest.first.as_ref().map(|entry| entry.1.clone()),
            Some(ImageId::from("img-0999"))
        );
        assert_eq!(
            latest.second.as_ref().map(|entry| entry.1.clone()),
            Some(ImageId::from("img-0998"))
        );

        let tie = base + std::time::Duration::from_secs(2_000);
        let (first_image, first_observation) = observation("tie-a", 2_000, tie);
        let (second_image, second_observation) = observation("tie-b", 2_001, tie);
        let mut tied = Projection::default();
        tied.observe(first_image, first_observation);
        tied.observe(second_image, second_observation);
        tied.images.insert(
            ImageId::from("source"),
            Observation {
                sequence: 2_002,
                finished: BTreeMap::new(),
            },
        );
        let cache = ReviewHistoryCache {
            inner: Mutex::new(Inner {
                projection: Some(tied),
                ..Inner::default()
            }),
            ..ReviewHistoryCache::default()
        };
        let source = assignment(
            "source",
            "source-assignment",
            "task",
            AssignmentStatus::Cancelled,
            tie,
            None,
        );
        assert!(cache.check_previous(&source).is_ok());
    }
}
