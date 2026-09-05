use super::*;
use crate::{
    AnnotationId, ClassId, ImageId, KeypointAnnotation, KeypointState, NormalizedPoint,
    ObjectGroupId, SkeletonGeometry, TaskId, UserId, now,
};

fn pose() -> AnnotationVersion {
    let mut annotation = AnnotationVersion::native(
        AnnotationId::from("pose"),
        TaskId::from("poses"),
        ClassId::from("person"),
        AnnotationType::Skeleton,
        AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![KeypointAnnotation {
                name: "nose".into(),
                state: KeypointState::Hidden,
                point: Some(NormalizedPoint { x: 0.4, y: 0.6 }),
            }],
        }),
        UserId::from("author"),
        now(),
    );
    annotation.object_group_id = Some(ObjectGroupId::from("group"));
    annotation
}

#[test]
fn pose_bounds_use_explicit_current_links_and_reject_ambiguous_boxes() {
    let pose = pose();
    let mut image = ImageState::new(ImageId::from("image"));
    image
        .annotations
        .insert(pose.annotation_id.clone(), vec![pose.clone()]);
    let dimensions = ImageDimensions {
        width: 100,
        height: 100,
    };
    assert!(image.export_pose_box(&pose, dimensions).unwrap().derived);
    let mut bbox = pose.clone();
    bbox.annotation_id = AnnotationId::from("box");
    bbox.task_id = TaskId::from("boxes");
    bbox.annotation_type = AnnotationType::BoundingBox;
    bbox.geometry = AnnotationGeometry::BoundingBox(crate::BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.8,
        height: 0.8,
    });
    image
        .annotations
        .insert(bbox.annotation_id.clone(), vec![bbox.clone()]);
    let linked = image.export_pose_box(&pose, dimensions).unwrap();
    assert!(!linked.derived);
    assert_eq!(
        linked.linked_annotation,
        Some((bbox.annotation_id.clone(), 1))
    );
    bbox.annotation_id = AnnotationId::from("ambiguous");
    image
        .annotations
        .insert(bbox.annotation_id.clone(), vec![bbox]);
    assert_eq!(
        image.export_pose_box(&pose, dimensions),
        Err(ExportPolicyError::AmbiguousBox)
    );
}

#[test]
fn stale_automatic_links_and_suggestions_are_never_reused() {
    let pose = pose();
    let mut bbox = pose.clone();
    bbox.annotation_id = AnnotationId::from("box");
    bbox.annotation_type = AnnotationType::BoundingBox;
    bbox.geometry = AnnotationGeometry::BoundingBox(crate::BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.8,
        height: 0.8,
    });
    let mut image = ImageState::new(ImageId::from("image"));
    image
        .annotations
        .insert(pose.annotation_id.clone(), vec![pose.clone()]);
    let dimensions = ImageDimensions {
        width: 100,
        height: 100,
    };
    for source in [
        RevisionSource::MigrationSkeleton {
            annotation_id: pose.annotation_id.clone(),
            version: 0,
        },
        RevisionSource::PrelabelSuggestion {
            config_id: "model".into(),
            model_id: "model".into(),
            confidence: 0.9,
        },
    ] {
        bbox.revision_source = source;
        image
            .annotations
            .insert(bbox.annotation_id.clone(), vec![bbox.clone()]);
        assert!(image.export_pose_box(&pose, dimensions).unwrap().derived);
    }
    let mut stale = pose.clone();
    stale.version += 1;
    assert_eq!(
        image.export_pose_box(&stale, dimensions),
        Err(ExportPolicyError::InvalidGeometry)
    );
}

#[test]
fn rejected_linked_box_versions_cannot_be_exported_as_authoritative_bounds() {
    let pose = pose();
    let mut bbox = pose.clone();
    bbox.annotation_id = AnnotationId::from("box");
    bbox.task_id = TaskId::from("boxes");
    bbox.annotation_type = AnnotationType::BoundingBox;
    bbox.geometry = AnnotationGeometry::BoundingBox(crate::BoundingBox {
        x: 0.1,
        y: 0.1,
        width: 0.8,
        height: 0.8,
    });
    let mut image = ImageState::new(ImageId::from("image"));
    image
        .annotations
        .insert(pose.annotation_id.clone(), vec![pose.clone()]);
    image
        .annotations
        .insert(bbox.annotation_id.clone(), vec![bbox.clone()]);
    let dimensions = ImageDimensions {
        width: 100,
        height: 100,
    };
    // Unselected box-task completeness is not selected pose coverage. A valid
    // current human box can be used until an effective decision rejects it.
    assert!(!image.export_pose_box(&pose, dimensions).unwrap().derived);
    let rejection = crate::ReviewRecord {
        review_id: "box-rejection".into(),
        target: crate::ReviewTarget::AnnotationVersion {
            annotation_id: bbox.annotation_id.clone(),
            version: bbox.version,
        },
        reviewer_user_id: "reviewer".into(),
        decision: crate::ReviewDecision::Rejected,
        timestamp: now(),
        comment: None,
    };
    image.reviews.push(rejection.clone());
    let derived = image.export_pose_box(&pose, dimensions).unwrap();
    assert!(derived.derived);
    assert!(derived.linked_annotation.is_none());
    let mut absent = pose.clone();
    if let AnnotationGeometry::Skeleton(geometry) = &mut absent.geometry {
        geometry.keypoints[0].state = KeypointState::Absent;
        geometry.keypoints[0].point = None;
    }
    image
        .annotations
        .insert(absent.annotation_id.clone(), vec![absent.clone()]);
    assert_eq!(
        image.export_pose_box(&absent, dimensions),
        Err(ExportPolicyError::PoseWithoutBounds)
    );
    // Committed decision replacement removes the rejection from effective policy.
    image.superseded_review_ids.insert(rejection.review_id);
    assert!(!image.export_pose_box(&absent, dimensions).unwrap().derived);
}
