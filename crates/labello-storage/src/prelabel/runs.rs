use super::*;
use predictions::{certify, result_key};

impl PrelabelService {
    pub async fn admin_state(&self, dataset: &DatasetId) -> Result<PrelabelAdminState> {
        let mut control = self.lock(dataset).await?;
        self.prune(dataset, &mut control).await?;
        Ok(admin_state(&control))
    }

    pub async fn command(
        &self,
        dataset: &DatasetId,
        repo: DatasetRepository,
        command: PrelabelAdminCommand,
    ) -> Result<PrelabelAdminState> {
        match command {
            PrelabelAdminCommand::Preflight { mappings } => {
                self.preflight(dataset, &repo, mappings).await?
            }
            PrelabelAdminCommand::Start { run_id } => {
                self.start(dataset, repo, &run_id, false).await?
            }
            PrelabelAdminCommand::Retry { run_id } => {
                self.start(dataset, repo, &run_id, true).await?
            }
            PrelabelAdminCommand::Cancel { run_id } => {
                let mut control = self.lock(dataset).await?;
                let mut next = control.clone();
                let run = next
                    .runs
                    .iter_mut()
                    .find(|run| run.summary.run_id == run_id)
                    .ok_or(PrelabelFailure::NotFound)?;
                if matches!(
                    run.summary.phase,
                    PrelabelRunPhase::Running
                        | PrelabelRunPhase::Ready
                        | PrelabelRunPhase::Interrupted
                ) {
                    run.summary.phase = PrelabelRunPhase::Cancelled;
                    run.summary.updated_at = now();
                    self.commit(dataset, &mut control, next).await?;
                    if let Some(cancel) = self.inner.running.lock().await.get(&run_id) {
                        let _ = cancel.send(true);
                    }
                }
            }
            PrelabelAdminCommand::Reset { scope } => {
                self.change_scope(dataset, &repo, scope, true).await?
            }
            PrelabelAdminCommand::Resume { scope } => {
                self.change_scope(dataset, &repo, scope, false).await?
            }
        }
        self.admin_state(dataset).await
    }

    async fn change_scope(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        scope: PrelabelScope,
        reset: bool,
    ) -> Result<()> {
        if let Some(id) = &scope.task_id {
            id.validate_path_segment()
                .map_err(|_| PrelabelFailure::Invalid)?;
        }
        if let Some(id) = &scope.config_id {
            id.validate_path_segment()
                .map_err(|_| PrelabelFailure::Invalid)?;
        }
        let mut control = self.lock(dataset).await?;
        let metadata = repo
            .load_dataset_config()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let mut next = control.clone();
        for task in &metadata.tasks {
            for config_id in &task.prelabel_config_ids {
                if scope.matches(&task.task_id, config_id)
                    && !next
                        .scopes
                        .iter()
                        .any(|s| s.task_id == task.task_id && &s.config_id == config_id)
                {
                    next.scopes.push(ScopeControl {
                        task_id: task.task_id.clone(),
                        config_id: config_id.clone(),
                        generation: 0,
                        paused: false,
                    });
                }
            }
        }
        let mut changed = false;
        for entry in next
            .scopes
            .iter_mut()
            .filter(|s| scope.matches(&s.task_id, &s.config_id))
        {
            if entry.paused != reset {
                entry.paused = reset;
                entry.generation = entry
                    .generation
                    .checked_add(1)
                    .ok_or(PrelabelFailure::Limit)?;
                changed = true;
            }
        }
        if reset {
            if scope == PrelabelScope::default() && changed {
                next.generation = next
                    .generation
                    .checked_add(1)
                    .ok_or(PrelabelFailure::Limit)?;
            }
            next.results
                .retain(|_, result| !scope.matches(&result.task_id, &result.config_id));
            for run in &mut next.runs {
                if run
                    .items
                    .iter()
                    .any(|item| scope.matches(&item.task_id, &item.config_id))
                    && matches!(
                        run.summary.phase,
                        PrelabelRunPhase::Running
                            | PrelabelRunPhase::Ready
                            | PrelabelRunPhase::Interrupted
                    )
                {
                    run.summary.phase = PrelabelRunPhase::Cancelled;
                    run.summary.updated_at = now();
                }
            }
        }
        self.commit(dataset, &mut control, next).await?;
        if reset {
            let running = self.inner.running.lock().await;
            for run in &control.runs {
                if run.summary.phase == PrelabelRunPhase::Cancelled
                    && let Some(cancel) = running.get(&run.summary.run_id)
                {
                    let _ = cancel.send(true);
                }
            }
            self.cleanup_files(dataset, &control).await?;
        }
        Ok(())
    }

