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
