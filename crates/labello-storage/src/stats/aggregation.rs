use super::*;

pub(super) struct StatsAggregation {
    stats: DatasetStats,
    throughput: BTreeMap<String, (usize, usize)>,
    imbalance: Option<labello_domain::ImbalanceConfig>,
    enabled_task_ids: Vec<TaskId>,
    pub(super) contributors: super::contributors::ContributorAggregation,
}

impl StatsAggregation {
    pub(super) fn new(metadata: &labello_domain::DatasetMetadata) -> Self {
        let stats = DatasetStats {
            total_images: metadata.images.len(),
            per_task: metadata
                .tasks
                .iter()
                .filter(|task| task.enabled)
                .map(|task| (task.task_id.clone(), TaskStats::default()))
                .collect(),
            per_class: metadata
                .label_classes
                .iter()
                .map(|class| (class.class_id.clone(), ClassStats::default()))
                .collect(),
            ..DatasetStats::default()
        };
        Self {
            contributors: Default::default(),
            stats,
            throughput: BTreeMap::new(),
            imbalance: metadata.imbalance.clone(),
            enabled_task_ids: metadata
                .tasks
                .iter()
                .filter(|task| task.enabled)
                .map(|task| task.task_id.clone())
                .collect(),
        }
    }

    pub(super) fn record_image(
        &mut self,
        metadata: &labello_domain::DatasetMetadata,
        state: &ImageState,
    ) {
        let stats = &mut self.stats;
        let throughput = &mut self.throughput;
        for task in &metadata.tasks {
            if !task.enabled {
                continue;
            }
            let task_stats = stats.per_task.entry(task.task_id.clone()).or_default();
            if let Some(coverage) = state.import_coverage.get(&task.task_id) {
                match coverage {
                    ImportCoverage::Complete => stats.import_coverage.complete += 1,
                    ImportCoverage::VerifiedEmpty => {
                        stats.import_coverage.verified_empty += 1;
                    }
                    ImportCoverage::Incomplete => stats.import_coverage.incomplete += 1,
                    ImportCoverage::Excluded => stats.import_coverage.excluded += 1,
                }
            }
            if let Some(target_set) = state.migration_target_sets.get(&task.task_id) {
                stats.migration.expected += target_set.targets.len();
                task_stats.migration.expected += target_set.targets.len();
                for target in &target_set.targets {
                    match &state.migration_dispositions[&task.task_id][&target.object_group_id]
                        .status
                    {
                        MigrationDispositionStatus::Annotated { .. } => {
                            stats.migration.annotated += 1;
                            task_stats.migration.annotated += 1;
                        }
                        MigrationDispositionStatus::Excluded { .. } => {
                            stats.migration.excluded += 1;
                            task_stats.migration.excluded += 1;
                        }
                        MigrationDispositionStatus::Pending => {
                            stats.migration.pending += 1;
                            task_stats.migration.pending += 1;
                        }
                    }
                }
            }
            if !state.included_in_completion_denominator(&task.task_id) {
                continue;
            }
            match state
                .task_states
                .get(&task.task_id)
                .map(|state| &state.status)
                .unwrap_or(&TaskStatus::Pending)
            {
                TaskStatus::Completed => {
                    stats.completed_tasks += 1;
                    task_stats.completed += 1;
                    for class_id in &task.class_ids {
                        stats
                            .per_class
                            .entry(class_id.clone())
                            .or_default()
                            .completed_tasks += 1;
                    }
                }
                TaskStatus::Submitted => {
                    stats.awaiting_review_tasks += 1;
                    task_stats.awaiting_review += 1;
                }
                TaskStatus::InProgress => {
                    stats.in_progress_tasks += 1;
                    task_stats.in_progress += 1;
                }
                TaskStatus::NeedsCorrection | TaskStatus::LegacyAdjudicationRequired => {
                    stats.needs_correction_tasks += 1;
                    task_stats.needs_correction += 1;
                }
                TaskStatus::Pending => {
                    stats.pending_tasks += 1;
                    task_stats.pending += 1;
                }
            }
        }
        for annotation in state.active_annotations() {
            stats.provenance.record_annotation(annotation);
            stats
                .per_task
                .entry(annotation.task_id.clone())
                .or_default()
                .provenance
                .record_annotation(annotation);
            let class_stats = stats
                .per_class
                .entry(annotation.class_id.clone())
                .or_default();
            class_stats.annotations += 1;
            class_stats.provenance.record_annotation(annotation);
            if matches!(annotation.revision_source, RevisionSource::Human { .. }) {
                let day = annotation.created_at.date_naive().to_string();
                throughput.entry(day).or_default().0 += 1;
            }
        }
        for review in &state.reviews {
            let day = review.timestamp.date_naive().to_string();
            throughput.entry(day).or_default().1 += 1;
        }
    }

