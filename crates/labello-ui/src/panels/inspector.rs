impl LabelloApp {
    pub(crate) fn inspector_panel_toggle(&mut self, ui: &mut egui::Ui) {
        let (label, hover) = if self.work.inspector_panel_collapsed {
            ("Expand inspector panel", "Expand inspector panel")
        } else {
            ("Collapse inspector panel", "Collapse inspector panel")
        };
        let response = ui
            .add(egui::Button::new("").min_size(egui::vec2(44.0, 44.0)))
            .on_hover_text(hover);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
        });
        paint_side_panel_toggle_icon(
            ui,
            response.rect,
            self.work.inspector_panel_collapsed,
            true,
            ui.style().interact(&response).fg_stroke.color,
        );
        if response.clicked() {
            self.trigger_user_action(labello_domain::UserAction::ToggleInspectorPanel);
            ui.ctx()
                .request_discard("inspector panel visibility changed");
        }
    }

    pub(crate) fn right_panel(&mut self, ui: &mut egui::Ui, show_primary_actions: bool) {
        ui.heading(RichText::new("Inspector").color(theme::TEXT));
        if self.view == AppView::Review && !self.review_context_section(ui) {
            return;
        }
        if self.manual_migration_active() {
            // Migration commands live in the persistent workspace action bar so
            // collapsing this optional panel never hides the current action.
            self.manual_migration_actions(ui, false);
            if self.view == AppView::Review { self.review_corrections_panel(ui); }
            return;
        }
        let active_count = self
            .work
            .annotations
            .iter()
            .filter(|annotation| {
                !annotation.deleted && self.annotation_matches_selected_workflow(annotation)
            })
            .count();
        if self.view != AppView::Review {
            theme::compact_metric(ui, "Active annotations", active_count.to_string());
        }
        if self.view == AppView::Annotate {
            self.annotation_object_actions(ui);
        }
        if self.view == AppView::Annotate && self.work.tool == Tool::Keypoints {
            self.keypoint_actions(ui);
        }
        match self.view {
            AppView::Annotate => self.prelabel_panel(ui),
            AppView::Review => self.review_actions(ui, show_primary_actions),
            AppView::Setup | AppView::Admin | AppView::Stats => {}
        }
        if self.view == AppView::Review { self.review_corrections_panel(ui); }
        self.missing_object_panel(ui);
    }

    fn annotation_object_actions(&mut self, ui: &mut egui::Ui) {
        let objects = self
            .work.annotations
            .iter()
            .filter(|annotation| {
                !annotation.deleted && self.annotation_matches_selected_workflow(annotation)
            })
            .enumerate()
            .map(|(index, annotation)| {
                let class_name = self.class_name(&annotation.class_id);
                let geometry = match &annotation.geometry {
                    AnnotationGeometry::BoundingBox(bbox) => format!(
                        "Position: {:.0}% from left, {:.0}% from top\nSize: {:.0}% wide by {:.0}% high",
                        bbox.x * 100.0,
                        bbox.y * 100.0,
                        bbox.width * 100.0,
                        bbox.height * 100.0
                    ),
                    AnnotationGeometry::Skeleton(skeleton) => format!(
                        "Keypoints placed: {} of {}",
                        skeleton
                            .keypoints
                            .iter()
                            .filter(|keypoint| keypoint.point.is_some())
                            .count(),
                        skeleton.keypoints.len()
                    ),
                };
                (
                    annotation.annotation_id.clone(),
                    index + 1,
                    class_name,
                    geometry,
                )
            })
            .collect::<Vec<_>>();
        if objects.is_empty() {
            theme::empty_state(
                ui,
                "No objects yet",
                "Draw or accept an object to inspect it.",
                None,
            );
            return;
        }

        if self.work.selected_annotation.is_some()
            && !objects.iter().any(|(annotation_id, ..)| {
                Some(annotation_id) == self.work.selected_annotation.as_ref()
            })
        {
            self.work.selected_annotation = None;
        }

        ui.separator();
        ui.label(RichText::new("Objects").strong());
        for (annotation_id, number, class_name, geometry) in objects {
            let selected = self.work.selected_annotation.as_ref() == Some(&annotation_id);
            theme::selected_card_frame(selected).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let label = format!(
                    "Object {number} | {class_name}{}",
                    if selected { " | Selected" } else { "" }
                );
                if ui
                    .add_sized(
                        [ui.available_width(), 44.0],
                        egui::Button::selectable(selected, &label).truncate(),
                    )
                    .on_hover_text(format!(
                        "{label}\nPrevious: {} | Next: {}",
                        self.shortcut_text(
                            ui.ctx(),
                            labello_domain::UserAction::SelectPreviousObject,
                        ),
                        self.shortcut_text(ui.ctx(), labello_domain::UserAction::SelectNextObject,)
                    ))
                    .clicked()
                {
                    self.work.selected_annotation = Some(annotation_id.clone());
                }

                egui::CollapsingHeader::new(format!("Geometry details for Object {number}"))
                    .id_salt(annotation_id.as_str())
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&geometry)
                                .monospace()
                                .color(theme::TEXT_MUTED),
                        );
                    });
            });
        }
        if self.work.selected_annotation.is_some()
            && theme::danger_button(
                ui,
                true,
                egui::Button::new("Delete selected annotation").shortcut_text(crate::theme::button_shortcut(
                    self.shortcut_text(ui.ctx(), labello_domain::UserAction::DeleteAnnotation))),
            )
            .clicked()
        {
            self.delete_selected();
        }
    }

    fn keypoint_actions(&mut self, ui: &mut egui::Ui) {
        let spec = self.selected_task().and_then(|task| task.skeleton.clone());
        let next_keypoint = spec.as_ref().and_then(|skeleton| {
            skeleton
                .keypoints
                .get(self.work.skeleton_keypoint_index)
                .map(|keypoint| keypoint.name.clone())
        });
        if let Some(name) = next_keypoint {
            theme::compact_metric(
                ui,
                if self.work.active_skeleton.is_some() {
                    "Place keypoint"
                } else {
                    "Start skeleton"
                },
                name.as_str(),
            );
            if let Some(spec) = spec {
                let hidden_shortcut = self.shortcut_text(
                    ui.ctx(),
                    labello_domain::UserAction::ToggleKeypointHidden,
                );
                ui.add_enabled_ui(!self.loading.saving, |ui| {
                    if spec.allow_hidden {
                        keypoint_placement_mode(
                            ui,
                            &name,
                            &mut self.work.next_keypoint_hidden,
                            &hidden_shortcut,
                        );
                    }
                    ui.horizontal(|ui| {
                        if spec.allow_absent
                            && self.work.active_skeleton.is_some()
                            && spec
                                .keypoints
                                .get(self.work.skeleton_keypoint_index)
                                .is_some_and(|keypoint| !keypoint.required)
                            && ui
                                .add(
                                    egui::Button::new(format!(
                                        "Mark {name} as not present"
                                    ))
                                    .shortcut_text(crate::theme::button_shortcut(self.shortcut_text(
                                        ui.ctx(),
                                        labello_domain::UserAction::MarkKeypointAbsent,
                                    ))),
                                )
                                .on_hover_text(
                                    "Record this optional keypoint without a position.",
                                )
                                .clicked()
                        {
                            self.skip_keypoint();
                        }
                    });
                });
            }
        }
    }

    fn review_context_section(&self, ui: &mut egui::Ui) -> bool {
        let Some(context) = self.review_context() else {
            theme::inline_message(
                ui,
                theme::Intent::Info,
                if self.loading.image {
                    "Loading review target…"
                } else if self.work.assignment.is_none() {
                    "No active review assignment"
                } else {
                    "Review target unavailable"
                },
            );
            return false;
        };
        let summary = context.accessible_summary();
        let response = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new("Active review target").strong());
            for line in context.details() {
                ui.add(egui::Label::new(line).wrap());
            }
            if context.preview_unavailable {
                theme::inline_message(ui, theme::Intent::Warning, "Image preview unavailable. Review target context is retained.");
            }
        });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Active review context: {summary}"))
        });
        true
    }

    fn review_phase(&self) -> (&'static str, String, &'static str) {
        let total = self
            .work
            .annotations
            .iter()
            .filter(|annotation| {
                !annotation.deleted && self.annotation_matches_selected_workflow(annotation)
            })
            .count();
        if self.work.review_index < total {
            (
                "Object review",
                format!("{} of {total}", self.work.review_index + 1),
                "The active object is highlighted on the canvas.",
            )
        } else {
            (
                "Final check",
                "Full image".to_string(),
                "Check for missed objects before completing this review.",
            )
        }
    }

    fn review_actions(&mut self, ui: &mut egui::Ui, show_primary_actions: bool) {
        let (_, _, explanation) = self.review_phase();
        ui.label(explanation);
        if show_primary_actions {
            ui.horizontal_wrapped(|ui| self.review_decision_buttons(ui, false, false));
        }
    }

    pub(crate) fn review_decision_buttons(
        &mut self,
        ui: &mut egui::Ui,
        shortcut_only: bool,
        fill_width: bool,
    ) {
        let ready = self.work.assignment.is_some() && !self.loading.saving
            && !self.loading.image && self.work.pending_transition.is_none();
        let shortcut = self.shortcut_text(ui.ctx(), labello_domain::UserAction::NextImage);
        let label = if self.focused_review_changed() { "Submit correction" } else { "Approve" };
        let label = if shortcut_only { shortcut_button_label(&shortcut, label) } else { label.to_string() };
        let explanation = match (self.review_overview(), self.focused_review_changed()) {
            (false, false) => "Approve this item and continue",
            (false, true) => "Keep this correction and continue",
            (true, false) => "Submit approval for this image",
            (true, true) => "Submit corrections for a fresh review round",
        };
        let button_width = fill_width.then(|| ui.available_size_before_wrap().x.floor().max(44.0));
        let enabled = ready && !self.work.migration.busy
            && (self.review_can_approve() || self.review_can_reject());
        if workspace_action_button(ui, enabled, &label, WorkspaceActionIcon::Approve, button_width, theme::Intent::Accent)
            .on_hover_text(format!("{explanation} ({shortcut})"))
            .on_disabled_hover_text("Finish or reset the current annotation and review every item before submitting.")
            .clicked()
        {
            self.confirm_review_item();
        }
    }

    pub(crate) fn correction_actions(&mut self, ui: &mut egui::Ui, ready: bool) {
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ready, egui::Button::new("Reset item")).clicked() { self.reset_review_item(); }
            if self.review_overview() && ui.add_enabled(ready && self.review_editor_valid(), egui::Button::new("Back to overview")).clicked() { self.retain_review_editor(); }
        });
        ui.label("Edit the highlighted item directly on the canvas.");

        let skeleton_keypoints = self.work.correction_draft.as_ref().and_then(|draft| {
            let AnnotationGeometry::Skeleton(skeleton) = &draft.edited_geometry else {
                return None;
            };
            Some(
                skeleton
                    .keypoints
                    .iter()
                    .enumerate()
                    .map(|(index, keypoint)| (index, keypoint.name.clone(), keypoint.state.clone()))
                    .collect::<Vec<_>>(),
            )
        });
        ui.add_space(theme::SPACE_2);
        ui.label(RichText::new("Object").strong().color(theme::TEXT_MUTED));
        if let Some(keypoints) = skeleton_keypoints {
            ui.label("Edit only the highlighted skeleton on the canvas.");
            ui.add_space(theme::SPACE_2);
            ui.label(RichText::new("Keypoints").strong().color(theme::TEXT_MUTED));
            ui.label("Select and drag an existing keypoint:");
            for (index, name, state) in keypoints {
                let selected = self
                    .work
                    .correction_draft
                    .as_ref()
                    .is_some_and(|draft| draft.selected_keypoint == Some(index));
                if ui
                    .selectable_label(
                        selected,
                        format!("{name} ({})", keypoint_state_label(&state)),
                    )
                    .clicked()
                {
                    self.select_correction_keypoint(index);
                }
            }
            self.correction_keypoint_state(ui, ready);
        } else {
            ui.label("Drag inside the box to move it, or drag a handle to resize it.");
        }

        ui.add_space(theme::SPACE_2);
        ui.label(RichText::new("Reason").strong().color(theme::TEXT_MUTED));
        if let Some(draft) = self.work.correction_draft.as_mut() {
            let label = ui.label("Reason (optional)");
            ui.add_enabled_ui(ready, |ui| {
                theme::resizable_multiline_text_edit(
                    ui,
                    ui.make_persistent_id("correction-reason"),
                    &mut draft.reason,
                    2,
                    Some("What was corrected?"),
                )
                .labelled_by(label.id);
            });
        }

    }

    fn correction_keypoint_state(&mut self, ui: &mut egui::Ui, ready: bool) {
        let Some((index, current, has_point, required)) =
            self.work.correction_draft.as_ref().and_then(|draft| {
                let index = draft.selected_keypoint?;
                let AnnotationGeometry::Skeleton(skeleton) = &draft.edited_geometry else {
                    return None;
                };
                let keypoint = skeleton.keypoints.get(index)?;
                let required = self
                    .selected_task()
                    .and_then(|task| task.skeleton.as_ref())
                    .and_then(|spec| spec.keypoints.get(index))
                    .is_some_and(|spec| spec.required);
                Some((
                    index,
                    keypoint.state.clone(),
                    keypoint.point.is_some(),
                    required,
                ))
            })
        else {
            return;
        };
        let (allow_hidden, allow_absent) = self
            .selected_task()
            .and_then(|task| task.skeleton.as_ref())
            .map(|spec| (spec.allow_hidden, spec.allow_absent))
            .unwrap_or_default();
        ui.label(format!("Keypoint {} visibility", index + 1));
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    ready && has_point,
                    egui::Button::selectable(current == KeypointState::Visible, "Visible"),
                )
                .clicked()
            {
                self.set_correction_keypoint_state(KeypointState::Visible);
            }
            if ui
                .add_enabled(
                    ready && allow_hidden && has_point,
                    egui::Button::selectable(current == KeypointState::Hidden, "Hidden"),
                )
                .clicked()
            {
                self.set_correction_keypoint_state(KeypointState::Hidden);
            }
            if ui
                .add_enabled(
                    ready && allow_absent && !required,
                    egui::Button::selectable(current == KeypointState::Absent, "Absent"),
                )
                .clicked()
            {
                self.set_correction_keypoint_state(KeypointState::Absent);
            }
        });
    }



}

pub(crate) fn shortcut_button_label(shortcut: &str, fallback: &str) -> String {
    if shortcut.is_empty() {
        fallback.to_string()
    } else {
        shortcut.to_string()
    }
}
