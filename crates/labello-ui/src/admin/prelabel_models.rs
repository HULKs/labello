fn edit_model_location(
    ui: &mut egui::Ui,
    config: &mut PrelabelConfig,
    checks: &mut std::collections::BTreeMap<PrelabelConfigId, crate::prelabel_flow::ModelCheckUi>,
    actions: &mut Vec<crate::prelabel_flow::PrelabelAction>,
) {
    use crate::prelabel_flow::{ModelCheckUi, PrelabelAction};
    let check = checks
        .entry(config.config_id.clone())
        .or_insert_with(|| ModelCheckUi {
            location: config.model.location.clone(),
            ..Default::default()
        });
    let old_location = config.model.location.clone();
    ui.label("Model file");
    ui.horizontal(|ui| {
        let button_width = (ui
            .painter()
            .layout_no_wrap(
                "Check model".into(),
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().text_color(),
            )
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x)
            .ceil()
            .min(ui.available_width() * 0.45);
        let width = (ui.available_width() - button_width - ui.spacing().item_spacing.x).max(24.0);
        ui.add_sized(
            [width, theme::COMPACT_TEXT_FIELD_HEIGHT],
            egui::TextEdit::singleline(&mut config.model.location).hint_text("model.onnx"),
        )
        .on_hover_text("ONNX filename in the server's configured models directory.")
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Model file"));
        let busy = check.pending.is_some();
        if ui
            .add_enabled_ui(
                !busy && ModelSpec::validate_location(&config.model.location).is_ok(),
                |ui| {
                    ui.add_sized(
                        [button_width, ui.spacing().interact_size.y],
                        egui::Button::new(if busy { "Checking…" } else { "Check model" }).wrap(),
                    )
                },
            )
            .inner
            .on_hover_text("Check the file and discover its input, output tensors, and classes.")
            .clicked()
        {
            actions.push(PrelabelAction::InspectModel {
                config_id: config.config_id.clone(),
                location: config.model.location.clone(),
            });
        }
    });
    if config.model.location != old_location || check.location != config.model.location {
        *check = ModelCheckUi {
            location: config.model.location.clone(),
            ..Default::default()
        };
        invalidate_model_profile(config);
    }
    if check.pending.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Checking model…");
        });
    }
    if let Some(Err(error)) = &check.result {
        ui.colored_label(theme::DANGER, error);
    }
}

