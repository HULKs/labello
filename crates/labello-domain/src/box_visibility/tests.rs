use super::*;
use crate::{AnnotationType, BoundingBox, ReviewRecord, now};

fn box_annotation(id: &str, x: f32) -> AnnotationVersion {
    AnnotationVersion::native(
        id.into(),
        "boxes".into(),
        "person".into(),
        AnnotationType::BoundingBox,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        }),
        "annotator".into(),
        now(),
    )
}

fn state_with(annotations: &[AnnotationVersion]) -> ImageState {
    let mut state = ImageState::new("image".into());
    for annotation in annotations {
        state
            .annotations
            .insert(annotation.annotation_id.clone(), vec![annotation.clone()]);
    }
    state
}

#[test]
fn defaults_and_threshold_validation() {
    assert_eq!(BoundingBoxVisibility::default().iou_threshold, 0.9);
    for iou_threshold in [0.0, 0.9, 1.0] {
        BoundingBoxVisibility { iou_threshold }.validate().unwrap();
    }
    for iou_threshold in [-0.1, 1.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(BoundingBoxVisibility { iou_threshold }.validate().is_err());
    }
}

#[test]
fn duplicates_compete_across_workflows_but_not_classes_and_preserve_records() {
    let a = box_annotation("a", 0.0);
    let mut b = box_annotation("b", 0.0);
    b.task_id = "other-box-workflow".into();
    let mut c = box_annotation("c", 0.0);
    c.class_id = "vehicle".into();
    let state = state_with(&[a.clone(), b.clone(), c.clone()]);
    let before = state.clone();
    let expected = BTreeMap::from([("b".into(), "a".into())]);
    let policy = BoundingBoxVisibility::default();
    assert_eq!(policy.exclusions(&state, [&c, &b, &a]).unwrap(), expected);
    assert_eq!(
        policy
            .exclusions(&state, state.active_annotations())
            .unwrap(),
        expected
    );
    assert_eq!(state, before);
    assert_eq!(state.active_annotations().count(), 3);
    assert!(
        policy
            .exclusions(&ImageState::new("other-image".into()), [&b])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn strict_boundary_and_nontransitive_groups_keep_deterministic_winners() {
    let annotations = [
        box_annotation("a", 0.0),
        box_annotation("b", 0.1),
        box_annotation("c", 0.2),
    ];
    let state = state_with(&annotations);
    let policy = BoundingBoxVisibility { iou_threshold: 0.6 };
    assert_eq!(
        policy.exclusions(&state, &annotations).unwrap(),
        BTreeMap::from([("b".into(), "a".into())])
    );
    let duplicate = box_annotation("d", 0.0);
    let policy = BoundingBoxVisibility { iou_threshold: 1.0 };
    assert!(
        policy
            .exclusions(&state, [&annotations[0], &duplicate])
            .unwrap()
            .is_empty()
    );
    let AnnotationGeometry::BoundingBox(a) = annotations[0].geometry else {
        unreachable!()
    };
    let AnnotationGeometry::BoundingBox(b) = annotations[1].geometry else {
        unreachable!()
    };
    let policy = BoundingBoxVisibility {
        iou_threshold: a.iou(b),
    };
    assert!(
        policy
            .exclusions(&state, [&annotations[0], &annotations[1]])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn current_approval_wins_but_superseded_review_or_changed_draft_does_not() {
    let a = box_annotation("a", 0.0);
    let b = box_annotation("b", 0.0);
    let mut state = state_with(&[a.clone(), b.clone()]);
    state.reviews.push(ReviewRecord {
        review_id: "review".into(),
        target: ReviewTarget::AnnotationVersion {
            annotation_id: b.annotation_id.clone(),
            version: b.version,
        },
        reviewer_user_id: "reviewer".into(),
        decision: ReviewDecision::Approved,
        timestamp: now(),
        comment: None,
    });
    let policy = BoundingBoxVisibility::default();
    assert_eq!(
        policy.exclusions(&state, [&a, &b]).unwrap(),
        BTreeMap::from([("a".into(), "b".into())])
    );
    let mut draft = b.clone();
    if let AnnotationGeometry::BoundingBox(bounds) = &mut draft.geometry {
        bounds.x = 0.001;
    }
    assert_eq!(
        policy.exclusions(&state, [&a, &draft]).unwrap(),
        BTreeMap::from([("b".into(), "a".into())])
    );
    state.superseded_review_ids.insert("review".into());
    assert_eq!(
        policy.exclusions(&state, [&a, &b]).unwrap(),
        BTreeMap::from([("b".into(), "a".into())])
    );
}

#[test]
fn moving_or_deleting_winner_reveals_preserved_duplicate() {
    let mut a = box_annotation("a", 0.0);
    let b = box_annotation("b", 0.0);
    let state = state_with(&[a.clone(), b.clone()]);
    let policy = BoundingBoxVisibility::default();
    a.deleted = true;
    assert!(policy.exclusions(&state, [&a, &b]).unwrap().is_empty());
    a.deleted = false;
    if let AnnotationGeometry::BoundingBox(bounds) = &mut a.geometry {
        bounds.x = 0.5;
    }
    assert!(policy.exclusions(&state, [&a, &b]).unwrap().is_empty());
    assert_eq!(state.active_annotations().count(), 2);
}

#[test]
fn visibility_wire_is_optional_for_history_and_validated_before_replay() {
    use crate::{
        DatasetConfig, DatasetMetadata, DatasetRole, EventLogEntry, EventPayload, rebuild_state,
    };
    let annotation = box_annotation("a", 0.0);
    let mut event = EventLogEntry::new(
        1,
        "image".into(),
        "annotator".into(),
        DatasetRole::Annotator,
        now(),
        EventPayload::AnnotationVersionCreated {
            annotation,
            previous_version: None,
            reason: None,
        },
    );
    for version in [crate::LEGACY_SCHEMA_VERSION, crate::SCHEMA_VERSION] {
        event.schema_version = version;
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.get("boundingBoxVisibility").is_none());
        let decoded: EventLogEntry = serde_json::from_value(json).unwrap();
        assert_eq!(
            rebuild_state("image".into(), &[decoded])
                .unwrap()
                .bounding_box_visibility,
            None
        );
    }
    event.bounding_box_visibility = Some(Default::default());
    let decoded =
        serde_json::from_str::<EventLogEntry>(&serde_json::to_string(&event).unwrap()).unwrap();
    assert_eq!(
        rebuild_state("image".into(), &[decoded])
            .unwrap()
            .bounding_box_visibility,
        Some(Default::default())
    );
    event.schema_version = crate::LEGACY_SCHEMA_VERSION;
    assert!(serde_json::to_value(&event).is_err());
    assert!(ImageState::new("image".into()).apply_event(&event).is_err());
    event.schema_version = crate::SCHEMA_VERSION;
    let mut json = serde_json::to_value(&event).unwrap();
    for invalid in [-0.1, 1.1] {
        json["boundingBoxVisibility"]["iouThreshold"] = serde_json::json!(invalid);
        assert!(serde_json::from_value::<EventLogEntry>(json.clone()).is_err());
    }
    let metadata = DatasetMetadata::new("dataset".into(), "Dataset", now());
    let mut config = serde_json::to_value(DatasetConfig::from_metadata(&metadata)).unwrap();
    config
        .as_object_mut()
        .unwrap()
        .remove("boundingBoxVisibility");
    assert_eq!(
        serde_json::from_value::<DatasetConfig>(config)
            .unwrap()
            .bounding_box_visibility,
        Default::default()
    );
}

#[test]
fn rejected_winner_yields_to_unreviewed_box_and_identical_iou_is_at_most_one() {
    let mut a = box_annotation("a", 0.1);
    a.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.2,
        height: 0.2,
    });
    let mut b = a.clone();
    b.annotation_id = "b".into();
    let mut state = state_with(&[a.clone(), b.clone()]);
    state.reviews.push(ReviewRecord {
        review_id: "rejection".into(),
        target: ReviewTarget::AnnotationVersion {
            annotation_id: a.annotation_id.clone(),
            version: a.version,
        },
        reviewer_user_id: "reviewer".into(),
        decision: ReviewDecision::Rejected,
        timestamp: now(),
        comment: None,
    });
    assert_eq!(
        BoundingBoxVisibility::default()
            .exclusions(&state, [&a, &b])
            .unwrap(),
        BTreeMap::from([("a".into(), "b".into())])
    );
    assert!(
        BoundingBoxVisibility { iou_threshold: 1.0 }
            .exclusions(&state, [&a, &b])
            .unwrap()
            .is_empty()
    );
}
