use super::*;
use labello_domain::{
    AnnotationType, ClassId, ExportClassSelection, KeypointAnnotation, KeypointSpec,
    NormalizedPoint, SkeletonGeometry, SkeletonSpec, TaskId, UserId, now,
};

fn bbox() -> BoundingBox {
    BoundingBox {
        x: 0.1,
        y: 0.2,
        width: 0.3,
        height: 0.4,
    }
}

fn annotation(id: &str, geometry: AnnotationGeometry) -> AnnotationVersion {
    let kind = match geometry {
        AnnotationGeometry::BoundingBox(_) => AnnotationType::BoundingBox,
        AnnotationGeometry::Skeleton(_) => AnnotationType::Skeleton,
    };
    AnnotationVersion::native(
        AnnotationId::from(id),
        TaskId::from("task"),
        ClassId::from("class"),
        kind,
        geometry,
        UserId::from("author"),
        now(),
    )
}

#[test]
fn labels_are_stably_ordered_and_preserve_detect_coordinates() {
    let a = annotation("a", AnnotationGeometry::BoundingBox(bbox()));
    let b = annotation("b", AnnotationGeometry::BoundingBox(bbox()));
    let mut rows = [
        Row {
            annotation: &b,
            class_index: 2,
            bbox: bbox(),
            derived_box: false,
        },
        Row {
            annotation: &a,
            class_index: 0,
            bbox: bbox(),
            derived_box: false,
        },
    ];
    let (bytes, trace) = labels(ExportProfile::UltralyticsYoloDetectV1, &mut rows).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    assert!(lines[0].starts_with("0 "));
    assert!(lines[1].starts_with("2 "));
    let values = lines[0]
        .split_whitespace()
        .skip(1)
        .map(|value| value.parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert!((values[0] - values[2] / 2.0 - f64::from(bbox().x)).abs() < 1e-9);
    assert!((values[1] - values[3] / 2.0 - f64::from(bbox().y)).abs() < 1e-9);
    assert_eq!(trace[0].annotation_id, a.annotation_id);
    assert_eq!(trace[0].row, 1);
    assert!(
        labels(ExportProfile::UltralyticsYoloDetectV1, &mut [])
            .unwrap()
            .0
            .is_empty()
    );
    rows[1].class_index = rows[0].class_index;
    assert_eq!(
        labels(ExportProfile::UltralyticsYoloDetectV1, &mut rows).unwrap_err(),
        ExportFailure::AmbiguousObjects
    );
    rows[1].annotation = &a;
    assert_eq!(
        labels(ExportProfile::UltralyticsYoloDetectV1, &mut rows).unwrap_err(),
        ExportFailure::InvalidInput
    );
}

#[test]
fn pose_visibility_and_absence_survive_encoding_with_derived_box_trace() {
    let pose = annotation(
        "pose",
        AnnotationGeometry::Skeleton(SkeletonGeometry {
            keypoints: vec![
                KeypointAnnotation {
                    name: "edge".into(),
                    state: KeypointState::Visible,
                    point: Some(NormalizedPoint { x: 1.0, y: 0.0 }),
                },
                KeypointAnnotation {
                    name: "hidden".into(),
                    state: KeypointState::Hidden,
                    point: Some(NormalizedPoint { x: 0.3, y: 0.4 }),
                },
                KeypointAnnotation {
                    name: "absent".into(),
                    state: KeypointState::Absent,
                    point: None,
                },
            ],
        }),
    );
    let (bytes, trace) = labels(
        ExportProfile::UltralyticsYoloPoseV1,
        &mut [Row {
            annotation: &pose,
            class_index: 0,
            bbox: bbox(),
            derived_box: true,
        }],
    )
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let values = text.split_whitespace().collect::<Vec<_>>();
    assert_eq!(&values[5..8], &["1.000000000", "0.000000000", "2"]);
    assert_eq!(values[10], "1");
    assert_eq!(&values[11..14], &["0.000000000", "0.000000000", "0"]);
    assert!(trace[0].derived_box);
}

#[test]
fn descriptor_preserves_identity_and_keypoint_names_without_inventing_flips() {
    let class = |index, id: &str, keypoint: &str| ExportClassMapping {
        index,
        selection: ExportClassSelection {
            task_id: TaskId::from(id),
            class_id: ClassId::from(id),
        },
        name: "same: \"quoted\"\nname".into(),
        skeleton: Some(SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: keypoint.into(),
                required: false,
            }],
            edges: vec![],
            allow_hidden: true,
            allow_absent: true,
        }),
    };
    let output = descriptor(
        ExportProfile::UltralyticsYoloPoseV1,
        &[class(0, "a", "nose"), class(1, "b", "tail")],
    )
    .unwrap();
    let yaml = String::from_utf8(output).unwrap();
    assert!(yaml.contains("kpt_shape: [1, 3]\nkpt_names:\n  0: [\"nose\"]\n  1: [\"tail\"]"));
    assert_eq!(yaml.matches("same: ").count(), 2);
    assert!(!yaml.contains("flip_idx"));
    assert!(!yaml.contains("path:"));
    assert!(yaml.contains("train: train.txt\nval: val.txt\ntest: test.txt"));
    let digest = "a".repeat(64);
    assert_eq!(
        image_paths(ExportSplit::Val, &digest, "png").unwrap(),
        (
            format!("images/val/{digest}.png"),
            format!("labels/val/{digest}.txt"),
        )
    );
    assert!(image_paths(ExportSplit::Train, "../bad", "png").is_err());
    assert!(image_paths(ExportSplit::Train, &digest, "../png").is_err());
}
