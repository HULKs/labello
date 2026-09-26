use super::*;
use prost::Message;

fn value(name: &str, shape: &[i64]) -> pb::ValueInfoProto {
    pb::ValueInfoProto {
        name: name.into(),
        r#type: Some(pb::TypeProto {
            value: Some(pb::type_proto::Value::TensorType(pb::type_proto::Tensor {
                elem_type: 1,
                shape: Some(pb::TensorShapeProto {
                    dim: shape
                        .iter()
                        .map(|&value| pb::tensor_shape_proto::Dimension {
                            value: Some(pb::tensor_shape_proto::dimension::Value::DimValue(value)),
                            ..Default::default()
                        })
                        .collect(),
                }),
            })),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn model() -> pb::ModelProto {
    pb::ModelProto {
        ir_version: 9,
        opset_import: vec![pb::OperatorSetIdProto {
            domain: "".into(),
            version: 17,
        }],
        metadata_props: vec![
            pb::StringStringEntryProto {
                key: "task".into(),
                value: "detect".into(),
            },
            pb::StringStringEntryProto {
                key: "names".into(),
                value: "{0: 'person', 1: 'ball'}".into(),
            },
        ],
        graph: Some(pb::GraphProto {
            name: "inspection_fixture".into(),
            input: vec![value("images", &[1, 3, 32, 32])],
            output: vec![value("auxiliary", &[1]), value("predictions", &[1, 6, 1])],
            initializer: vec![
                pb::TensorProto {
                    name: "auxiliary".into(),
                    dims: vec![1],
                    data_type: 1,
                    float_data: vec![0.0],
                    ..Default::default()
                },
                pb::TensorProto {
                    name: "predictions".into(),
                    dims: vec![1, 6, 1],
                    data_type: 1,
                    float_data: vec![16.0, 16.0, 10.0, 10.0, 0.9, 0.1],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn discovers_named_outputs_and_classes_without_running_an_image() {
    let inspected = inspect(&model().encode_to_vec()).unwrap();
    assert_eq!(inspected.problem, None);
    assert_eq!(inspected.input_size, Some(32));
    assert_eq!(inspected.outputs[0].profile, None);
    assert_eq!(inspected.outputs[1].tensor.name, "predictions");
    let profile = inspected.outputs[1].profile.as_ref().unwrap();
    assert_eq!(profile.class_count, 2);
    assert_eq!(profile.class_names, ["person", "ball"]);
}

#[test]
fn inference_uses_selected_output_and_rejects_missing_name_or_replaced_model() {
    let bytes = model().encode_to_vec();
    let (mut config, task) = crate::tests::contract(false);
    let spec = config.yolo.as_mut().unwrap();
    spec.input_size = 32;
    spec.use_explicit_mapping(2);
    spec.output_name = Some("predictions".into());
    spec.model_digest = Some(blake3::hash(&bytes).to_hex().to_string());
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(32, 32)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let result = infer(&bytes, image.get_ref(), &config, &task).unwrap();
    assert_eq!(result.suggestions.len(), 1);
    assert_eq!(result.suggestions[0].class_id.as_str(), "person");
    config.yolo.as_mut().unwrap().output_name = Some("missing".into());
    assert!(infer(&bytes, image.get_ref(), &config, &task).is_err());
    config.yolo.as_mut().unwrap().output_name = Some("predictions".into());
    config.yolo.as_mut().unwrap().model_digest = Some("0".repeat(64));
    assert!(infer(&bytes, image.get_ref(), &config, &task).is_err());
}

#[test]
fn metadata_and_output_disagreements_and_unsupported_layouts_are_explained() {
    let mut model = model();
    model.metadata_props[1].value = "{0: 'person'}".into();
    let inspected = inspect(&model.encode_to_vec()).unwrap();
    assert_eq!(
        inspected.outputs[1].problem.as_deref(),
        Some("class metadata disagrees with output dimensions")
    );
    model.metadata_props.clear();
    assert!(inspect(b"not onnx").is_err());
    model.graph.as_mut().unwrap().initializer[0].data_location = Some(1);
    assert!(inspect(&model.encode_to_vec()).is_err());
}

#[test]
fn detection_metadata_is_optional_and_class_names_accept_the_classes_alias() {
    for class_names in [None, Some("names"), Some("classes")] {
        let mut model = model();
        model.metadata_props = class_names
            .map(|key| pb::StringStringEntryProto {
                key: key.into(),
                value: r#"{"0": "person", "1": "ball"}"#.into(),
            })
            .into_iter()
            .collect();
        let inspected = inspect(&model.encode_to_vec()).unwrap();
        assert_eq!(inspected.problem, None);
        assert_eq!(inspected.outputs[0].profile, None);
        let profile = inspected.outputs[1].profile.as_ref().unwrap();
        assert_eq!((profile.class_count, profile.keypoint_count), (2, 0));
        assert_eq!(
            profile.class_names,
            if class_names.is_some() {
                vec!["person", "ball"]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn task_metadata_accepts_plain_and_json_strings_without_ignoring_other_values() {
    for task in ["detect", r#""detect""#, r#" "de\u0074ect" "#] {
        let mut model = model();
        model.metadata_props[0].value = task.into();
        let inspected = inspect(&model.encode_to_vec()).unwrap();
        assert_eq!(inspected.problem, None);
        assert_eq!(inspected.outputs[1].problem, None, "task {task}");
        assert_eq!(
            inspected.outputs[1].profile.as_ref().unwrap().class_count,
            2
        );
    }
    for task in [
        r#""segment""#,
        r#""pose""#,
        r#""detect"#,
        "null",
        "true",
        "1",
        r#"["detect"]"#,
    ] {
        let mut model = model();
        model.metadata_props[0].value = task.into();
        assert!(
            inspect(&model.encode_to_vec()).unwrap().outputs[1]
                .profile
                .is_none(),
            "task {task}"
        );
    }
    let mut model = model();
    model.metadata_props[0].value = r#""detect""#.into();
    model.metadata_props.push(pb::StringStringEntryProto {
        key: "kpt_shape".into(),
        value: "[17, 3]".into(),
    });
    assert_eq!(
        inspect(&model.encode_to_vec()).unwrap().outputs[1]
            .problem
            .as_deref(),
        Some("detection metadata must not declare keypoints")
    );
}

#[test]
fn class_aliases_must_be_valid_consistent_and_match_output_channels() {
    for (classes, expected_problem) in [
        (r#"{"0": "person", "1": "ball"}"#, None),
        (
            r#"{"0": "ball", "1": "person"}"#,
            Some("names and classes metadata disagree"),
        ),
        (
            r#"{"1": "ball"}"#,
            Some("invalid model class-name metadata"),
        ),
    ] {
        let mut model = model();
        model.metadata_props.push(pb::StringStringEntryProto {
            key: "classes".into(),
            value: classes.into(),
        });
        let inspected = inspect(&model.encode_to_vec()).unwrap();
        assert_eq!(inspected.outputs[1].problem.as_deref(), expected_problem);
    }
    let mut model = model();
    model.metadata_props = vec![pb::StringStringEntryProto {
        key: "classes".into(),
        value: r#"{"0": "person"}"#.into(),
    }];
    assert_eq!(
        inspect(&model.encode_to_vec()).unwrap().outputs[1]
            .problem
            .as_deref(),
        Some("class metadata disagrees with output dimensions")
    );
}

#[test]
fn optional_metadata_does_not_relax_static_shapes_or_explicit_task_validation() {
    let mut symbolic = model();
    symbolic.metadata_props.clear();
    let output = &mut symbolic.graph.as_mut().unwrap().output[1];
    let Some(pb::type_proto::Value::TensorType(tensor)) =
        output.r#type.as_mut().unwrap().value.as_mut()
    else {
        panic!("tensor fixture");
    };
    tensor.shape.as_mut().unwrap().dim[2].value = Some(
        pb::tensor_shape_proto::dimension::Value::DimParam("candidates".into()),
    );
    assert_eq!(
        inspect(&symbolic.encode_to_vec()).unwrap().outputs[1]
            .problem
            .as_deref(),
        Some("output must have static shape [1, channels, candidates]")
    );
    for task in ["segment", "classify", "unknown", "pose"] {
        let mut model = model();
        model.metadata_props[0].value = task.into();
        assert!(
            inspect(&model.encode_to_vec()).unwrap().outputs[1]
                .profile
                .is_none()
        );
    }
    for (task, shape) in [(None, "[17, 2]"), (Some("detect"), "[17, 3]")] {
        let mut model = model();
        model
            .metadata_props
            .retain(|p| p.key != "task" || task.is_some());
        model.metadata_props.push(pb::StringStringEntryProto {
            key: "kpt_shape".into(),
            value: shape.into(),
        });
        assert!(
            inspect(&model.encode_to_vec()).unwrap().outputs[1]
                .profile
                .is_none()
        );
    }
}

#[test]
fn pose_count_subtracts_keypoint_channels_and_checks_declared_classes() {
    let mut model = model();
    model.metadata_props[0].value = "pose".into();
    model.metadata_props[1].value = "{0: 'person'}".into();
    model.metadata_props.push(pb::StringStringEntryProto {
        key: "kpt_shape".into(),
        value: "[17, 3]".into(),
    });
    let graph = model.graph.as_mut().unwrap();
    graph.output[1] = value("predictions", &[1, 56, 1]);
    graph.initializer[1].dims = vec![1, 56, 1];
    graph.initializer[1].float_data = vec![0.0; 56];
    let inspected = inspect(&model.encode_to_vec()).unwrap();
    let profile = inspected.outputs[1].profile.as_ref().unwrap();
    assert_eq!((profile.class_count, profile.keypoint_count), (1, 17));
    model.metadata_props[0].value = r#""pose""#.into();
    let inspected = inspect(&model.encode_to_vec()).unwrap();
    let profile = inspected.outputs[1].profile.as_ref().unwrap();
    assert_eq!((profile.class_count, profile.keypoint_count), (1, 17));
    model
        .metadata_props
        .retain(|property| property.key != "task");
    let inspected = inspect(&model.encode_to_vec()).unwrap();
    let profile = inspected.outputs[1].profile.as_ref().unwrap();
    assert_eq!((profile.class_count, profile.keypoint_count), (1, 17));
}

#[test]
fn class_names_accept_python_and_json_strings_but_never_expressions_or_sparse_ids() {
    assert_eq!(
        names::parse(r#"{0: "person's hat", 1: 'ball, \'red\'', 2: '\u00e4'}"#).unwrap(),
        ["person's hat", "ball, 'red'", "ä"]
    );
    assert_eq!(
        names::parse(r#"{"0": "person", "1": "ball"}"#).unwrap(),
        ["person", "ball"]
    );
    for invalid in [
        "{1: 'person'}",
        "{0: 'a', 0: 'b'}",
        "{0: execute()}",
        "{0: 'a'} trailing",
        "{0: ''}",
    ] {
        assert!(names::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn compiled_session_reuses_model_but_revalidates_each_requests_mapping_and_digest() {
    let bytes = model().encode_to_vec();
    let mut session = crate::InferenceSession::new(&bytes, NativeProvider::Cpu, 1).unwrap();
    let (mut config, mut task) = crate::tests::contract(false);
    let spec = config.yolo.as_mut().unwrap();
    spec.input_size = 32;
    spec.output_name = Some("predictions".into());
    spec.model_digest = Some(crate::model_digest(&bytes));
    drop(bytes);
    let image = image::RgbImage::from_pixel(32, 32, image::Rgb([128, 128, 128]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let first = session.infer(encoded.get_ref(), &config, &task).unwrap();
    assert_eq!(first.suggestions[0].class_id.as_str(), "person");
    config.yolo.as_mut().unwrap().class_ids[0] = Some("other".into());
    task.class_ids[0] = "other".into();
    let second = session.infer(encoded.get_ref(), &config, &task).unwrap();
    assert_eq!(second.suggestions[0].class_id.as_str(), "other");
    assert_eq!(
        first.suggestions[0].geometry,
        second.suggestions[0].geometry
    );
    config.yolo.as_mut().unwrap().model_digest = Some("0".repeat(64));
    assert!(session.infer(encoded.get_ref(), &config, &task).is_err());
}
