use super::*;
use crate::{
    DatasetId, KeypointAnnotation, KeypointSpec, KeypointState, LabelClass, NormalizedPoint,
    ReviewConfig, TaskDefinition, TutorialContent, now,
};

fn options(profile: ExportProfile) -> ExportOptions {
    ExportOptions {
        profile,
        classes: BTreeSet::new(),
        fallback_split: ExportSplit::Train,
        split_choices: BTreeMap::new(),
    }
}

type KeypointFixture<'a> = (&'a str, Option<(f32, f32)>, KeypointState);

fn skeleton(points: &[KeypointFixture<'_>]) -> SkeletonGeometry {
    SkeletonGeometry {
        keypoints: points
            .iter()
            .map(|(name, point, state)| KeypointAnnotation {
                name: (*name).into(),
                state: state.clone(),
                point: point.map(|(x, y)| NormalizedPoint { x, y }),
            })
            .collect(),
    }
}

#[test]
fn pose_envelope_uses_hidden_points_and_one_original_pixel_at_edges() {
    let dimensions = ImageDimensions {
        width: 100,
        height: 200,
    };
    let point = skeleton(&[("edge", Some((1.0, 0.0)), KeypointState::Hidden)]);
    let (bbox, derived) = export_pose_bounds(&point, dimensions, None).unwrap();
    assert!(derived);
    assert!((bbox.x - 0.99).abs() < 1e-6);
    assert_eq!(bbox.y, 0.0);
    assert!((bbox.width - 0.01).abs() < 1e-6);
    assert!((bbox.height - 0.005).abs() < 1e-6);
    let points = skeleton(&[
        ("a", Some((0.1, 0.2)), KeypointState::Visible),
        ("b", Some((0.8, 0.9)), KeypointState::Hidden),
        ("c", None, KeypointState::Absent),
    ]);
    let (bbox, _) = export_pose_bounds(&points, dimensions, None).unwrap();
    assert!((bbox.x - 0.1).abs() < 1e-6);
    assert!((bbox.y - 0.2).abs() < 1e-6);
    assert!((bbox.width - 0.7).abs() < 1e-6);
    assert!((bbox.height - 0.7).abs() < 1e-6);
}

#[test]
fn absent_pose_requires_a_valid_explicit_box_and_never_places_origin_points() {
    let dimensions = ImageDimensions {
        width: 100,
        height: 100,
    };
    let pose = skeleton(&[("missing", None, KeypointState::Absent)]);
    assert_eq!(
        export_pose_bounds(&pose, dimensions, None),
        Err(ExportPolicyError::PoseWithoutBounds)
    );
    let bbox = BoundingBox {
        x: 0.2,
        y: 0.3,
        width: 0.4,
        height: 0.5,
    };
    assert_eq!(
        export_pose_bounds(&pose, dimensions, Some(bbox)),
        Ok((bbox, false))
    );
    assert!(pose.keypoints[0].point.is_none());
    assert_eq!(
        export_pose_bounds(&pose, dimensions, Some(BoundingBox { width: 0.0, ..bbox })),
        Err(ExportPolicyError::InvalidGeometry)
    );
}

#[test]
fn split_provenance_is_preserved_and_conflicts_require_an_explicit_choice() {
    let image = ImageId::from("image");
    let membership = |split: &str| split.to_owned();
    let mut options = options(ExportProfile::UltralyticsYoloDetectV1);
    assert_eq!(options.image_split(&image, &[]), Ok(ExportSplit::Train));
    assert_eq!(
        options.image_split(&image, &[membership("test"), membership("test")]),
        Ok(ExportSplit::Test)
    );
    let conflict = [membership("train"), membership("val")];
    assert_eq!(
        options.image_split(&image, &conflict),
        Err(ExportPolicyError::SplitConflict)
    );
    options
        .split_choices
        .insert(image.clone(), ExportSplit::Val);
    assert_eq!(options.image_split(&image, &conflict), Ok(ExportSplit::Val));
}

fn add_class(
    dataset: &mut DatasetMetadata,
    task_id: &str,
    class_id: &str,
    names: &[&str],
) -> ExportClassSelection {
    let task_id = TaskId::from(task_id);
    let class_id = ClassId::from(class_id);
    dataset.label_classes.push(LabelClass {
        class_id: class_id.clone(),
        name: "Same display name".into(),
        color: "#ffffff".into(),
        description: None,
    });
    dataset.tasks.push(TaskDefinition {
        task_id: task_id.clone(),
        name: "Pose".into(),
        annotation_type: AnnotationType::Skeleton,
        class_ids: vec![class_id.clone()],
        instructions: TutorialContent {
            title: "Pose".into(),
            example_text: String::new(),
            example_images: Vec::new(),
        },
        skeleton: Some(SkeletonSpec {
            keypoints: names
                .iter()
                .map(|name| KeypointSpec {
                    name: (*name).into(),
                    required: true,
                })
                .collect(),
            edges: Vec::new(),
            allow_hidden: true,
            allow_absent: true,
        }),
        review: ReviewConfig::default(),
        prelabel_config_ids: Vec::new(),
        manual_box_guide_migration: None,
        enabled: true,
    });
    ExportClassSelection { task_id, class_id }
}

#[test]
fn mapping_preserves_distinct_class_identities_and_per_class_keypoint_order() {
    let mut dataset = DatasetMetadata::new(DatasetId::from("dataset"), "Export", now());
    let b = add_class(&mut dataset, "b", "class_b", &["right", "left"]);
    let a = add_class(&mut dataset, "a", "class_a", &["tail", "nose"]);
    let mut options = options(ExportProfile::UltralyticsYoloPoseV1);
    options.classes = BTreeSet::from([b, a.clone()]);
    let mapping = options.class_mapping(&dataset).unwrap();
    assert_eq!(mapping.len(), 2);
    assert_eq!(mapping[0].selection, a);
    assert_eq!(mapping[0].index, 0);
    assert_eq!(mapping[1].index, 1);
    assert_eq!(mapping[0].name, mapping[1].name);
    assert_eq!(
        mapping[0].skeleton.as_ref().unwrap().keypoints[0].name,
        "tail"
    );
    assert_eq!(
        mapping[1].skeleton.as_ref().unwrap().keypoints[0].name,
        "right"
    );
    dataset.tasks[0].skeleton.as_mut().unwrap().keypoints.pop();
    assert_eq!(
        options.class_mapping(&dataset),
        Err(ExportPolicyError::IncompatibleSkeletons)
    );
    dataset.tasks[0].annotation_type = AnnotationType::BoundingBox;
    assert_eq!(
        options.class_mapping(&dataset),
        Err(ExportPolicyError::IncompatibleSelection)
    );
}
