use super::*;

impl DatasetRepository {
    pub async fn initialize(&self, mut metadata: DatasetMetadata) -> StorageResult<()> {
        self.ensure_layout().await?;
        metadata.schema_version = SCHEMA_VERSION;
        metadata.updated_at = now();
        self.create_dataset(&metadata).await?;
        self.save_images_index(&ImagesIndex::default()).await?;
        write_json_atomic(&self.schema_path(), &labello_schema_bundle()).await?;
        Ok(())
    }

    async fn create_dataset(&self, metadata: &DatasetMetadata) -> StorageResult<()> {
        labello_domain::validate_schema_version(metadata.schema_version)?;
        validate_current_review_config(metadata)?;
        let path = self.dataset_path();
        let text = toml::to_string_pretty(&DatasetConfig::from_metadata(metadata))
            .with_toml_encode_path(&path)?;
        let mut file = match tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StorageError::AlreadyExists(path));
            }
            Err(source) => return Err(StorageError::Io { path, source }),
        };
        file.write_all(text.as_bytes()).await.with_path(&path)?;
        if !text.ends_with('\n') {
            file.write_all(b"\n").await.with_path(&path)?;
        }
        file.sync_all().await.with_path(&path)?;
        Ok(())
    }

    pub async fn load_dataset(&self) -> StorageResult<DatasetMetadata> {
        self.ensure_artifact_migration().await?;
        let config: DatasetConfig = read_current_toml(&self.dataset_path()).await?;
        let images = self
            .load_images_index()
            .await?
            .images_by_hash
            .into_values()
            .map(|record| (record.image_id.clone(), record))
            .collect();
        Ok(config.into_metadata(images))
    }

    pub async fn load_dataset_config(&self) -> StorageResult<DatasetMetadata> {
        self.ensure_artifact_migration().await?;
        let config: DatasetConfig = read_current_toml(&self.dataset_path()).await?;
        Ok(config.into_metadata(BTreeMap::new()))
    }

    pub async fn save_dataset(&self, metadata: &DatasetMetadata) -> StorageResult<()> {
        self.ensure_artifact_migration().await?;
        let _review_config_guard = self.review_config_lock.write().await;
        labello_domain::validate_schema_version(metadata.schema_version)?;
        validate_current_review_config(metadata)?;
        write_toml_atomic(
            &self.dataset_path(),
            &DatasetConfig::from_metadata(metadata),
        )
        .await?;
        self.stats_cache.invalidate();
        self.assignment_availability_cache.invalidate();
        Ok(())
    }

    pub async fn load_images_index(&self) -> StorageResult<ImagesIndex> {
        Ok(self.load_images_index_shared().await?.as_ref().clone())
    }

    pub(crate) async fn load_images_index_shared(&self) -> StorageResult<Arc<ImagesIndex>> {
        self.ensure_artifact_migration().await?;
        if let Some(index) = self.images_index_cache.read().await.as_ref() {
            return Ok(index.clone());
        }
        let mut cached = self.images_index_cache.write().await;
        if let Some(index) = cached.as_ref() {
            return Ok(index.clone());
        }
        let path = self.images_index_path();
        let index = if tokio::fs::try_exists(&path).await.with_path(&path)? {
            #[cfg(test)]
            self.images_index_loads.fetch_add(1, Ordering::Relaxed);
            read_current_json(&path).await?
        } else {
            ImagesIndex::default()
        };
        let index = Arc::new(index);
        *cached = Some(index.clone());
        Ok(index)
    }

    pub async fn image_count(&self) -> StorageResult<usize> {
        if !tokio::fs::try_exists(self.images_index_path())
            .await
            .with_path(self.images_index_path())?
        {
            return Ok(0);
        }
        if let Some(count) = self.read_image_count_hint().await? {
            return Ok(count);
        }
        Ok(self.load_images_index().await?.images_by_hash.len())
    }

    pub async fn load_image_record(&self, image_id: &ImageId) -> StorageResult<ImageRecord> {
        self.load_images_index_shared()
            .await?
            .images_by_hash
            .values()
            .find(|record| &record.image_id == image_id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(self.images_index_path()))
    }

    pub async fn save_images_index(&self, index: &ImagesIndex) -> StorageResult<()> {
        self.ensure_artifact_migration().await?;
        labello_domain::validate_schema_version(index.schema_version)?;
        let mut index = index.clone();
        index.image_count = index.images_by_hash.len();
        let _history_membership = self.review_history_cache.membership.write().await;
        let mut cached = self.images_index_cache.write().await;
        let previous = if let Some(previous) = cached.as_ref() {
            previous.clone()
        } else {
            let path = self.images_index_path();
            Arc::new(if tokio::fs::try_exists(&path).await.with_path(&path)? {
                read_current_json(&path).await?
            } else {
                ImagesIndex::default()
            })
        };
        let previous_image_ids = previous
            .images_by_hash
            .values()
            .map(|record| record.image_id.clone())
            .collect::<BTreeSet<_>>();
        let next_image_ids = index
            .images_by_hash
            .values()
            .map(|record| record.image_id.clone())
            .collect::<BTreeSet<_>>();
        // Publication may rename the index before failing or being cancelled.
        // Invalidate while membership is locked, before crossing that boundary.
        if previous_image_ids != next_image_ids {
            self.review_history_cache.invalidate();
        }
        *cached = None;
        write_json_atomic(&self.images_index_path(), &index).await?;
        *cached = Some(Arc::new(index));
        if previous_image_ids != next_image_ids {
            self.task_completion_cache
                .invalidate_membership("image_index_membership_changed");
        }
        self.stats_cache.invalidate();
        self.assignment_availability_cache.invalidate();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn images_index_load_count(&self) -> u64 {
        self.images_index_loads.load(Ordering::Relaxed)
    }

    async fn read_image_count_hint(&self) -> StorageResult<Option<usize>> {
        let path = self.images_index_path();
        let mut file = tokio::fs::File::open(&path).await.with_path(&path)?;
        let mut buffer = vec![0; 4096];
        let read = file.read(&mut buffer).await.with_path(&path)?;
        let prefix = String::from_utf8_lossy(&buffer[..read]);
        Ok(extract_image_count_hint(&prefix))
    }
}

