use super::*;

/// Aggregate additive metrics only. Dataset-local task/class identities, balance
/// policy and focus selections must be presented with their source dataset.
pub fn aggregate_statistics<'a>(
    datasets: impl IntoIterator<Item = &'a DatasetStats>,
) -> DatasetStats {
    let datasets: Vec<_> = datasets.into_iter().collect();
    let mut total = DatasetStats::default();
    let contributors_available = datasets.iter().all(|stats| stats.contributors.is_some());
    let score_version = datasets.first().and_then(|stats| stats.scoring_version);
    total.scoring_version = score_version.filter(|version| {
        datasets
            .iter()
            .all(|stats| stats.scoring_version == Some(*version))
    });
    let mut contributors: BTreeMap<UserId, ContributorStats> = BTreeMap::new();
    let mut days: BTreeMap<String, ThroughputPoint> = BTreeMap::new();
    for stats in datasets {
        total.total_images += stats.total_images;
        total.completed_tasks += stats.completed_tasks;
        total.pending_tasks += stats.pending_tasks;
        total.in_progress_tasks += stats.in_progress_tasks;
        total.awaiting_review_tasks += stats.awaiting_review_tasks;
        total.needs_correction_tasks += stats.needs_correction_tasks;
        total.provenance.accepted_prelabel_annotations +=
            stats.provenance.accepted_prelabel_annotations;
        total.provenance.imported_direct_annotations +=
            stats.provenance.imported_direct_annotations;
        total.provenance.imported_derived_annotations +=
            stats.provenance.imported_derived_annotations;
        total.provenance.human_authored_annotations += stats.provenance.human_authored_annotations;
        total.provenance.human_accepted_imports += stats.provenance.human_accepted_imports;
        total.provenance.reviewer_corrections += stats.provenance.reviewer_corrections;
        total.migration.expected += stats.migration.expected;
        total.migration.annotated += stats.migration.annotated;
        total.migration.excluded += stats.migration.excluded;
        total.migration.pending += stats.migration.pending;
        total.import_coverage.complete += stats.import_coverage.complete;
        total.import_coverage.verified_empty += stats.import_coverage.verified_empty;
        total.import_coverage.incomplete += stats.import_coverage.incomplete;
        total.import_coverage.excluded += stats.import_coverage.excluded;
        for point in &stats.throughput {
            let day = days
                .entry(point.day.clone())
                .or_insert_with(|| ThroughputPoint {
                    day: point.day.clone(),
                    annotations: 0,
                    reviews: 0,
                });
            day.annotations += point.annotations;
            day.reviews += point.reviews;
        }
        if contributors_available {
            for (id, person) in stats.contributors.as_ref().unwrap() {
                let combined = contributors
                    .entry(id.clone())
                    .or_insert_with(|| ContributorStats {
                        display_name: person.display_name.clone(),
                        github_user_id: person.github_user_id.clone(),
                        history: Vec::new(),
                    });
                combined.history.extend(person.history.iter().cloned());
            }
        }
    }
    for person in contributors.values_mut() {
        let mut history: BTreeMap<String, ContributorDay> = BTreeMap::new();
        for day in std::mem::take(&mut person.history) {
            let combined = history
                .entry(day.day.clone())
                .or_insert_with(|| ContributorDay {
                    day: day.day.clone(),
                    ..Default::default()
                });
            combined.labeled += day.labeled;
            combined.reviewed += day.reviewed;
            combined.accepted += day.accepted;
            combined.rejected += day.rejected;
            combined.score.add(&day.score);
        }
        person.history = history.into_values().collect();
    }
    total.contributors = contributors_available.then_some(contributors);
    total.throughput = days.into_values().collect();
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_days_by_user_and_keeps_dataset_local_metrics_out_of_totals() {
        let user = UserId::from("same_user");
        let first = DatasetStats {
            total_images: 4,
            completed_tasks: 3,
            scoring_version: Some(1),
            per_task: BTreeMap::from([(
                TaskId::from("same_task"),
                TaskStats {
                    completed: 3,
                    ..Default::default()
                },
            )]),
            contributors: Some(BTreeMap::from([(
                user.clone(),
                ContributorStats {
                    display_name: "Person".into(),
                    github_user_id: None,
                    history: vec![ContributorDay {
                        day: "2026-09-29".into(),
                        labeled: 10,
                        accepted: 1,
                        rejected: 1,
                        score: ScoreDay {
                            labeling: 100,
                            ..Default::default()
                        },
                        ..Default::default()
                    }],
                },
            )])),
            throughput: vec![ThroughputPoint {
                day: "2026-09-29".into(),
                annotations: 2,
                reviews: 1,
            }],
            ..Default::default()
        };
        let mut second = first.clone();
        let day = &mut second
            .contributors
            .as_mut()
            .unwrap()
            .get_mut(&user)
            .unwrap()
            .history[0];
        day.accepted = 8;
        day.rejected = 0;
        let total = aggregate_statistics([&first, &second]);
        assert_eq!((total.total_images, total.completed_tasks), (8, 6));
        assert_eq!(total.scoring_version, Some(1));
        let person = &total.contributors.as_ref().unwrap()[&user];
        assert_eq!(person.history.len(), 1);
        assert_eq!(
            (person.history[0].accepted, person.history[0].rejected),
            (9, 1)
        );
        assert_eq!(person.history[0].score.labeling, 200);
        assert_eq!(
            person
                .label_streak(chrono::NaiveDate::from_ymd_opt(2026, 9, 29).unwrap())
                .days,
            1
        );
        assert_eq!(total.throughput[0].annotations, 4);
        assert!(total.per_task.is_empty());
        assert!(total.per_class.is_empty());
        assert!(total.assignment_balance.is_none());
        assert!(total.scoring_focus.is_none());
        assert_eq!(first.per_task.len(), 1);
    }

    #[test]
    fn unavailable_history_or_scoring_is_not_reported_as_zero_or_complete() {
        let current = DatasetStats {
            contributors: Some(BTreeMap::new()),
            scoring_version: Some(1),
            ..Default::default()
        };
        let unavailable = DatasetStats::default();
        let total = aggregate_statistics([&current, &unavailable]);
        assert!(total.contributors.is_none());
        assert!(total.scoring_version.is_none());
        let empty = aggregate_statistics([]);
        assert_eq!(empty.total_images, 0);
        assert_eq!(empty.contributors, Some(BTreeMap::new()));
    }
}
