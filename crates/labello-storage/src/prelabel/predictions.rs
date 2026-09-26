use super::*;

/// Keeps reset/config changes serialized with the caller's annotation transaction.
/// Dropping it after the event commit is essential: validation alone is insufficient.
pub struct AcceptanceGuard {
    _control: OwnedMutexGuard<Control>,
}

impl PrelabelService {
    pub async fn generation_status(
        &self,
        dataset: &DatasetId,
        task: &TaskId,
        config: &PrelabelConfigId,
    ) -> Result<PrelabelGeneration> {
        let control = self.lock(dataset).await?;
        Ok(Self::generation(&control, task, config))
    }

    pub async fn configuration_guard(&self, dataset: &DatasetId) -> Result<AcceptanceGuard> {
        Ok(AcceptanceGuard {
            _control: self.lock(dataset).await?,
        })
    }

    pub async fn suggestions(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        user: &UserId,
        image_id: &ImageId,
        task_id: &TaskId,
        config_id: &PrelabelConfigId,
    ) -> Result<PrelabelResponse> {
        let metadata = repo
            .load_dataset()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let (task, config, config_digest) = configuration(&metadata, task_id, config_id)?;
        let record = metadata
            .images
            .get(image_id)
            .ok_or(PrelabelFailure::Invalid)?;
        let state = repo
            .load_image_state(image_id)
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        if !prelabel_task_eligible(task, &state) {
            return Err(PrelabelFailure::Stale);
        }
        if !config.available_to_annotators {
            return Err(PrelabelFailure::Invalid);
        }
        let _permit = if matches!(config.execution, PrelabelExecution::ServerSide { .. }) {
            Some(self.inner.workers.interactive().await?)
        } else {
            None
        };
        let model = self.model(config).await?;
        let model_digest = blake3::hash(&model).to_hex().to_string();
        let control = self.lock(dataset).await?;
        let generation = Self::generation(&control, task_id, config_id);
        if generation.paused {
            return Ok(PrelabelResponse {
                execution: None,
                generation,
                suggestions: vec![],
                from_batch: false,
                browser_grant: None,
            });
        }
        let item = WorkItem {
            image_id: image_id.clone(),
            image_hash: record.blake3.clone(),
            task_id: task_id.clone(),
            config_id: config_id.clone(),
            config_digest,
            model_digest,
            generation: generation.clone(),
            outcome: PrelabelItemOutcome::Pending,
        };
        let key = result_key(&item)?;
        if let Some(cached) = control.results.get(&key)
            && cached.created_at
                + std::time::Duration::from_secs(self.inner.limits.retention_seconds)
                > now()
        {
            let suggestions =
                read_json(&self.directory(dataset)?.join(format!("result-{key}.json")))
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
            return Ok(PrelabelResponse {
                execution: Some(cached.execution.clone()),
                generation,
                suggestions,
                from_batch: true,
                browser_grant: None,
            });
        }
        if matches!(config.execution, PrelabelExecution::BrowserLocal { .. }) {
            let mut grant = BrowserPrelabelGrant {
                dataset_id: dataset.clone(),
                image_id: image_id.clone(),
                image_hash: record.blake3.clone(),
                task_id: task_id.clone(),
                config_id: config_id.clone(),
                config_digest: item.config_digest,
                model_digest: item.model_digest,
                generation: generation.clone(),
                expires_at: now() + std::time::Duration::from_secs(600),
                signature: String::new(),
            };
            grant.signature = sign(&control.key, &grant)?;
            return Ok(PrelabelResponse {
                execution: None,
                generation,
                suggestions: vec![],
                from_batch: false,
                browser_grant: Some(grant),
            });
        }
        drop(control);
        let image = files::image(repo, record)?;
        let candidates = self
            .inner
            .runner
            .infer(
                InferenceOwner::Interactive {
                    dataset: dataset.clone(),
                    user: user.clone(),
                },
                model,
                image,
                config.clone(),
                task.clone(),
            )
            .await?;
        let control = self.lock(dataset).await?;
        self.revalidate(repo, &control, &item).await?;
        let suggestions = certify(
            dataset,
            &control,
            &item,
            config,
            task,
            record.dimensions(),
            candidates.suggestions,
            candidates.execution.clone(),
        )?;
        self.check_result_size(&suggestions)?;
        Ok(PrelabelResponse {
            execution: Some(candidates.execution),
            generation,
            suggestions,
            from_batch: false,
            browser_grant: None,
        })
    }

