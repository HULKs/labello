use super::*;

impl PrelabelService {
    /// Reconciles admin-selected preparation and queues bounded server jobs for
    /// boxes and poses. No inference is performed in the request itself.
    pub async fn synchronize_workflows(
        &self,
        dataset: &DatasetId,
        repo: DatasetRepository,
        force: bool,
    ) -> Result<()> {
        let metadata = repo
            .load_dataset()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let identity = digest(&(
            &metadata.tasks,
            &metadata.prelabel_configs,
            metadata.images.len(),
        ))?;
        let mut control = self.lock(dataset).await?;
        if !force
            && self
                .inner
                .managed_sync
                .lock()
                .await
                .get(dataset)
                .is_some_and(|(at, key)| {
                    key == &identity && at.elapsed() < std::time::Duration::from_secs(5)
                })
        {
            return Ok(());
        }
        self.prune(dataset, &mut control).await?;
        let mut items = Vec::new();
        for task in metadata.tasks.iter().filter(|t| t.enabled) {
            let config = workflow_prelabel_config(&metadata, task);
            let model_digest = match config {
                Some(config) => self
                    .model(config)
                    .await
                    .map(|bytes| blake3::hash(&bytes).to_hex().to_string()),
                None => Ok(String::new()),
            };
            for record in metadata.images.values() {
                let state = repo
                    .workflow_polling_state(&record.image_id, metadata.bounding_box_visibility)
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
                if !prelabel_task_eligible(task, &state)
                    || state.workflow_item_seen(&task.task_id, &WorkflowItem::Overview)
                {
                    continue;
                }
                let Some(config) = config else {
                    if state
                        .workflow_preparations
                        .get(&task.task_id)
                        .is_none_or(|p| {
                            p.config_digest.is_none()
                                && p.status == WorkflowPreparationStatus::Ready
                        })
                    {
                        // Model-free sources are prepared lazily by the claim transaction.
                        continue;
                    }
                    repo.prepare_workflow_predictions(
                        &record.image_id,
                        &task.task_id,
                        None,
                        None,
                        WorkflowPreparationStatus::Ready,
                        vec![],
                    )
                    .await
                    .map_err(|_| PrelabelFailure::Stale)?;
                    continue;
                };
                let item = WorkItem {
                    image_id: record.image_id.clone(),
                    image_hash: record.blake3.clone(),
                    task_id: task.task_id.clone(),
                    config_id: config.config_id.clone(),
                    config_digest: digest(&(task, config))?,
                    model_digest: model_digest
                        .clone()
                        .unwrap_or_else(|_| "unavailable".into()),
                    generation: Self::generation(&control, &task.task_id, &config.config_id),
                    outcome: if model_digest.is_ok() {
                        PrelabelItemOutcome::Pending
                    } else {
                        PrelabelItemOutcome::Failed
                    },
                };
                let key = predictions::result_key(&item)?;
                let previous = state.workflow_preparations.get(&task.task_id);
                if previous.is_some_and(|p| {
                    p.generation_key.as_ref() == Some(&key)
                        && matches!(
                            p.status,
                            WorkflowPreparationStatus::Ready | WorkflowPreparationStatus::Failed
                        )
                }) {
                    continue;
                }
                if state.assignments.iter().any(|a| {
                    a.task_id == task.task_id
                        && !state.workflow_assignments.contains_key(&a.assignment_id)
                }) {
                    self.publish_managed(&repo, &item, WorkflowPreparationStatus::Ready, vec![])
                        .await?;
                    continue;
                }
                if let Some(retained) = self.retained_response(dataset, &control, &item).await? {
                    self.publish_managed(
                        &repo,
                        &item,
                        WorkflowPreparationStatus::Ready,
                        retained.suggestions,
                    )
                    .await?;
                    continue;
                }
                if items.len() >= self.inner.limits.max_work_items {
                    // Leave the rest for the next bounded maintenance pass.
                    continue;
                }
                self.publish_managed(
                    &repo,
                    &item,
                    if item.generation.paused {
                        WorkflowPreparationStatus::Ready
                    } else if model_digest.is_ok() {
                        WorkflowPreparationStatus::Pending
                    } else {
                        WorkflowPreparationStatus::Failed
                    },
                    vec![],
                )
                .await?;
                if item.generation.paused
                    || control
                        .runs
                        .iter()
                        .filter(|r| {
                            matches!(
                                r.summary.phase,
                                PrelabelRunPhase::Ready | PrelabelRunPhase::Running
                            )
                        })
                        .any(|r| {
                            r.items
                                .iter()
                                .any(|old| predictions::result_key(old).is_ok_and(|old| old == key))
                        })
                {
                    continue;
                }
                items.push(item);
            }
        }
        if !items.is_empty() {
            let mut next = control.clone();
            if next.runs.len() >= self.inner.limits.max_retained_runs {
                if let Some(index) = next.runs.iter().position(|r| {
                    !matches!(
                        r.summary.phase,
                        PrelabelRunPhase::Ready | PrelabelRunPhase::Running
                    )
                }) {
                    next.runs.remove(index);
                } else {
                    return Err(PrelabelFailure::Limit);
                }
            }
            let timestamp = now();
            let mut run = Run {
                managed: true,
                summary: PrelabelRunSummary {
                    run_id: format!("managed-{}", uuid::Uuid::new_v4()),
                    phase: PrelabelRunPhase::Ready,
                    created_at: timestamp,
                    updated_at: timestamp,
                    total: items.len(),
                    pending: 0,
                    reusable: 0,
                    ineligible: 0,
                    generated: 0,
                    empty: 0,
                    skipped: 0,
                    failed: 0,
                    blockers: vec![],
                },
                items,
            };
            runs::update_counts(&mut run);
            next.runs.push(run);
            self.commit(dataset, &mut control, next).await?;
        }
        let start = if control
            .runs
            .iter()
            .any(|r| r.summary.phase == PrelabelRunPhase::Running)
        {
            None
        } else {
            control
                .runs
                .iter()
                .find(|r| r.managed && r.summary.phase == PrelabelRunPhase::Ready)
                .map(|r| r.summary.run_id.clone())
        };
        drop(control);
        if let Some(run_id) = start {
            self.start(dataset, repo, &run_id, false).await?;
        }
        self.inner
            .managed_sync
            .lock()
            .await
            .insert(dataset.clone(), (std::time::Instant::now(), identity));
        Ok(())
    }