pub(super) fn extract_image_count_hint(text: &str) -> Option<usize> {
    let key = "\"imageCount\"";
    let rest = text.get(text.find(key)? + key.len()..)?;
    let rest = rest.get(rest.find(':')? + 1..)?.trim_start();
    let digits = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn validate_current_review_config(metadata: &DatasetMetadata) -> StorageResult<()> {
    if metadata.tasks.iter().any(|task| !task.review.is_current())
        || metadata.role_assignments.iter().any(|assignment| {
            assignment
                .roles
                .contains(&labello_domain::DatasetRole::LegacyAdjudicator)
        })
    {
        return Err(StorageError::InvalidAssignment(
            "unsupported review configuration or role".into(),
        ));
    }
    Ok(())
}

impl DatasetRepository {
    /// Runs under the artifact-migration gate before current commands can load configuration.
    pub(crate) async fn upgrade_review_policy(&self) -> StorageResult<()> {
        use labello_domain::{
            AssignmentKind, AssignmentStatus, DatasetRole, EventPayload, ReviewDecision,
            TaskOutcome, TaskStatus, UserId,
        };
        let mut config: DatasetConfig = read_current_toml(&self.dataset_path()).await?;
        if config.review_policy_version == 1 {
            validate_current_review_config(&config.into_metadata(BTreeMap::new()))?;
            return Ok(());
        }
        if config.review_policy_version != 0 {
            return Err(StorageError::InvalidAssignment(
                "unsupported review policy version".into(),
            ));
        }
        let index: ImagesIndex = if tokio::fs::try_exists(self.images_index_path())
            .await
            .with_path(self.images_index_path())?
        {
            read_current_json(&self.images_index_path()).await?
        } else {
            ImagesIndex::default()
        };
        for image in index.images_by_hash.values() {
            let history = self.load_events(&image.image_id).await?;
            let mut state = rebuild_state(image.image_id.clone(), &history)?;
            let timestamp = now();
            let mut payloads = Vec::new();
            for task in &config.tasks {
                let Some(previous) = state.task_states.get(&task.task_id) else {
                    continue;
                };
                let mut next = previous.clone();
                if previous.status == TaskStatus::LegacyAdjudicationRequired {
                    next.status = TaskStatus::NeedsCorrection;
                    next.outcome = None;
                    next.assigned_to = None;
                    next.completed_by = None;
                    next.completed_at = None;
                } else if previous.status == TaskStatus::Submitted {
                    let targets = state.review_targets(task).ok();
                    let finals = state
                        .effective_reviews_for_task(&task.task_id)
                        .filter(|review| {
                            matches!(
                                review.target,
                                labello_domain::ReviewTarget::Task { .. }
                                    | labello_domain::ReviewTarget::MigrationConfirmation { .. }
                            )
                        })
                        .collect::<Vec<_>>();
                    if !finals.is_empty() {
                        if finals
                            .iter()
                            .any(|review| review.decision == ReviewDecision::Rejected)
                        {
                            next.status = TaskStatus::NeedsCorrection;
                            next.outcome = None;
                        } else if let Some(final_review) = finals.iter().rev().find(|review| {
                            targets.as_ref().is_some_and(|targets| {
                                targets.iter().all(|target| {
                                    state
                                        .effective_review_for_target(
                                            &task.task_id,
                                            target,
                                            &review.reviewer_user_id,
                                        )
                                        .is_some_and(|decision| {
                                            decision.decision == ReviewDecision::Approved
                                        })
                                })
                            })
                        }) {
                            next.status = TaskStatus::Completed;
                            next.outcome = Some(TaskOutcome::Approved);
                            next.completed_by = Some(final_review.reviewer_user_id.clone());
                            next.completed_at = Some(final_review.timestamp);
                        } else {
                            // A historical final row without all current object decisions cannot
                            // prove completion. A fresh submission round permits the same reviewer.
                            next.outcome = None;
                            next.completed_at = Some(timestamp);
                        }
                        next.assigned_to = None;
                    }
                }
                if next != *previous {
                    next.updated_at = timestamp;
                    payloads.push(EventPayload::TaskStateChanged { task_state: next });
                }
            }
            for assignment in &state.assignments {
                let old_review = assignment.kind == AssignmentKind::Review
                    && (config.tasks.iter().any(|task| {
                        task.task_id == assignment.task_id && !task.review.is_current()
                    }) || state
                        .review_assignment_contexts
                        .get(&assignment.assignment_id)
                        .is_some_and(|context| !context.task.review.is_current()));
                if assignment.status == AssignmentStatus::Active
                    && (assignment.kind == AssignmentKind::LegacyAdjudication || old_review)
                {
                    let mut cancelled = assignment.clone();
                    cancelled.status = AssignmentStatus::Cancelled;
                    cancelled.updated_at = timestamp;
                    cancelled.expires_at = Some(timestamp);
                    payloads.push(EventPayload::AssignmentUpdated {
                        assignment: cancelled,
                    });
                    if state
                        .review_assignment_contexts
                        .contains_key(&assignment.assignment_id)
                        && !state
                            .review_finished_sequences
                            .contains_key(&assignment.assignment_id)
                    {
                        payloads.push(EventPayload::ReviewAssignmentFinished {
                            assignment_id: assignment.assignment_id.clone(),
                            task_id: assignment.task_id.clone(),
                        });
                    }
                }
            }
            let mut appended = Vec::new();
            for payload in payloads {
                let event = EventLogEntry::new(
                    state.current_sequence + 1,
                    image.image_id.clone(),
                    UserId::from("system_review_upgrade"),
                    DatasetRole::DataAdmin,
                    timestamp,
                    payload,
                );
                state.apply_event(&event)?;
                appended.push(event);
            }
            self.append_events_atomic(&image.image_id, &appended)
                .await?;
            write_json_atomic(&self.state_path(&image.image_id), &state).await?;
        }
        for task in &mut config.tasks {
            task.review.upgrade();
        }
        for assignment in &mut config.role_assignments {
            assignment.roles.remove(&DatasetRole::LegacyAdjudicator);
        }
        config
            .role_assignments
            .retain(|assignment| !assignment.roles.is_empty());
        write_json_atomic(&self.schema_path(), &labello_schema_bundle()).await?;
        // Publishing configuration last makes every interrupted prefix safely resumable.
        config.review_policy_version = 1;
        write_toml_atomic(&self.dataset_path(), &config).await?;
        self.stats_cache.invalidate();
        self.assignment_availability_cache.invalidate();
        self.task_completion_cache
            .invalidate("review_policy_upgrade");
        Ok(())
    }
}