fn edit_model_profile(
    ui: &mut egui::Ui,
    config: &mut PrelabelConfig,
    labels: &[LabelClass],
    tasks: &[TaskDefinition],
    checks: &std::collections::BTreeMap<PrelabelConfigId, crate::prelabel_flow::ModelCheckUi>,
) {
    let inspection = checks
        .get(&config.config_id)
        .filter(|check| check.location == config.model.location)
        .and_then(|check| check.result.as_ref())
        .and_then(|result| result.as_ref().ok());
    let Some(inspection) = inspection else {
        ui.small("Check the model to select its output and map classes.");
        if let Some(spec) = &config.yolo {
            ui.small(format!(
                "Saved profile: {} × {} input, {} model classes",
                spec.input_size,
                spec.input_size,
                spec.model_class_count()
            ));
            for label in labels {
                let outputs = (0..spec.model_class_count())
                    .filter(|&id| spec.dataset_class(id) == Some(&label.class_id))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                if !outputs.is_empty() {
                    ui.small(format!(
                        "{} · {} → {}",
                        label.name,
                        label.class_id,
                        outputs.join(", ")
                    ));
                }
            }
        }
        return;
    };
    if let Some(problem) = &inspection.problem {
        invalidate_model_profile(config);
        ui.colored_label(theme::DANGER, problem);
        return;
    }
    let compatible = inspection
        .outputs
        .iter()
        .filter(|output| output.profile.is_some())
        .collect::<Vec<_>>();
    let mut selected = config
        .yolo
        .as_ref()
        .and_then(|spec| spec.output_name.clone())
        .filter(|name| compatible.iter().any(|output| &output.tensor.name == name));
    if selected.is_none() && compatible.len() == 1 {
        selected = Some(compatible[0].tensor.name.clone());
    }
    let label = ui.label("Output tensor");
    egui::ComboBox::from_id_salt((&config.config_id, "model_output"))
        .width(ui.available_width().min(520.0))
        .truncate()
        .selected_text(
            selected
                .as_ref()
                .and_then(|name| inspection.outputs.iter().find(|o| &o.tensor.name == name))
                .map_or_else(
                    || "Select an output".into(),
                    |output| model_tensor_label(&output.tensor),
                ),
        )
        .show_ui(ui, |ui| {
            for output in &inspection.outputs {
                ui.add_enabled_ui(output.profile.is_some(), |ui| {
                    ui.selectable_value(
                        &mut selected,
                        Some(output.tensor.name.clone()),
                        model_tensor_label(&output.tensor),
                    )
                    .on_hover_text(output.problem.as_deref().unwrap_or("Supported YOLO output"));
                });
            }
        })
        .response
        .labelled_by(label.id);
    if selected.is_none() {
        invalidate_model_profile(config);
    }
    if compatible.is_empty() {
        ui.colored_label(theme::DANGER, "No supported YOLO output was found.");
        for output in &inspection.outputs {
            if let Some(problem) = &output.problem {
                ui.label(format!("{}: {problem}", output.tensor.name));
            }
        }
        return;
    }
    let Some(output) = selected
        .as_ref()
        .and_then(|name| compatible.iter().find(|o| &o.tensor.name == name))
    else {
        return;
    };
    let profile = output.profile.as_ref().expect("compatible output");
    let spec = config.yolo.get_or_insert_with(Default::default);
    let changed_output = spec.output_name != selected
        || spec.model_digest.as_ref() != Some(&inspection.model_digest);
    spec.use_explicit_mapping(profile.class_count);
    spec.output_name = selected;
    spec.model_digest = Some(inspection.model_digest.clone());
    spec.input_size = inspection.input_size.expect("supported input");
    if changed_output && spec.keypoints.len() != profile.keypoint_count as usize {
        spec.keypoints = tasks
            .iter()
            .filter_map(|task| task.skeleton.as_ref())
            .find(|skeleton| skeleton.keypoints.len() == profile.keypoint_count as usize)
            .map(|skeleton| {
                skeleton
                    .keypoints
                    .iter()
                    .map(|keypoint| keypoint.name.clone())
                    .collect()
            })
            .unwrap_or_else(|| {
                (1..=profile.keypoint_count)
                    .map(|id| format!("keypoint_{id}"))
                    .collect()
            });
    }
    ui.small(format!(
        "{} · {} × {} input · {} classes{}",
        if profile.keypoint_count == 0 {
            "Detection"
        } else {
            "Pose"
        },
        spec.input_size,
        spec.input_size,
        profile.class_count,
        if profile.keypoint_count == 0 {
            String::new()
        } else {
            format!(" · {} keypoints", profile.keypoint_count)
        }
    ));
    ui.add_space(8.0);
    ui.label(RichText::new("Class mapping").strong());
    ui.small(
        "Choose the model output IDs for each dataset class. Unmapped outputs produce no hints.",
    );
    for label in labels {
        let label_response = ui.label(format!("{} · {}", label.name, label.class_id));
        let mapped = spec
            .class_mappings
            .iter()
            .filter(|m| m.class_id == label.class_id)
            .map(|m| model_class_label(profile, m.model_class_id))
            .collect::<Vec<_>>();
        egui::ComboBox::from_id_salt((&config.config_id, "class_mapping", &label.class_id))
            .width(ui.available_width().min(520.0))
            .height(260.0)
            .truncate()
            .selected_text(if mapped.is_empty() {
                "Not mapped".into()
            } else {
                mapped.join(", ")
            })
            .show_ui(ui, |ui| {
                for model_class_id in 0..profile.class_count {
                    let owner = spec
                        .class_mappings
                        .iter()
                        .find(|m| m.model_class_id == model_class_id)
                        .map(|m| m.class_id.clone());
                    let mut checked = owner.as_ref() == Some(&label.class_id);
                    let enabled = owner.as_ref().is_none_or(|id| id == &label.class_id);
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Checkbox::new(
                                &mut checked,
                                model_class_label(profile, model_class_id),
                            ),
                        )
                        .changed()
                    {
                        spec.class_mappings
                            .retain(|mapping| mapping.model_class_id != model_class_id);
                        if checked {
                            spec.class_mappings.push(labello_domain::YoloClassMapping {
                                model_class_id,
                                class_id: label.class_id.clone(),
                            });
                        }
                        spec.class_mappings
                            .sort_by_key(|mapping| mapping.model_class_id);
                    }
                }
            })
            .response
            .labelled_by(label_response.id);
    }
    let invalid = spec.class_mappings.iter().any(|mapping| {
        mapping.model_class_id >= profile.class_count
            || !labels
                .iter()
                .any(|label| label.class_id == mapping.class_id)
    });
    if invalid {
        ui.colored_label(
            theme::DANGER,
            "Some saved mappings no longer match this model or the dataset classes.",
        );
        if ui.button("Remove invalid mappings").clicked() {
            spec.class_mappings.retain(|mapping| {
                mapping.model_class_id < profile.class_count
                    && labels
                        .iter()
                        .any(|label| label.class_id == mapping.class_id)
            });
        }
    }
    if profile.keypoint_count > 0
        && let Some(keypoints) = prelabel_list_field(
            ui,
            (&config.config_id, "keypoints"),
            "Pose keypoint names in model order",
            spec.keypoints.join(", "),
        )
    {
        spec.keypoints = keypoints
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
}

