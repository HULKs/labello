use super::*;
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct ImageContribution {
    config: blake3::Hash,
    aggregation: StatsAggregation,
    scoring: labello_domain::ScoringProjection,
}

impl DatasetRepository {
    async fn image_stats_contribution(
        &self,
        image: &labello_domain::ImageId,
        metadata: &labello_domain::DatasetMetadata,
        config: blake3::Hash,
    ) -> StorageResult<Arc<ImageContribution>> {
        let lock = self.image_lock(image);
        let _guard = lock.lock().await;
        if let Some(value) = self
            .stats_cache
            .images
            .lock()
            .get(image)
            .filter(|value| value.config == config)
            .cloned()
        {
            return Ok(value);
        }
        let (state, events) = self.load_image_state_with_events(image).await?;
        let mut aggregation = StatsAggregation::new(metadata);
        aggregation.record_image(metadata, &state);
        aggregation.record_contributors(&state, &events);
        let mut scoring = labello_domain::ScoringProjection::default();
        scoring.record_image(&state, &events);
        let value = Arc::new(ImageContribution {
            config,
            aggregation,
            scoring,
        });
        self.stats_cache
            .images
            .lock()
            .insert(image.clone(), value.clone());
        Ok(value)
    }

    pub(super) async fn compute_dataset_stats(&self) -> StorageResult<DatasetStats> {
        let metadata = self.load_dataset().await?;
        // Configuration changes can alter contributions without an image event.
        let config = labello_domain::DatasetConfig::from_metadata(&metadata);
        let config = blake3::hash(&serde_json::to_vec(&config).map_err(|source| {
            crate::StorageError::Json {
                path: self.dataset_path(),
                source,
            }
        })?);
        let mut aggregation = StatsAggregation::new(&metadata);
        let mut scoring = labello_domain::ScoringProjection::default();
        let focus = self.scoring_focus(labello_domain::now()).await?;
        let metadata = Arc::new(metadata);
        let mut missing = Vec::new();
        let mut cached = Vec::new();
        {
            let images = self.stats_cache.images.lock();
            for image in metadata.images.keys() {
                if let Some(value) = images.get(image).filter(|value| value.config == config) {
                    cached.push(value.clone());
                } else {
                    missing.push(image.clone());
                }
            }
        }
        for value in cached {
            aggregation.extend(&value.aggregation);
            scoring.extend(&value.scoring);
        }
        // Keep the existing cold-read concurrency, without spawning a task for
        // every unchanged image on warm refreshes.
        let mut missing = missing.into_iter();
        let mut workers = tokio::task::JoinSet::new();
        let read = |image: labello_domain::ImageId| {
            let repository = self.clone();
            let metadata = metadata.clone();
            async move {
                repository
                    .image_stats_contribution(&image, &metadata, config)
                    .await
            }
        };
        for image in missing.by_ref().take(32) {
            workers.spawn(read(image));
        }
        while let Some(result) = workers.join_next().await {
            let value = result.map_err(|_| {
                crate::StorageError::BackgroundTask("statistics contribution worker failed".into())
            })??;
            aggregation.extend(&value.aggregation);
            scoring.extend(&value.scoring);
            if let Some(image) = missing.next() {
                workers.spawn(read(image));
            }
        }
        let mut stats = aggregation.finish();
        scoring.finish(
            stats.contributors.as_mut().expect("contributors supported"),
            &focus,
        );
        stats.scoring_version = Some(labello_domain::stats::scoring::SCORING_VERSION);
        stats.scoring_focus = focus.last().cloned();
        Ok(stats)
    }
}

#[cfg(test)]
impl DatasetRepository {
    pub(crate) async fn reference_dataset_stats(&self) -> StorageResult<DatasetStats> {
        let metadata = self.load_dataset().await?;
        let focus = self.scoring_focus(labello_domain::now()).await?;
        let mut aggregation = StatsAggregation::new(&metadata);
        let mut scoring = labello_domain::ScoringProjection::default();
        for image in metadata.images.keys() {
            let (state, events) = self.load_image_state_with_events(image).await?;
            aggregation.record_image(&metadata, &state);
            aggregation.record_contributors(&state, &events);
            scoring.record_image(&state, &events);
        }
        let mut stats = aggregation.finish();
        scoring.finish(stats.contributors.as_mut().unwrap(), &focus);
        stats.scoring_version = Some(labello_domain::stats::scoring::SCORING_VERSION);
        stats.scoring_focus = focus.last().cloned();
        Ok(stats)
    }
}