    pub(super) fn finish(mut self) -> DatasetStats {
        self.stats.contributors = Some(self.contributors.finish());
        self.stats.throughput = self
            .throughput
            .into_iter()
            .map(
                |(day, (annotations, reviews))| labello_domain::ThroughputPoint {
                    day,
                    annotations,
                    reviews,
                },
            )
            .collect();
        if let Some(imbalance) = self.imbalance {
            let annotation_counts = self
                .enabled_task_ids
                .iter()
                .map(|task_id| {
                    let stats = &self.stats.per_task[task_id];
                    (task_id.clone(), stats.completed + stats.awaiting_review)
                })
                .collect::<BTreeMap<_, _>>();
            let review_counts = self
                .enabled_task_ids
                .iter()
                .map(|task_id| (task_id.clone(), self.stats.per_task[task_id].completed))
                .collect::<BTreeMap<_, _>>();
            let (annotation_blocked_tasks, review_blocked_tasks) = if imbalance.enforce {
                (
                    imbalance.blocked_tasks(&self.enabled_task_ids, &annotation_counts),
                    imbalance.blocked_tasks(&self.enabled_task_ids, &review_counts),
                )
            } else {
                (
                    std::collections::BTreeSet::new(),
                    std::collections::BTreeSet::new(),
                )
            };
            self.stats.assignment_balance = Some(labello_domain::AssignmentBalanceStats {
                annotation_counts,
                review_counts,
                annotation_blocked_tasks,
                review_blocked_tasks,
            });
        }
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labello_domain::{
        AnnotationType, DatasetId, DatasetMetadata, ImageId, ReviewConfig, TaskDefinition,
        TaskOutcome, TaskState, TutorialContent, now,
    };

    #[test]
    fn task_counts_are_exclusive_and_preserve_the_completion_denominator() {
        let timestamp = now();
        let task_id = TaskId::from("boxes");
        let mut metadata = DatasetMetadata::new(DatasetId::from("ds"), "Dataset", timestamp);
        metadata.tasks.push(TaskDefinition {
            task_id: task_id.clone(),
            name: "Boxes".into(),
            annotation_type: AnnotationType::BoundingBox,
            class_ids: Vec::new(),
            instructions: TutorialContent {
                title: "Boxes".into(),
                example_text: String::new(),
                example_images: Vec::new(),
            },
            skeleton: None,
            review: ReviewConfig::default(),
            prelabel_config_ids: Vec::new(),
            manual_box_guide_migration: None,
            enabled: true,
        });
        let mut disabled = metadata.tasks[0].clone();
        disabled.task_id = TaskId::from("disabled");
        disabled.enabled = false;
        metadata.tasks.push(disabled);
        let mut aggregation = StatsAggregation::new(&metadata);
        for (status, outcome) in [
            (TaskStatus::Pending, None),
            (TaskStatus::InProgress, None),
            (TaskStatus::Submitted, None),
            (TaskStatus::NeedsCorrection, None),
            (TaskStatus::Completed, Some(TaskOutcome::Approved)),
            (TaskStatus::Completed, Some(TaskOutcome::ReviewerCorrected)),
            (
                TaskStatus::Completed,
                Some(TaskOutcome::AnnotationCompleted),
            ),
        ] {
            let mut state = ImageState::new(ImageId::from("image"));
            let mut task_state = TaskState::new(task_id.clone(), timestamp);
            task_state.status = status;
            task_state.outcome = outcome;
            state.task_states.insert(task_id.clone(), task_state);
            aggregation.record_image(&metadata, &state);
        }
        // Missing state is pending; explicitly excluded import coverage contributes nothing.
        let mut missing = ImageState::new(ImageId::from("missing"));
        aggregation.record_image(&metadata, &missing);
        missing
            .import_coverage
            .insert(task_id.clone(), ImportCoverage::Excluded);
        aggregation.record_image(&metadata, &missing);
        let stats = aggregation.finish();
        assert_eq!(stats.per_task.len(), 1);
        let task = &stats.per_task[&task_id];
        assert_eq!(
            [
                task.pending,
                task.in_progress,
                task.awaiting_review,
                task.needs_correction,
                task.completed
            ],
            [2, 1, 1, 1, 3]
        );
        assert_eq!(
            [
                stats.pending_tasks,
                stats.in_progress_tasks,
                stats.awaiting_review_tasks,
                stats.needs_correction_tasks,
                stats.completed_tasks
            ],
            [2, 1, 1, 1, 3]
        );
        assert_eq!(
            task.pending
                + task.in_progress
                + task.awaiting_review
                + task.needs_correction
                + task.completed,
            8
        );
        let wire = serde_json::to_value(task).unwrap();
        for removed in [
            "unreviewed",
            "reviewed",
            "approved",
            "rejected",
            "reviewerCorrected",
            "finalized",
        ] {
            assert!(wire.get(removed).is_none());
        }
    }
}