    async fn preflight(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        mappings: BTreeMap<TaskId, PrelabelConfigId>,
    ) -> Result<()> {
        let mut control = self.lock(dataset).await?;
        self.prune(dataset, &mut control).await?;
        if control.runs.len() >= self.inner.limits.max_retained_runs {
            return Err(PrelabelFailure::Limit);
        }
        if control
            .runs
            .iter()
            .any(|run| run.summary.phase == PrelabelRunPhase::Running)
        {
            return Err(PrelabelFailure::Busy);
        }
        let metadata = repo
            .load_dataset()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        if mappings.len() > metadata.tasks.len()
            || mappings.keys().any(|id| metadata.task(id).is_none())
        {
            return Err(PrelabelFailure::Invalid);
        }
        let mut blockers = Vec::new();
        let mut models = BTreeMap::new();
        let tasks: Vec<_> = metadata
            .tasks
            .iter()
            .filter(|task| task.enabled && task.annotation_type == AnnotationType::BoundingBox)
            .collect();
        let mut remaining = Vec::new();
        for record in metadata.images.values() {
            let state = repo
                .load_image_state(&record.image_id)
                .await
                .map_err(|_| PrelabelFailure::Storage)?;
            for task in &tasks {
                if prelabel_task_eligible(task, &state) {
                    if remaining.len() >= self.inner.limits.max_work_items {
                        return Err(PrelabelFailure::Limit);
                    }
                    remaining.push((record, *task));
                }
            }
        }
        for task in tasks.iter().filter(|task| {
            remaining
                .iter()
                .any(|(_, needed)| needed.task_id == task.task_id)
        }) {
            let candidates: Vec<_> = metadata
                .prelabel_configs
                .iter()
                .filter(|config| {
                    config.validate_for_task(task).is_ok()
                        && matches!(config.execution, PrelabelExecution::ServerSide { .. })
                })
                .collect();
            let config = if let Some(id) = mappings.get(&task.task_id) {
                candidates.iter().find(|c| &c.config_id == id).copied()
            } else if candidates.len() == 1 {
                Some(candidates[0])
            } else {
                None
            };
            if let Some(config) = config {
                if let Ok(model) = self.model(config).await {
                    models.insert(
                        task.task_id.clone(),
                        (
                            config,
                            digest(&(*task, config))?,
                            blake3::hash(&model).to_hex().to_string(),
                        ),
                    );
                } else {
                    blockers.push(format!(
                        "Workflow {}: model file is unavailable or invalid",
                        task.task_id
                    ));
                }
            } else {
                blockers.push(format!(
                    "Workflow {}: select one compatible server model",
                    task.task_id
                ));
            }
        }
        let eligible = remaining.len();
        let ineligible = metadata
            .tasks
            .iter()
            .filter(|task| task.annotation_type == AnnotationType::BoundingBox)
            .count()
            .saturating_mul(metadata.images.len())
            .saturating_sub(eligible);
        let items: Vec<_> = remaining
            .into_iter()
            .filter_map(|(record, task)| {
                models
                    .get(&task.task_id)
                    .map(|(config, config_digest, model_digest)| WorkItem {
                        image_id: record.image_id.clone(),
                        image_hash: record.blake3.clone(),
                        task_id: task.task_id.clone(),
                        config_id: config.config_id.clone(),
                        config_digest: config_digest.clone(),
                        model_digest: model_digest.clone(),
                        generation: Self::generation(&control, &task.task_id, &config.config_id),
                        outcome: PrelabelItemOutcome::Pending,
                    })
            })
            .collect();
        let reusable = items
            .iter()
            .filter(|item| result_key(item).is_ok_and(|key| control.results.contains_key(&key)))
            .count();
        let timestamp = now();
        let mut next = control.clone();
        next.runs.push(Run {
            summary: PrelabelRunSummary {
                run_id: uuid::Uuid::new_v4().to_string(),
                phase: PrelabelRunPhase::Ready,
                created_at: timestamp,
                updated_at: timestamp,
                total: eligible,
                pending: eligible,
                reusable,
                ineligible,
                generated: 0,
                empty: 0,
                skipped: 0,
                failed: 0,
                blockers,
            },
            items,
        });
        self.commit(dataset, &mut control, next).await
    }

