use super::*;

const MAX_STATS_SCAN_WORKERS: usize = 32;

impl DatasetRepository {
    pub(super) async fn compute_dataset_stats(
        &self,
    ) -> StorageResult<(DatasetStats, labello_domain::stats::scoring::ImageScores)> {
        let metadata = self.load_dataset().await?;
        let mut aggregation = StatsAggregation::new(&metadata);
        let mut scoring = labello_domain::ScoringProjection::default();
        self.review_scoring_focus(labello_domain::now()).await?;

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

        // Every scanned reward event published its focus first. Capture after the scan
        // so concurrent submissions cannot be credited with an older focus snapshot.
        let focus = self
            .scoring
            .lock()
            .await
            .clone()
            .expect("focus initialized");
        let mut stats = aggregation.finish();
        let image_scores = scoring.finish(
            stats.contributors.as_mut().expect("contributors supported"),
            &focus.windows,
            &focus.review_windows,
        );
        stats.scoring_version = Some(labello_domain::stats::scoring::SCORING_VERSION);
        stats.scoring_focus = focus.windows.last().cloned();
        stats.review_scoring_focus = focus.review_windows.last().cloned();
        Ok((stats, image_scores))
    }
}