    pub(super) async fn revalidate_managed(
        &self,
        repo: &DatasetRepository,
        item: &WorkItem,
    ) -> Result<()> {
        let metadata = repo
            .load_dataset_config()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let task = metadata.task(&item.task_id).ok_or(PrelabelFailure::Stale)?;
        if workflow_prelabel_config(&metadata, task).is_none_or(|c| c.config_id != item.config_id) {
            return Err(PrelabelFailure::Stale);
        }
        let state = repo
            .load_image_state(&item.image_id)
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        if state.workflow_item_seen(&item.task_id, &WorkflowItem::Overview) {
            return Err(PrelabelFailure::Stale);
        }
        Ok(())
    }

    pub(super) async fn publish_managed(
        &self,
        repo: &DatasetRepository,
        item: &WorkItem,
        status: WorkflowPreparationStatus,
        suggestions: Vec<PrelabelSuggestion>,
    ) -> Result<()> {
        repo.prepare_workflow_predictions(
            &item.image_id,
            &item.task_id,
            Some(item.config_digest.clone()),
            Some(predictions::result_key(item)?),
            status,
            suggestions,
        )
        .await
        .map_err(|error| match error {
            crate::StorageError::AssignmentConflict(_) => PrelabelFailure::Stale,
            _ => PrelabelFailure::Storage,
        })?;
        Ok(())
    }
}
