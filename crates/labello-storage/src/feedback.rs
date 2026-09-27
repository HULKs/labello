//! Root-wide durable acknowledgement and mandatory-feedback transaction owner.
use crate::error::PathIo;
use crate::{
    DatasetRepository, StorageError, StorageResult,
    fsjson::{read_json, write_json_atomic},
};
use labello_domain::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Clone, Debug)]
pub struct FeedbackStore {
    root: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
    repositories: Arc<parking_lot::Mutex<BTreeMap<DatasetId, Arc<DatasetRepository>>>>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    version: u32,
    baseline: BTreeMap<String, u64>,
    dismissed: BTreeMap<UserId, BTreeSet<EventId>>,
    viewed: BTreeMap<UserId, BTreeSet<EventId>>,
    mandatory: BTreeMap<UserId, BTreeSet<String>>,
    thresholds: BTreeMap<String, u32>,
}
fn workflow(dataset: &DatasetId, task: &TaskId) -> String {
    format!("{dataset}/{task}")
}
fn image_key(dataset: &DatasetId, image: &ImageId) -> String {
    format!("{dataset}/{image}")
}

impl FeedbackStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            lock: Arc::default(),
            repositories: Arc::default(),
        }
    }
    pub fn repository(
        &self,
        dataset: &DatasetId,
    ) -> Result<Arc<DatasetRepository>, IdValidationError> {
        dataset.validate_path_segment()?;
        Ok(self
            .repositories
            .lock()
            .entry(dataset.clone())
            .or_insert_with(|| Arc::new(DatasetRepository::new(self.root.join(dataset.as_str()))))
            .clone())
    }
    /// Hold through workflow validation and publication, including correction writes.
    pub async fn transaction(&self) -> OwnedMutexGuard<()> {
        self.lock.clone().lock_owned().await
    }
    fn path(&self) -> PathBuf {
        self.root.join(".labello-server/feedback-v1.json")
    }
    async fn save(&self, journal: &Journal) -> StorageResult<()> {
        write_json_atomic(&self.path(), journal).await
    }
    async fn repositories(&self) -> StorageResult<Vec<Arc<DatasetRepository>>> {
        if !tokio::fs::try_exists(self.root.as_ref())
            .await
            .with_path(self.root.as_ref())?
        {
            return Ok(Vec::new());
        }
        let mut entries = tokio::fs::read_dir(self.root.as_ref())
            .await
            .with_path(self.root.as_ref())?;
        let mut result = Vec::new();
        while let Some(entry) = entries.next_entry().await.with_path(self.root.as_ref())? {
            if !entry.file_type().await.with_path(entry.path())?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with('.')
                || DatasetId::from(name.clone())
                    .validate_path_segment()
                    .is_err()
            {
                continue;
            }
            let repo = self
                .repository(&DatasetId::from(name))
                .expect("validated dataset ID");
            if tokio::fs::try_exists(repo.dataset_path())
                .await
                .with_path(repo.dataset_path())?
            {
                result.push(repo);
            }
        }
        Ok(result)
    }
    async fn load(&self) -> StorageResult<Journal> {
        if tokio::fs::try_exists(self.path())
            .await
            .with_path(self.path())?
        {
            let journal: Journal = read_json(&self.path()).await?;
            if journal.version != 1 {
                return Err(StorageError::InvalidAssignment(
                    "unsupported feedback journal".into(),
                ));
            }
            return Ok(journal);
        }
        let mut journal = Journal {
            version: 1,
            ..Default::default()
        };
        // Capture existing history before admitting the first workflow write. No
        // timestamp guess and no retroactive notification flood on upgrade.
        for repo in self.repositories().await? {
            let metadata = repo.load_dataset_config().await?;
            for record in repo.load_images_index().await?.images_by_hash.values() {
                let image = &record.image_id;
                let events = repo.load_events(image).await?;
                journal.baseline.insert(
                    image_key(&metadata.dataset_id, image),
                    events.last().map_or(0, |e| e.event_sequence),
                );
            }
        }
        self.save(&journal).await?;
        Ok(journal)
    }
    async fn pending(
        &self,
        journal: &Journal,
        user: &UserId,
    ) -> StorageResult<Vec<CorrectionFeedback>> {
        let mut items = Vec::new();
        for repo in self.repositories().await? {
            let metadata = repo.load_dataset_config().await?;
            if !metadata.role_assignments.iter().any(|r| {
                r.dataset_id == metadata.dataset_id && &r.user_id == user && !r.roles.is_empty()
            }) {
                continue;
            }
            for record in repo.load_images_index().await?.images_by_hash.values() {
                let image = &record.image_id;
                // Removed originals cannot trap the user behind unreadable feedback.
                if !tokio::fs::try_exists(repo.image_path(&record.canonical_path)?)
                    .await
                    .with_path(repo.root())?
                {
                    continue;
                }
                let baseline = journal
                    .baseline
                    .get(&image_key(&metadata.dataset_id, image))
                    .copied()
                    .unwrap_or(0);
                let lock = repo.image_lock(image);
                let _guard = lock.lock().await;
                let cached = repo.feedback_cache.lock().get(image).cloned();
                let projected = if let Some(cached) = cached {
                    cached
                } else {
                    let events = repo.load_events(image).await?;
                    let mut projected = Vec::new();
                    for (index, event) in events.iter().enumerate().filter(|(_, e)| {
                        e.event_sequence > baseline
                            && matches!(e.payload, EventPayload::ReviewCorrectionSubmitted { .. })
                    }) {
                        let _ = event;
                        if let Some(item) = correction_feedback(&metadata, &events, index)? {
                            projected.push(item);
                        }
                    }
                    let projected = Arc::new(projected);
                    repo.feedback_cache
                        .lock()
                        .insert(image.clone(), projected.clone());
                    projected
                };
                for item in projected.iter() {
                    if item.recipients.contains(user)
                        && metadata.task(&item.summary.task_id).is_some()
                        && !journal
                            .dismissed
                            .get(user)
                            .is_some_and(|ids| ids.contains(&item.summary.event_id))
                    {
                        let mut item = item.clone();
                        item.summary.dataset_name = metadata.name.clone();
                        items.push(item);
                    }
                }
            }
        }
        items.sort_by(|a, b| {
            b.summary
                .timestamp
                .cmp(&a.summary.timestamp)
                .then_with(|| a.summary.event_id.cmp(&b.summary.event_id))
        });
        Ok(items)
    }
    async fn refresh(&self, user: &UserId) -> StorageResult<(Journal, Vec<CorrectionFeedback>)> {
        let mut journal = self.load().await?;
        let mut items = self.pending(&journal, user).await?;
        let mut counts = BTreeMap::<String, usize>::new();
        for item in &items {
            *counts
                .entry(workflow(&item.summary.dataset_id, &item.summary.task_id))
                .or_default() += 1;
        }
        let required: BTreeSet<_> = counts
            .iter()
            .filter(|(key, n)| {
                **n >= *journal
                    .thresholds
                    .get(*key)
                    .unwrap_or(&DEFAULT_FEEDBACK_THRESHOLD) as usize
            })
            .map(|(key, _)| key.clone())
            .collect();
        let mandatory = journal.mandatory.entry(user.clone()).or_default();
        let previous = mandatory.clone();
        mandatory.retain(|key| counts.contains_key(key));
        mandatory.extend(required);
        for item in &mut items {
            item.summary.mandatory =
                mandatory.contains(&workflow(&item.summary.dataset_id, &item.summary.task_id));
        }
        if *mandatory != previous {
            self.save(&journal).await?;
        }
        Ok((journal, items))
    }
    pub async fn inbox(&self, user: &UserId) -> StorageResult<Vec<FeedbackSummary>> {
        Ok(self
            .refresh(user)
            .await?
            .1
            .into_iter()
            .map(|item| item.summary)
            .collect())
    }
    pub async fn require_labeling_allowed(&self, user: &UserId) -> StorageResult<()> {
        if self.inbox(user).await?.iter().any(|item| item.mandatory) {
            return Err(StorageError::AssignmentConflict(
                "View mandatory correction feedback before continuing labeling".into(),
            ));
        }
        Ok(())
    }
    pub async fn detail(
        &self,
        user: &UserId,
        event: &EventId,
    ) -> StorageResult<CorrectionFeedback> {
        self.refresh(user)
            .await?
            .1
            .into_iter()
            .find(|item| &item.summary.event_id == event)
            .ok_or_else(|| StorageError::Unauthorized("feedback is unavailable".into()))
    }
    /// The transport calls this only after it has loaded the detail and preview.
    pub async fn acknowledge(
        &self,
        user: &UserId,
        event: &EventId,
        viewed: bool,
    ) -> StorageResult<()> {
        let (mut journal, items) = self.refresh(user).await?;
        if journal
            .dismissed
            .get(user)
            .is_some_and(|ids| ids.contains(event))
        {
            return Ok(());
        }
        let item = items
            .iter()
            .find(|item| &item.summary.event_id == event)
            .ok_or_else(|| StorageError::Unauthorized("feedback is unavailable".into()))?;
        if item.summary.mandatory
            && (!viewed
                || !journal
                    .viewed
                    .get(user)
                    .is_some_and(|ids| ids.contains(event)))
        {
            return Err(StorageError::AssignmentConflict(
                "Open this feedback before dismissing it".into(),
            ));
        }
        if viewed {
            journal
                .viewed
                .entry(user.clone())
                .or_default()
                .insert(event.clone());
        }
        journal
            .dismissed
            .entry(user.clone())
            .or_default()
            .insert(event.clone());
        let key = workflow(&item.summary.dataset_id, &item.summary.task_id);
        if !items.iter().any(|other| {
            other.summary.event_id != *event
                && workflow(&other.summary.dataset_id, &other.summary.task_id) == key
        }) {
            journal
                .mandatory
                .entry(user.clone())
                .or_default()
                .remove(&key);
        }
        self.save(&journal).await
    }
    pub async fn mark_presented(&self, user: &UserId, event: &EventId) -> StorageResult<()> {
        let (mut journal, items) = self.refresh(user).await?;
        if !items.iter().any(|item| &item.summary.event_id == event) {
            return Err(StorageError::Unauthorized("feedback is unavailable".into()));
        }
        journal
            .viewed
            .entry(user.clone())
            .or_default()
            .insert(event.clone());
        self.save(&journal).await
    }
    pub async fn observe_corrections(&self) -> StorageResult<()> {
        let mut users = BTreeSet::new();
        for repo in self.repositories().await? {
            for role in repo.load_dataset_config().await?.role_assignments {
                users.insert(role.user_id);
            }
        }
        for user in users {
            self.refresh(&user).await?;
        }
        Ok(())
    }
    pub async fn threshold(&self, dataset: &DatasetId, task: &TaskId) -> StorageResult<u32> {
        Ok(*self
            .load()
            .await?
            .thresholds
            .get(&workflow(dataset, task))
            .unwrap_or(&DEFAULT_FEEDBACK_THRESHOLD))
    }
    pub async fn set_threshold(
        &self,
        dataset: &DatasetId,
        task: &TaskId,
        threshold: u32,
    ) -> StorageResult<()> {
        if threshold == 0 {
            return Err(StorageError::InvalidAssignment(
                "feedback threshold must be positive".into(),
            ));
        }
        self.observe_corrections().await?;
        let mut journal = self.load().await?;
        journal
            .thresholds
            .insert(workflow(dataset, task), threshold);
        self.save(&journal).await
    }
}
