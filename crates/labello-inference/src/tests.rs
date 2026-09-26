use super::*;

pub(super) fn contract(pose: bool) -> (PrelabelConfig, TaskDefinition) {
    let keypoints = if pose { vec!["nose".into()] } else { vec![] };
    let config = PrelabelConfig {
        config_id: "model".into(),
        name: "Model".into(),
        model: ModelSpec {
            model_id: "yolo".into(),
            display_name: "YOLO".into(),
            version: None,
            location: "model.onnx".into(),
        },
        execution: PrelabelExecution::ServerSide { command: vec![] },
        output_processing: OutputProcessing {
            confidence_threshold: 0.25,
            suppress_overlaps_iou: None,
        },
        available_to_annotators: true,
        yolo: Some(YoloModelSpec {
            input_size: 320,
            class_ids: vec![Some("person".into()), None],
            keypoints,
            ..Default::default()
        }),
    };
    let task = TaskDefinition {
        task_id: "people".into(),
        name: "People".into(),
        annotation_type: if pose {
            AnnotationType::Skeleton
        } else {
            AnnotationType::BoundingBox
        },
        class_ids: vec!["person".into()],
        instructions: TutorialContent {
            title: "".into(),
            example_text: "".into(),
            example_images: vec![],
        },
        skeleton: pose.then(|| SkeletonSpec {
            keypoints: vec![KeypointSpec {
                name: "nose".into(),
                required: true,
            }],
            edges: vec![],
            allow_hidden: true,
            allow_absent: false,
        }),
        review: ReviewConfig::default(),
        prelabel_config_ids: vec!["model".into()],
        manual_box_guide_migration: None,
        enabled: true,
    };
    (config, task)
}

#[test]
fn sparse_mapping_keeps_all_model_scores_and_keypoint_offsets() {
    let (mut config, task) = contract(false);
    let spec = config.yolo.as_mut().unwrap();
    spec.class_ids.clear();
    spec.class_count = Some(80);
    spec.output_name = Some("predictions".into());
    spec.class_mappings = vec![YoloClassMapping {
        model_class_id: 32,
        class_id: "person".into(),
    }];
    let mut data = vec![0.0; 84];
    data[..4].copy_from_slice(&[160.0, 160.0, 80.0, 80.0]);
    data[4 + 32] = 0.9;
    let result = decode(&[1, 84, 1], &data, letterbox(), &config, &task).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].class_id.as_str(), "person");
    data[4 + 79] = 0.95;
    assert!(
        decode(&[1, 84, 1], &data, letterbox(), &config, &task)
            .unwrap()
            .is_empty()
    );
}

fn letterbox() -> Letterbox {
    Letterbox {
        width: 640,
        height: 320,
        scale_x: 0.5,
        scale_y: 0.5,
        left: 0,
        top: 80,
    }
}

#[test]
fn rgb_input_is_letterboxed_and_normalized_in_channel_order() {
    let image = image::RgbImage::from_pixel(4, 2, image::Rgb([255, 128, 0]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let (input, transform) = prepare(bytes.get_ref(), 32).unwrap();
    assert_eq!(
        (
            transform.left,
            transform.top,
            transform.width,
            transform.height
        ),
        (0, 8, 4, 2)
    );
    assert_eq!(input.len(), 3 * 32 * 32);
    assert_eq!(input[0], 114.0 / 255.0);
    assert_eq!(input[8 * 32], 1.0);
    assert_eq!(input[32 * 32 + 8 * 32], 128.0 / 255.0);
    assert_eq!(input[2 * 32 * 32 + 8 * 32], 0.0);
    assert!(prepare(b"not an image", 320).is_err());
}

#[test]
fn detection_coordinates_remove_padding_and_ignored_classes_stay_ignored() {
    let (config, task) = contract(false);
    // Two candidates, channel-major. The second belongs to an ignored model class.
    let data = [
        160.0, 160.0, 160.0, 160.0, 160.0, 160.0, 80.0, 80.0, 0.9, 0.1, 0.1, 0.95,
    ];
    let hints = decode(&[1, 6, 2], &data, letterbox(), &config, &task).unwrap();
    assert_eq!(hints.len(), 1);
    assert_eq!(
        hints[0].geometry,
        AnnotationGeometry::BoundingBox(BoundingBox {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5
        })
    );
    assert_eq!(hints[0].class_id, ClassId::from("person"));
    let mut changed_image_output = data;
    changed_image_output[0] = 80.0;
    assert_ne!(
        decode(
            &[1, 6, 2],
            &changed_image_output,
            letterbox(),
            &config,
            &task
        )
        .unwrap()[0]
            .geometry,
        hints[0].geometry
    );
}

#[test]
fn pose_uses_object_box_nms_and_preserves_named_hidden_keypoints() {
    let (config, task) = contract(true);
    let data = [
        160.0, 160.0, 160.0, 160.0, 160.0, 160.0, 80.0, 80.0, 0.9, 0.8, 0.1, 0.1, 160.0, 150.0,
        160.0, 150.0, 0.2, 0.8,
    ];
    let hints = decode(&[1, 9, 2], &data, letterbox(), &config, &task).unwrap();
    assert_eq!(hints.len(), 1);
    let AnnotationGeometry::Skeleton(skeleton) = &hints[0].geometry else {
        panic!("expected pose");
    };
    assert_eq!(skeleton.keypoints[0].name, "nose");
    assert_eq!(skeleton.keypoints[0].state, KeypointState::Hidden);
    assert_eq!(
        skeleton.keypoints[0].point,
        Some(NormalizedPoint { x: 0.5, y: 0.5 })
    );
}

#[test]
fn unsupported_shapes_nonfinite_outputs_and_invalid_confidence_fail_safely() {
    let (config, task) = contract(false);
    let mut data = [160.0, 160.0, 160.0, 80.0, 0.9, 0.1];
    assert!(
        decode(&[1, 6, 0], &[], letterbox(), &config, &task)
            .unwrap()
            .is_empty()
    );
    assert!(decode(&[1, 1, 6], &data, letterbox(), &config, &task).is_err());
    assert!(decode(&[1, 6, 35_001], &[], letterbox(), &config, &task).is_err());
    data[4] = 1.1;
    assert!(decode(&[1, 6, 1], &data, letterbox(), &config, &task).is_err());
    data[4] = f32::NAN;
    assert!(decode(&[1, 6, 1], &data, letterbox(), &config, &task).is_err());
}