    pub async fn certify_browser(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        result: BrowserPrelabelResult,
    ) -> Result<PrelabelResponse> {
        let mut grant = result.grant;
        let control = self.lock(dataset).await?;
        let signature = std::mem::take(&mut grant.signature);
        if &grant.dataset_id != dataset
            || grant.expires_at < now()
            || !valid_signature(&control.key, &grant, &signature)?
            || result.execution.is_server()
        {
            return Err(PrelabelFailure::Invalid);
        }
        let item = WorkItem {
            image_id: grant.image_id,
            image_hash: grant.image_hash,
            task_id: grant.task_id,
            config_id: grant.config_id,
            config_digest: grant.config_digest,
            model_digest: grant.model_digest,
            generation: grant.generation,
            outcome: PrelabelItemOutcome::Pending,
        };
        self.revalidate(repo, &control, &item).await?;
        let metadata = repo
            .load_dataset()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let (task, config, _) = configuration(&metadata, &item.task_id, &item.config_id)?;
        if !config.available_to_annotators
            || !matches!(config.execution, PrelabelExecution::BrowserLocal { .. })
        {
            return Err(PrelabelFailure::Invalid);
        }
        let record = metadata
            .images
            .get(&item.image_id)
            .ok_or(PrelabelFailure::Invalid)?;
        let suggestions = certify(
            dataset,
            &control,
            &item,
            config,
            task,
            record.dimensions(),
            result.suggestions,
            result.execution.clone(),
        )?;
        self.check_result_size(&suggestions)?;
        Ok(PrelabelResponse {
            execution: Some(result.execution),
            generation: item.generation,
            suggestions,
            from_batch: false,
            browser_grant: None,
        })
    }

    pub async fn authorize_acceptance(
        &self,
        dataset: &DatasetId,
        repo: &DatasetRepository,
        image_id: &ImageId,
        evidence: &BTreeMap<AnnotationId, Box<PrelabelEvidence>>,
    ) -> Result<AcceptanceGuard> {
        let control = self.lock(dataset).await?;
        let state = repo
            .load_image_state(image_id)
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        for (annotation_id, evidence) in evidence {
            let mut unsigned = evidence.as_ref().clone();
            let signature = std::mem::take(&mut unsigned.signature);
            let p = &unsigned.provenance;
            if &p.dataset_id != dataset
                || &p.image_id != image_id
                || !valid_signature(&control.key, &unsigned, &signature)?
            {
                return Err(PrelabelFailure::Invalid);
            }
            // Exact committed retries remain valid after reset; they create no new acceptance.
            if let Some(existing) = state.current_annotation(annotation_id)
                && matches!(&existing.origin, AnnotationOrigin::Prelabel { prelabel } if prelabel.provenance == p.clone() && prelabel.predicted_geometry == unsigned.predicted_geometry)
            {
                continue;
            }
            let item = WorkItem {
                image_id: p.image_id.clone(),
                image_hash: p.image_hash.clone(),
                task_id: p.task_id.clone(),
                config_id: p.config_id.clone(),
                config_digest: p.config_digest.clone(),
                model_digest: p.model_digest.clone(),
                generation: PrelabelGeneration {
                    generation: p.generation,
                    scope_generation: p.scope_generation,
                    paused: false,
                },
                outcome: PrelabelItemOutcome::Pending,
            };
            self.revalidate(repo, &control, &item).await?;
        }
        Ok(AcceptanceGuard { _control: control })
    }

    pub(super) async fn revalidate(
        &self,
        repo: &DatasetRepository,
        control: &Control,
        item: &WorkItem,
    ) -> Result<()> {
        if Self::generation(control, &item.task_id, &item.config_id) != item.generation {
            return Err(PrelabelFailure::Stale);
        }
        if item.generation.paused {
            return Err(PrelabelFailure::Paused);
        }
        let metadata = repo
            .load_dataset()
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        let (task, config, current_digest) =
            configuration(&metadata, &item.task_id, &item.config_id)
                .map_err(|_| PrelabelFailure::Stale)?;
        if current_digest != item.config_digest
            || metadata
                .images
                .get(&item.image_id)
                .is_none_or(|r| r.blake3 != item.image_hash)
            || blake3::hash(&self.model(config).await?).to_hex().as_str() != item.model_digest
        {
            return Err(PrelabelFailure::Stale);
        }
        let state = repo
            .load_image_state(&item.image_id)
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        if !prelabel_task_eligible(task, &state) {
            return Err(PrelabelFailure::Stale);
        }
        Ok(())
    }

