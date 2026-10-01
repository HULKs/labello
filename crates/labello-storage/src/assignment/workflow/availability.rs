use super::*;
use labello_domain::{
    WorkflowAvailability, WorkflowPreparationStatus, WorkflowUnavailableReason as Reason,
};

impl DatasetRepository {
    pub async fn workflow_availability(
        &self,
        user: &UserId,
        kind: AssignmentKind,
    ) -> StorageResult<Vec<WorkflowAvailability>> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let metadata = self.load_dataset().await?;
        require_role(
            &metadata.role_assignments,
            &metadata.dataset_id,
            user,
            role_for_kind(&kind),
        )?;
        let key = (
            user.clone(),
            match kind {
                AssignmentKind::Annotation => "annotation",
                AssignmentKind::Review => "review",
                AssignmentKind::LegacyAdjudication => "adjudication",
            }
            .to_string(),
        );
        let cache = &self.assignment_availability_cache;
        if let Some(cached) = cache.workflow_lookup(&key, cache.generation()).await {
            return Ok(cached);
        }
        let _refresh = cache.workflow_refresh().await;
        let generation = cache.generation();
        if let Some(cached) = cache.workflow_lookup(&key, generation).await {
            return Ok(cached);
        }
        // Availability is advisory. Cache entries are invalidated under the image
        // lock before publication; claims still validate their exact target state.
        // Never hold dataset admission while cold polling reads every image.
        let sampled_at = labello_domain::now();
        let mut workers = tokio::task::JoinSet::new();
        let mut images = metadata.images.keys().cloned();
        let visibility = metadata.bounding_box_visibility;
        for image in images.by_ref().take(32) {
            let repo = self.clone();
            workers.spawn(async move { repo.workflow_polling_state(&image, visibility).await });
        }
        let mut states = Vec::with_capacity(metadata.images.len());
        while let Some(result) = workers.join_next().await {
            states.push(result.map_err(|_| {
                StorageError::BackgroundTask("workflow availability scan failed".into())
            })??);
            if let Some(image) = images.next() {
                let repo = self.clone();
                workers.spawn(async move { repo.workflow_polling_state(&image, visibility).await });
            }
        }
        let exception = kind == AssignmentKind::Review
            && !self
                .has_independent_workflow_review(&metadata, user)
                .await?;
        let mut availability = Vec::new();
        for task in &metadata.tasks {
            let enabled = Self::task_supports_assignment(task, &kind)?;
            let balanced = !self
                .task_is_overrepresented(&metadata, &task.task_id, &kind)
                .await?;
            let split = if kind == AssignmentKind::Annotation {
                states.iter().any(|s| {
                    s.workflow_preparations.get(&task.task_id).map_or_else(
                        || !initial_preparation(s, task).objects.is_empty(),
                        |p| !p.objects.is_empty(),
                    )
                })
            } else {
                states.iter().any(|s| {
                    s.review_object_targets(task)
                        .is_ok_and(|targets| !targets.is_empty())
                })
            };
            let object_selection = WorkflowSelection {
                task_id: task.task_id.clone(),
                kind: kind.clone(),
                variant: WorkflowVariant::Objects,
            };
            let restricted = if let Some(limit) = metadata.workflow_queue.max_pending_overviews {
                let images = self
                    .workflow_images_awaiting_overview(&metadata, task, &object_selection)
                    .await?;
                (images.len() >= limit).then_some(images)
            } else {
                None
            };
            for variant in [WorkflowVariant::Objects, WorkflowVariant::Overview] {
                let selection = WorkflowSelection {
                    variant,
                    ..object_selection.clone()
                };
                let base = if !task.enabled {
                    Some(Reason::WorkflowDisabled)
                } else if !enabled {
                    Some(Reason::ReviewDisabled)
                } else if states.is_empty() {
                    Some(Reason::EmptyDataset)
                } else if !balanced {
                    Some(Reason::BalanceLimit)
                } else {
                    None
                };
                if let Some(reason) = base {
                    availability.push(WorkflowAvailability {
                        selection,
                        available: false,
                        reason: Some(reason),
                        split,
                    });
                    continue;
                }
                let mut available = false;
                let mut claimed = false;
                let mut capped = false;
                let mut pending_objects = false;
                let mut preparation = None;
                for state in &states {
                    let state = state.as_ref();
                    let mut prepared_state;
                    let state = if kind == AssignmentKind::Annotation
                        && state.assignment_eligible(&task.task_id)
                        && !state.workflow_preparations.contains_key(&task.task_id)
                    {
                        prepared_state = state.clone();
                        let mut initial = initial_preparation(state, task);
                        if labello_domain::workflow_prelabel_config(&metadata, task).is_some() {
                            initial.status = WorkflowPreparationStatus::Pending;
                        }
                        prepared_state
                            .workflow_preparations
                            .insert(task.task_id.clone(), initial);
                        &prepared_state
                    } else {
                        state
                    };
                    if kind == AssignmentKind::Annotation
                        && state.assignment_eligible(&task.task_id)
                    {
                        if let Some(p) = state.workflow_preparations.get(&task.task_id) {
                            match p.status {
                                WorkflowPreparationStatus::Pending => {
                                    preparation = Some(Reason::PreparationPending);
                                }
                                WorkflowPreparationStatus::Failed => {
                                    preparation = Some(Reason::PreparationFailed);
                                }
                                WorkflowPreparationStatus::Ready => {}
                            }
                        }
                        pending_objects |=
                            !state.workflow_pending_objects(&task.task_id).is_empty();
                    } else if kind == AssignmentKind::Review
                        && state
                            .task_states
                            .get(&task.task_id)
                            .is_some_and(|s| s.status == TaskStatus::Submitted)
                    {
                        pending_objects |= !state.workflow_pending_reviews(task)?.is_empty();
                    }
                    for context in candidates(state, task, &selection)? {
                        if context.review_target.as_ref().is_some_and(|target| {
                            !state.workflow_independent_reviewer(&context.item, target, user)
                        }) && !exception
                        {
                            continue;
                        }
                        if variant == WorkflowVariant::Objects
                            && restricted
                                .as_ref()
                                .is_some_and(|images| !images.contains(&state.image_id))
                        {
                            capped = true;
                            continue;
                        }
                        let owned = state.assignments.iter().any(|a| {
                            a.task_id == task.task_id
                                && a.kind == kind
                                && a.assigned_to == *user
                                && a.status == AssignmentStatus::Active
                                && !assignment_is_expired(a, labello_domain::now())
                                && state
                                    .workflow_assignments
                                    .get(&a.assignment_id)
                                    .is_some_and(|c| c.item == context.item)
                        });
                        if owned || claimable(state, &selection, &context, labello_domain::now()) {
                            available = true;
                            break;
                        }
                        claimed = true;
                    }
                    if available {
                        break;
                    }
                }
                let reason = (!available).then(|| {
                    if capped {
                        Reason::OverviewLimit
                    } else if claimed {
                        Reason::ClaimedByOthers
                    } else if let Some(preparation) = preparation {
                        preparation
                    } else if variant == WorkflowVariant::Overview && pending_objects {
                        Reason::ObjectsPending
                    } else if variant == WorkflowVariant::Objects {
                        Reason::NoObjects
                    } else if kind == AssignmentKind::Review {
                        Reason::NothingAwaitingReview
                    } else {
                        Reason::AnnotationFinished
                    }
                });
                availability.push(WorkflowAvailability {
                    selection,
                    available,
                    reason,
                    split,
                });
            }
        }
        let expires_at = states
            .iter()
            .flat_map(|s| &s.assignments)
            .filter(|a| a.status == AssignmentStatus::Active)
            .map(|a| {
                a.expires_at
                    .unwrap_or_else(|| lease_expiration(a.updated_at))
            })
            .filter(|expiry| *expiry > sampled_at)
            .min();
        cache
            .workflow_store(key, generation, expires_at, availability.clone())
            .await;
        Ok(availability)
    }
}
