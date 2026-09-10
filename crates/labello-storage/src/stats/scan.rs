use super::*;

const MAX_STATS_SCAN_WORKERS: usize = 32;

impl DatasetRepository {
    pub(super) async fn compute_dataset_stats(&self) -> StorageResult<DatasetStats> {
        let metadata = self.load_dataset().await?;
        let mut aggregation = StatsAggregation::new(&metadata);
        let mut scoring = labello_domain::ScoringProjection::default();
        let focus = self.scoring_focus(labello_domain::now()).await?;

        let mut image_ids = metadata.images.keys().cloned();
        let mut workers = tokio::task::JoinSet::new();
        for image_id in image_ids.by_ref().take(MAX_STATS_SCAN_WORKERS) {
            let repository = self.clone();
            workers.spawn(async move { repository.load_image_state_with_events(&image_id).await });
        }

        while let Some(result) = workers.join_next().await {
            let (state, events) = result.map_err(|error| {
                crate::StorageError::BackgroundTask(format!(
                    "dataset statistics worker failed: {error}"
                ))
            })??;
            aggregation.record_image(&metadata, &state);
            aggregation.record_contributors(&state, &events);
            scoring.record_image(&state, &events);
            if let Some(image_id) = image_ids.next() {
                let repository = self.clone();
                workers
                    .spawn(async move { repository.load_image_state_with_events(&image_id).await });
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