    async fn start(
        &self,
        dataset: &DatasetId,
        repo: DatasetRepository,
        run_id: &str,
        retry: bool,
    ) -> Result<()> {
        let mut control = self.lock(dataset).await?;
        self.prune(dataset, &mut control).await?;
        if control
            .runs
            .iter()
            .any(|run| run.summary.phase == PrelabelRunPhase::Running)
        {
            return Err(PrelabelFailure::Busy);
        }
        let mut next = control.clone();
        let index = next
            .runs
            .iter()
            .position(|r| r.summary.run_id == run_id)
            .ok_or(PrelabelFailure::NotFound)?;
        let run = &next.runs[index];
        if !run.summary.blockers.is_empty()
            || (!retry && run.summary.phase != PrelabelRunPhase::Ready)
            || (retry
                && !matches!(
                    run.summary.phase,
                    PrelabelRunPhase::Cancelled
                        | PrelabelRunPhase::Interrupted
                        | PrelabelRunPhase::Completed
                ))
        {
            return Err(PrelabelFailure::NotReady);
        }
        if !retry {
            // Preflight is advisory. Start must not silently omit work added since it ran.
            let metadata = repo
                .load_dataset()
                .await
                .map_err(|_| PrelabelFailure::Storage)?;
            let captured: std::collections::BTreeSet<_> = run
                .items
                .iter()
                .map(|item| (&item.image_id, &item.task_id))
                .collect();
            for image in metadata.images.keys() {
                let state = repo
                    .load_image_state(image)
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
                if metadata.tasks.iter().any(|task| {
                    task.annotation_type == AnnotationType::BoundingBox
                        && prelabel_task_eligible(task, &state)
                        && !captured.contains(&(image, &task.task_id))
                }) {
                    return Err(PrelabelFailure::NotReady);
                }
            }
        }
        // An explicit start/retry resumes only the captured workflow/model pairs.
        for item in &run.items {
            if let Some(scope) = next
                .scopes
                .iter_mut()
                .find(|s| s.task_id == item.task_id && s.config_id == item.config_id)
            {
                scope.paused = false;
            }
        }
        let generations: Vec<_> = next.runs[index]
            .items
            .iter()
            .map(|item| Self::generation(&next, &item.task_id, &item.config_id))
            .collect();
        let run = &mut next.runs[index];
        for (item, generation) in run.items.iter_mut().zip(generations) {
            if item.generation != generation {
                // Reset removed any previously generated result. Explicit retry regenerates it.
                item.outcome = PrelabelItemOutcome::Pending;
            } else if retry && item.outcome == PrelabelItemOutcome::Failed {
                item.outcome = PrelabelItemOutcome::Pending;
            }
            item.generation = generation;
        }
        for item_index in 0..next.runs[index].items.len() {
            let item = next.runs[index].items[item_index].clone();
            if item.outcome == PrelabelItemOutcome::Pending
                && let Some(cached) = next.results.get(&result_key(&item)?)
            {
                let outcome = if self.revalidate(&repo, &next, &item).await.is_err() {
                    PrelabelItemOutcome::Skipped
                } else if cached.empty {
                    PrelabelItemOutcome::Empty
                } else {
                    PrelabelItemOutcome::Generated
                };
                next.runs[index].items[item_index].outcome = outcome;
            }
        }
        let run = &mut next.runs[index];
        run.summary.phase = PrelabelRunPhase::Running;
        update_counts(run);
        self.commit(dataset, &mut control, next).await?;
        let (cancel, receiver) = watch::channel(false);
        self.inner
            .running
            .lock()
            .await
            .insert(run_id.to_owned(), cancel);
        let service = self.clone();
        let dataset = dataset.clone();
        let run_id = run_id.to_owned();
        tokio::spawn(async move {
            if service
                .execute_run(&dataset, &repo, &run_id, receiver)
                .await
                .is_err()
            {
                // The durable pending item remains retryable if a write fails.
                if let Ok(mut control) = service.lock(&dataset).await {
                    let mut next = control.clone();
                    if let Some(run) = next.runs.iter_mut().find(|r| r.summary.run_id == run_id)
                        && run.summary.phase == PrelabelRunPhase::Running
                    {
                        run.summary.phase = PrelabelRunPhase::Interrupted;
                        run.summary.updated_at = now();
                    }
                    let _ = service.commit(&dataset, &mut control, next).await;
                }
                tracing::warn!(event = "prelabel_run_interrupted", run_id = %run_id);
            }
            service.inner.running.lock().await.remove(&run_id);
        });
        Ok(())
    }