    pub(super) fn check_result_size(&self, suggestions: &[PrelabelSuggestion]) -> Result<()> {
        if serde_json::to_vec(suggestions)
            .map_err(|_| PrelabelFailure::Invalid)?
            .len()
            > self.inner.limits.max_result_bytes
        {
            return Err(PrelabelFailure::Limit);
        }
        Ok(())
    }
}

pub(super) fn result_key(item: &WorkItem) -> Result<String> {
    digest(&(
        &item.image_id,
        &item.image_hash,
        &item.task_id,
        &item.config_id,
        &item.config_digest,
        &item.model_digest,
        &item.generation,
    ))
}

fn sign<T: Serialize>(key: &[u8; 32], value: &T) -> Result<String> {
    Ok(blake3::keyed_hash(
        key,
        &serde_json::to_vec(value).map_err(|_| PrelabelFailure::Invalid)?,
    )
    .to_hex()
    .to_string())
}
fn valid_signature<T: Serialize>(key: &[u8; 32], value: &T, supplied: &str) -> Result<bool> {
    let expected = sign(key, value)?;
    // Equal-length fixed digests, compared without a data-dependent early exit.
    Ok(supplied.len() == expected.len()
        && supplied
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn certify(
    dataset: &DatasetId,
    control: &Control,
    item: &WorkItem,
    config: &PrelabelConfig,
    task: &TaskDefinition,
    dimensions: ImageDimensions,
    candidates: Vec<PrelabelSuggestion>,
    execution: PrelabelExecutionKind,
) -> Result<Vec<PrelabelSuggestion>> {
    if candidates.len() > 35_000 {
        return Err(PrelabelFailure::Limit);
    }
    let mut suggestions = Vec::new();
    for (index, mut hint) in candidates.into_iter().enumerate() {
        if hint.config_id != config.config_id
            || hint.task_id != task.task_id
            || !hint.confidence.is_finite()
            || !(0.0..=1.0).contains(&hint.confidence)
        {
            return Err(PrelabelFailure::Invalid);
        }
        let annotation = AnnotationVersion::native(
            "validation".into(),
            task.task_id.clone(),
            hint.class_id.clone(),
            task.annotation_type.clone(),
            hint.geometry.clone(),
            "validation".into(),
            now(),
        );
        annotation
            .validate_for_task(task, dimensions)
            .map_err(|_| PrelabelFailure::Invalid)?;
        if !hint.passes(&config.output_processing) {
            continue;
        }
        // The client supplies geometry, never trusted identity or provenance.
        hint.suggestion_id = digest(&(
            result_key(item)?,
            &execution,
            index,
            &hint.class_id,
            hint.confidence,
            &hint.geometry,
        ))?;
        let provenance = PrelabelProvenance {
            dataset_id: dataset.clone(),
            image_id: item.image_id.clone(),
            image_hash: item.image_hash.clone(),
            task_id: item.task_id.clone(),
            class_id: hint.class_id.clone(),
            config_id: item.config_id.clone(),
            config_digest: item.config_digest.clone(),
            model_id: config.model.model_id.clone(),
            model_version: config.model.version.clone(),
            model_digest: item.model_digest.clone(),
            execution: execution.clone(),
            trust: if execution.is_server() {
                PredictionTrust::ServerGenerated
            } else {
                PredictionTrust::BrowserReported
            },
            processing: config.output_processing.clone(),
            generation: item.generation.generation,
            scope_generation: item.generation.scope_generation,
            suggestion_id: hint.suggestion_id.clone(),
            confidence: hint.confidence,
        };
        let mut evidence = PrelabelEvidence {
            provenance,
            predicted_geometry: hint.geometry.clone(),
            signature: String::new(),
        };
        evidence.signature = sign(&control.key, &evidence)?;
        hint.evidence = Some(Box::new(evidence));
        suggestions.push(hint);
    }
    Ok(suggestions)
}