fn invalidate_model_profile(config: &mut PrelabelConfig) {
    if let Some(spec) = &mut config.yolo {
        spec.use_explicit_mapping(spec.model_class_count() as u32);
        spec.output_name = None;
        spec.model_digest = None;
    }
}

fn model_tensor_label(tensor: &labello_domain::PrelabelTensor) -> String {
    format!(
        "{} · {} · [{}]",
        tensor.name,
        tensor.data_type,
        tensor
            .shape
            .iter()
            .map(|d| d.map_or_else(|| "?".into(), |d| d.to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn model_class_label(profile: &labello_domain::YoloOutputProfile, id: u32) -> String {
    profile
        .class_names
        .get(id as usize)
        .map_or_else(|| id.to_string(), |name| format!("{id} · {name}"))
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT as _, Queryable as _},
    };
    use labello_domain::*;
    use std::collections::BTreeMap;

    struct Editor {
        config: PrelabelConfig,
        labels: Vec<LabelClass>,
        checks: BTreeMap<PrelabelConfigId, crate::prelabel_flow::ModelCheckUi>,
        actions: Vec<crate::prelabel_flow::PrelabelAction>,
    }
    fn editor() -> Editor {
        let config: PrelabelConfig = serde_json::from_value(serde_json::json!({
            "configId":"model", "name":"Model", "model":{"modelId":"model","displayName":"Model","version":null,"location":"model.onnx"},
            "execution":{"mode":"server_side","command":[]}, "outputProcessing":{"confidenceThreshold":0.25,"suppressOverlapsIou":0.5},
            "availableToAnnotators":true, "yolo":{"inputSize":320,"classIds":["person","-"],"keypoints":[]}
        })).unwrap();
        let inspection = PrelabelModelInspection {
            model_digest: "a".repeat(64),
            inputs: vec![],
            input_size: Some(320),
            problem: None,
            outputs: vec![PrelabelModelOutput {
                tensor: PrelabelTensor {
                    name: "predictions".into(),
                    data_type: "float32".into(),
                    shape: vec![Some(1), Some(6), Some(2100)],
                },
                profile: Some(YoloOutputProfile {
                    class_count: 2,
                    class_names: vec!["person".into(), "ball".into()],
                    keypoint_count: 0,
                }),
                problem: None,
            }],
        };
        Editor {
            config,
            labels: vec![
                LabelClass {
                    class_id: "person".into(),
                    name: "Person".into(),
                    color: "#ffffff".into(),
                    description: None,
                },
                LabelClass {
                    class_id: "ball".into(),
                    name: "Ball".into(),
                    color: "#ffffff".into(),
                    description: None,
                },
            ],
            checks: BTreeMap::from([(
                "model".into(),
                crate::prelabel_flow::ModelCheckUi {
                    location: "model.onnx".into(),
                    pending: None,
                    result: Some(Ok(inspection)),
                },
            )]),
            actions: vec![],
        }
    }
    fn render(ui: &mut egui::Ui, state: &mut Editor) {
        if !theme::apply_fallback(ui.ctx()) {
            return;
        }
        ui.set_style(ui.ctx().global_style());
        egui::ScrollArea::vertical().show(ui, |ui| {
            edit_model_location(ui, &mut state.config, &mut state.checks, &mut state.actions);
            edit_model_profile(ui, &mut state.config, &state.labels, &[], &state.checks);
        });
    }

    #[test]
    fn model_editor_discovers_output_preserves_legacy_mapping_and_maps_explicit_ids() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 800.0))
            .build_ui_state(render, editor());
        harness.run();
        let spec = harness.state().config.yolo.as_ref().unwrap();
        assert_eq!(spec.class_count, Some(2));
        assert_eq!(spec.output_name.as_deref(), Some("predictions"));
        assert_eq!(spec.dataset_class(0).unwrap().as_str(), "person");
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Ball · ball")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::CheckBox, "1 · ball")
            .click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .config
                .yolo
                .as_ref()
                .unwrap()
                .dataset_class(1)
                .unwrap()
                .as_str(),
            "ball"
        );
        harness.key_press(egui::Key::Escape);
        harness.run();
        harness.state_mut().config.model.location = "other.onnx".into();
        harness.run();
        assert!(
            harness.state().checks[&PrelabelConfigId::from("model")]
                .result
                .is_none()
        );
        assert!(
            harness
                .state()
                .config
                .yolo
                .as_ref()
                .unwrap()
                .output_name
                .is_none()
        );
        assert!(harness.state().config.validate().is_err());
    }

    #[test]
    fn model_editor_requires_selection_for_ambiguous_outputs_and_explains_failed_checks() {
        let mut state = editor();
        let inspection = state
            .checks
            .get_mut(&"model".into())
            .unwrap()
            .result
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap();
        let mut second = inspection.outputs[0].clone();
        second.tensor.name = "another_output".into();
        inspection.outputs.push(second);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 800.0))
            .build_ui_state(render, state);
        harness.run();
        assert!(harness.state().config.validate().is_err());
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Output tensor")
            .click();
        harness.run();
        harness
            .get_by_label("another_output · float32 · [1, 6, 2100]")
            .click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .config
                .yolo
                .as_ref()
                .unwrap()
                .output_name
                .as_deref(),
            Some("another_output")
        );
        assert!(harness.state().config.validate().is_ok());
        harness
            .state_mut()
            .checks
            .get_mut(&"model".into())
            .unwrap()
            .result = Some(Err("Model file is unavailable".into()));
        harness.run();
        harness.get_by_label("Model file is unavailable");
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Check model")
            .click();
        harness.run();
        assert!(matches!(
            harness.state().actions.last(),
            Some(crate::prelabel_flow::PrelabelAction::InspectModel { .. })
        ));
        harness
            .state_mut()
            .checks
            .get_mut(&"model".into())
            .unwrap()
            .pending = Some(7);
        harness.run_steps(2);
        assert!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, "Checking…")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn model_editor_check_and_selectors_stay_within_narrow_viewports() {
        for width in [320.0, 390.0, 600.0, 1288.0, 1440.0] {
            let mut state = editor();
            state.labels[0].name =
                "A long dataset label that needs to wrap across several lines".repeat(3);
            state.config.model.location = "a".repeat(120) + ".onnx";
            state.checks.get_mut(&"model".into()).unwrap().location =
                state.config.model.location.clone();
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, 800.0))
                .build_ui_state(render, state);
            harness.run();
            for node in harness
                .query_all_by_role(egui::accesskit::Role::ComboBox)
                .chain(
                    harness
                        .query_all_by_role_and_label(egui::accesskit::Role::Button, "Check model"),
                )
            {
                let bounds = node.rect();
                assert!(
                    bounds.left() >= 0.0 && bounds.right() <= width,
                    "{width}: {bounds:?}"
                );
            }
        }
    }
}