    async fn execute_run(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        run_id: &str,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        loop {
            let item = {
                let mut control = self.lock(dataset).await?;
                let run = control
                    .runs
                    .iter()
                    .find(|run| run.summary.run_id == run_id)
                    .ok_or(PrelabelFailure::NotFound)?;
                if run.summary.phase != PrelabelRunPhase::Running || *cancel.borrow() {
                    return Ok(());
                }
                match run
                    .items
                    .iter()
                    .position(|i| i.outcome == PrelabelItemOutcome::Pending)
                {
                    Some(index) => (index, run.items[index].clone()),
                    None => {
                        let mut next = control.clone();
                        let run = next
                            .runs
                            .iter_mut()
                            .find(|r| r.summary.run_id == run_id)
                            .ok_or(PrelabelFailure::NotFound)?;
                        run.summary.phase = PrelabelRunPhase::Completed;
                        update_counts(run);
                        self.commit(dataset, &mut control, next).await?;
                        return Ok(());
                    }
                }
            };
            let inference = async {
                let _permit = self
                    .inner
                    .workers
                    .acquire()
                    .await
                    .map_err(|_| PrelabelFailure::Busy)?;
                let control = self.lock(dataset).await?;
                self.revalidate(repo, &control, &item.1).await?;
                drop(control);
                let metadata = repo
                    .load_dataset()
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
                let (task, config, _) =
                    configuration(&metadata, &item.1.task_id, &item.1.config_id)?;
                let record = metadata
                    .images
                    .get(&item.1.image_id)
                    .ok_or(PrelabelFailure::Stale)?;
                let model = self.model(config).await?;
                let image = files::image(repo, record)?;
                let candidates = self
                    .inner
                    .runner
                    .infer(model, image, config.clone(), task.clone())
                    .await?;
                Ok::<_, PrelabelFailure>((
                    candidates,
                    config.clone(),
                    task.clone(),
                    record.dimensions(),
                ))
            };
            let result = tokio::select! { result = inference => result, _ = cancel.changed() => return Ok(()) };
            let mut control = self.lock(dataset).await?;
            if control
                .runs
                .iter()
                .find(|r| r.summary.run_id == run_id)
                .is_none_or(|r| r.summary.phase != PrelabelRunPhase::Running)
            {
                return Ok(());
            }
            let result = match result {
                Ok((candidates, config, task, dimensions)) => {
                    match self.revalidate(repo, &control, &item.1).await {
                        Ok(()) => certify(
                            dataset,
                            &control,
                            &item.1,
                            &config,
                            &task,
                            dimensions,
                            candidates.suggestions,
                            candidates.execution.clone(),
                        )
                        .map(|suggestions| (suggestions, candidates.execution)),
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            };
            let mut next = control.clone();
            let outcome = match result {
                Ok((suggestions, execution)) => {
                    self.check_result_size(&suggestions)?;
                    let key = result_key(&item.1)?;
                    if next.results.len() >= self.inner.limits.max_retained_results
                        && !next.results.contains_key(&key)
                    {
                        return Err(PrelabelFailure::Limit);
                    }
                    let bytes = serde_json::to_vec_pretty(&suggestions)
                        .map_err(|_| PrelabelFailure::Invalid)?
                        .len() as u64
                        + 1;
                    let retained_bytes: u64 = next
                        .results
                        .iter()
                        .filter(|(existing, _)| *existing != &key)
                        .map(|(_, result)| result.bytes)
                        .sum();
                    if retained_bytes.saturating_add(bytes)
                        > self.inner.limits.max_total_result_bytes
                    {
                        return Err(PrelabelFailure::Limit);
                    }
                    write_json_atomic(
                        &self.directory(dataset)?.join(format!("result-{key}.json")),
                        &suggestions,
                    )
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
                    next.results.insert(
                        key,
                        CachedResult {
                            execution,
                            task_id: item.1.task_id.clone(),
                            config_id: item.1.config_id.clone(),
                            created_at: now(),
                            bytes,
                            empty: suggestions.is_empty(),
                        },
                    );
                    if suggestions.is_empty() {
                        PrelabelItemOutcome::Empty
                    } else {
                        PrelabelItemOutcome::Generated
                    }
                }
                Err(PrelabelFailure::Stale | PrelabelFailure::Paused) => {
                    PrelabelItemOutcome::Skipped
                }
                Err(_) => PrelabelItemOutcome::Failed,
            };
            let run = next
                .runs
                .iter_mut()
                .find(|r| r.summary.run_id == run_id)
                .ok_or(PrelabelFailure::NotFound)?;
            run.items[item.0].outcome = outcome;
            update_counts(run);
            self.commit(dataset, &mut control, next).await?;
        }
    }
}

fn update_counts(run: &mut Run) {
    let count = |outcome| {
        run.items
            .iter()
            .filter(|item| item.outcome == outcome)
            .count()
    };
    run.summary.pending = count(PrelabelItemOutcome::Pending);
    run.summary.generated = count(PrelabelItemOutcome::Generated);
    run.summary.empty = count(PrelabelItemOutcome::Empty);
    run.summary.skipped = count(PrelabelItemOutcome::Skipped);
    run.summary.failed = count(PrelabelItemOutcome::Failed);
    run.summary.updated_at = now();
}
fn admin_state(control: &Control) -> PrelabelAdminState {
    PrelabelAdminState {
        runs: control
            .runs
            .iter()
            .rev()
            .map(|run| run.summary.clone())
            .collect(),
        retained_results: control.results.len(),
        paused_scopes: control
            .scopes
            .iter()
            .filter(|s| s.paused)
            .map(|s| PrelabelScope {
                task_id: Some(s.task_id.clone()),
                config_id: Some(s.config_id.clone()),
            })
            .collect(),
    }
}
