use super::*;

#[test]
fn image_receipts_use_global_daily_tiers_and_focus_but_isolate_images_users_and_sequences() {
    let mut previous = Vec::new();
    for index in 0..100 {
        save(&mut previous, annotation(&format!("prior-{index}")), time());
    }
    submit(&mut previous, time());
    let other_image = ImageId::from("a-prior-image");
    for event in &mut previous {
        event.image_id = other_image.clone();
    }
    let previous_state = rebuild_state(other_image.clone(), &previous).unwrap();
    let mut events = Vec::new();
    save(&mut events, annotation("current"), time());
    let mut other_author = annotation("other-author");
    other_author.author_user_id = UserId::from("other");
    save(&mut events, other_author, time());
    submit(&mut events, time());
    submit(&mut events, time());
    let image = ImageId::from("image");
    let state = rebuild_state(image.clone(), &events).unwrap();
    let mut projection = ScoringProjection::default();
    // Scan order must not affect the globally ordered daily multiplier.
    projection.record_image(&state, &events);
    projection.record_image(&previous_state, &previous);
    let mut totals = BTreeMap::new();
    let scores = projection.finish(
        &mut totals,
        &[FocusWindow {
            starts_at: time(),
            ends_at: time() + chrono::Duration::minutes(20),
            task_id: Some(TaskId::from("boxes")),
        }],
        &[],
    );
    let author = UserId::from("author");
    assert_eq!(scores.score(&image, &author, 0, 2), Some(0));
    assert_eq!(scores.score(&image, &author, 2, 3), Some(3520));
    assert_eq!(scores.score(&image, &author, 3, 4), Some(0));
    assert_eq!(scores.score(&image, &author, 4, 4), Some(0));
    assert_eq!(scores.score(&image, &author, 0, 5), None);
    assert_eq!(scores.score(&image, &author, 4, 3), None);
    assert_eq!(scores.score(&ImageId::from("missing"), &author, 0, 0), None);
    assert_eq!(
        scores.score(&image, &UserId::from("other"), 0, 4),
        Some(3200)
    );
    assert_eq!(
        scores.score(&image, &UserId::from("uncredited"), 0, 4),
        Some(0)
    );
    assert_eq!(scores.score(&other_image, &author, 0, 101), Some(320_000));
    assert_eq!(totals[&author].history[0].score.total(), 323_520);
}

#[test]
fn review_and_correction_receipts_belong_to_the_actual_awarding_event() {
    let mut events = Vec::new();
    let mut label = annotation("label");
    save(&mut events, label.clone(), time());
    submit(&mut events, time());
    review(&mut events, "reject", 1, ReviewDecision::Rejected, time());
    review(&mut events, "retry", 1, ReviewDecision::Rejected, time());
    label.version = 2;
    label.author_user_id = UserId::from("corrector");
    label.revision_source = RevisionSource::Human {
        action: HumanRevisionKind::Edited,
    };
    if let AnnotationGeometry::BoundingBox(bbox) = &mut label.geometry {
        bbox.width = 0.3;
    }
    save(&mut events, label, time());
    submit(&mut events, time());
    review(&mut events, "approve", 2, ReviewDecision::Approved, time());
    review(
        &mut events,
        "approve-retry",
        2,
        ReviewDecision::Approved,
        time(),
    );
    let image = ImageId::from("image");
    let state = rebuild_state(image.clone(), &events).unwrap();
    let mut projection = ScoringProjection::default();
    projection.record_image(&state, &events);
    let scores = projection.finish(&mut BTreeMap::new(), &[], &[]);
    let author = UserId::from("author");
    let reviewer = UserId::from("reviewer");
    let corrector = UserId::from("corrector");
    assert_eq!(scores.score(&image, &author, 1, 2), Some(2200));
    assert_eq!(scores.score(&image, &author, 2, 3), Some(-1600));
    assert_eq!(scores.score(&image, &reviewer, 2, 3), Some(1600));
    assert_eq!(scores.score(&image, &reviewer, 3, 8), Some(0));
    assert_eq!(scores.score(&image, &author, 3, 8), Some(0));
    assert_eq!(scores.score(&image, &corrector, 0, 6), Some(0));
    assert_eq!(scores.score(&image, &corrector, 6, 7), Some(1600));
    assert_eq!(scores.score(&image, &corrector, 7, 8), Some(0));

    for (starts_at, ends_at, task, expected) in [
        (
            time(),
            time() + chrono::Duration::minutes(10),
            "boxes",
            2400,
        ),
        (
            time(),
            time() + chrono::Duration::minutes(10),
            "other",
            1600,
        ),
        (
            time() + chrono::Duration::seconds(1),
            time() + chrono::Duration::minutes(10),
            "boxes",
            1600,
        ),
        (
            time() - chrono::Duration::minutes(10),
            time(),
            "boxes",
            1600,
        ),
    ] {
        let mut projection = ScoringProjection::default();
        projection.record_image(&state, &events);
        let mut totals = BTreeMap::new();
        let scores = projection.finish(
            &mut totals,
            &[],
            &[FocusWindow {
                starts_at,
                ends_at,
                task_id: Some(TaskId::from(task)),
            }],
        );
        assert_eq!(scores.score(&image, &reviewer, 2, 3), Some(expected));
        assert_eq!(scores.score(&image, &reviewer, 3, 8), Some(0));
        assert_eq!(scores.score(&image, &author, 1, 2), Some(2200));
        assert_eq!(scores.score(&image, &author, 2, 3), Some(-1600));
        assert_eq!(scores.score(&image, &corrector, 6, 7), Some(1600));
        assert_eq!(totals[&reviewer].history[0].score.labels, 0);
        assert_eq!(totals[&reviewer].history[0].score.reviewing, expected);
    }
}
