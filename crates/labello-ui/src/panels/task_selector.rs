use labello_domain::WorkflowVariant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowActivity { Boxes, Migration, Skeleton }

impl WorkflowActivity {
    fn label(self, review: bool) -> &'static str {
        match self {
            Self::Boxes if review => crate::glossary::BOUNDING_BOX_REVIEW,
            Self::Skeleton | Self::Migration if review => crate::glossary::SKELETON_REVIEW,
            Self::Boxes => crate::glossary::BOUNDING_BOX_ANNOTATION,
            Self::Migration => crate::glossary::MIGRATION,
            Self::Skeleton => crate::glossary::SKELETON_ANNOTATION,
        }
    }
    fn short_label(self) -> &'static str {
        match self { Self::Boxes => crate::glossary::BOXES, Self::Migration => crate::glossary::MIGRATE, Self::Skeleton => crate::glossary::SKELETON }
    }
}

impl LabelloApp {
    const WORKFLOW_ICON_SIZE: f32 = 28.0;

    pub(crate) fn workflow_panel_width(&self, _ctx: &egui::Context) -> f32 { 340.0 }

    #[cfg(test)]
    pub(crate) fn workflow_entry_label(&self, workflow: &crate::app::WorkflowChoice, activity: Option<WorkflowActivity>) -> String {
        let title = if self.workflow_is_split(workflow) {
            match self.work.workflow.variant { WorkflowVariant::Objects => crate::glossary::OBJECTS, WorkflowVariant::Overview => crate::glossary::OVERVIEW }
        } else if self.view == AppView::Review { crate::glossary::REVIEW } else { crate::glossary::ANNOTATE };
        format!("{} · {title}", self.workflow_entry_identity(workflow, activity))
    }

    fn workflow_entry_identity(&self, workflow: &crate::app::WorkflowChoice, activity: Option<WorkflowActivity>) -> String {
        let activity = activity.unwrap_or_else(|| self.workflow_primary_activity(workflow));
        format!("{} · {}", self.workflow_type_label(workflow, activity), workflow.label())
    }

    fn workflow_type_label(&self, workflow: &crate::app::WorkflowChoice, activity: WorkflowActivity) -> String {
        let class = self.work.tasks.iter().find(|task| task.task_id == workflow.task_id)
            .and_then(|task| task.class_ids.first()).map(|id| self.class_name(id)).unwrap_or_default();
        format!("{class}: {}", activity.label(self.view == AppView::Review))
    }

    fn workflow_primary_activity(&self, workflow: &crate::app::WorkflowChoice) -> WorkflowActivity {
        if workflow.annotation_type == AnnotationType::BoundingBox { WorkflowActivity::Boxes }
        else if self.view == AppView::Review { WorkflowActivity::Skeleton }
        else if self.work.tasks.iter().find(|task| task.task_id == workflow.task_id).is_some_and(|task| task.manual_box_guide_migration.is_some()) { WorkflowActivity::Migration }
        else { WorkflowActivity::Skeleton }
    }

    fn workflow_is_split(&self, workflow: &crate::app::WorkflowChoice) -> bool {
        self.workflow_variant_availability(&workflow.task_id, WorkflowVariant::Objects).map(|entry| entry.split)
            .unwrap_or_else(|| self.work.tasks.iter().find(|task| task.task_id == workflow.task_id)
                .is_some_and(|task| task.manual_box_guide_migration.is_some() || !task.prelabel_config_ids.is_empty()))
    }

    fn workflow_pass_block(&self, workflow: &crate::app::WorkflowChoice, variant: WorkflowVariant) -> Option<WorkflowMarkerReason> {
        self.workflow_interaction_block().or_else(|| {
            // The current lease stays usable even when no additional items are available.
            let current = self.workflow_context().is_some() && self.work.assignment.as_ref().is_some_and(|a| a.task_id == workflow.task_id && a.status == labello_domain::AssignmentStatus::Active)
                && self.work.workflow.variant == variant;
            if current { return None; }
            if let Some(entry) = self.workflow_variant_availability(&workflow.task_id, variant) {
                return (!entry.available).then_some(WorkflowMarkerReason::Unavailable(entry.reason.unwrap_or(labello_domain::WorkflowUnavailableReason::Unavailable)));
            }
            self.workflow_marker_reason(&workflow.task_id)
        })
    }

    fn class_workflow_groups(&mut self, ui: &mut egui::Ui, workflows: &[crate::app::WorkflowChoice]) {
        let mut groups: Vec<(labello_domain::ClassId, Vec<crate::app::WorkflowChoice>)> = Vec::new();
        for workflow in workflows {
            let Some(class_id) = self.work.tasks.iter().find(|task| task.task_id == workflow.task_id).and_then(|task| task.class_ids.first()).cloned() else { continue; };
            if let Some((_, entries)) = groups.iter_mut().find(|(id, _)| id == &class_id) { entries.push(workflow.clone()); }
            else { groups.push((class_id, vec![workflow.clone()])); }
        }
        ui.spacing_mut().item_spacing.y = 8.0;
        for (class_id, entries) in groups {
            ui.push_id(&class_id, |ui| {
                let frame = theme::inset_frame();
                let width = (ui.available_width() - frame.total_margin().sum().x).max(1.0);
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    let heading = ui.add_sized([width, 0.0], egui::Label::new(RichText::new(self.class_name(&class_id)).heading()).halign(egui::Align::Center).wrap());
                    ui.ctx().accesskit_node_builder(heading.id, |node| { node.set_role(egui::accesskit::Role::Heading); node.set_label(self.class_name(&class_id)); });
                    let activities: Vec<_> = [WorkflowActivity::Boxes, WorkflowActivity::Migration, WorkflowActivity::Skeleton].into_iter().filter_map(|activity| {
                        let matching: Vec<_> = entries.iter().filter(|entry| self.workflow_primary_activity(entry) == activity).collect();
                        (!matching.is_empty()).then_some((activity, matching))
                    }).collect();
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    // Reflow complete type groups before shrinking labels or targets.
                    let columns = ((width + 4.0) / 84.0).floor().max(1.0) as usize;
                    for row in activities.chunks(columns) {
                        ui.columns(row.len(), |columns| {
                            let label_height = columns.iter().zip(row).map(|(column, (activity, _))| {
                                column.painter().layout(activity.short_label().into(), egui::TextStyle::Button.resolve(column.style()), theme::TEXT, column.available_width()).size().y
                            }).fold(0.0, f32::max);
                            for (column, (activity, choices)) in columns.iter_mut().zip(row) {
                                column.push_id(activity.short_label(), |ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), Self::WORKFLOW_ICON_SIZE), egui::Sense::hover());
                                    let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(Self::WORKFLOW_ICON_SIZE, Self::WORKFLOW_ICON_SIZE));
                                    workflow_type_icon(ui, ui.id().with("type"), icon, &choices[0].annotation_type);
                                    if *activity == WorkflowActivity::Migration { paint_migration_type_badge(ui, icon); }
                                    ui.add_sized([ui.available_width(), label_height], egui::Label::new(activity.short_label()).halign(egui::Align::Center).wrap());
                                    let split = choices.iter().any(|choice| self.workflow_is_split(choice));
                                    if split {
                                        self.workflow_pass_button(ui, choices, *activity, WorkflowVariant::Objects, crate::glossary::OBJECTS, false);
                                        self.workflow_pass_button(ui, choices, *activity, WorkflowVariant::Overview, crate::glossary::OVERVIEW, false);
                                    } else {
                                        self.workflow_pass_button(ui, choices, *activity, WorkflowVariant::Overview, if self.view == AppView::Review { crate::glossary::REVIEW } else { crate::glossary::ANNOTATE }, false);
                                    }
                                });
                            }
                        });
                    }
                });
            });
        }
    }

    fn workflow_pass_button(&mut self, ui: &mut egui::Ui, choices: &[&crate::app::WorkflowChoice], activity: WorkflowActivity, variant: WorkflowVariant, title: &str, chooser: bool) {
        ui.push_id((variant == WorkflowVariant::Objects, chooser), |ui| {
            ui.spacing_mut().button_padding = egui::vec2(4.0, 4.0);
            let multiple = choices.len() > 1;
            let current = choices.iter().find(|choice| self.work.selected_task_id.as_ref() == Some(&choice.task_id));
            let workflow = current.copied().unwrap_or(choices[0]);
            let selected = current.is_some() && self.work.workflow.variant == variant;
            let block = if multiple {
                self.workflow_interaction_block()
            } else { self.workflow_pass_block(workflow, variant) };
            let boost = choices.iter().find_map(|choice| self.workflow_boost_window(&choice.task_id));
            let pass_label = if chooser { workflow.label() } else { title.to_owned() };
            let label = if multiple {
                format!("{} · {title} · Choose workflow", self.workflow_type_label(workflow, activity))
            } else { format!("{} · {title}", self.workflow_entry_identity(workflow, Some(activity))) };
            let width = (ui.available_width() - 8.0).max(1.0);
            let galley = ui.painter().layout(pass_label, egui::TextStyle::Button.resolve(ui.style()), theme::TEXT, width);
            let height = (galley.size().y + 18.0 + 12.0).max(44.0);
            let response = ui.add_enabled(block.is_none() || block == Some(WorkflowMarkerReason::CheckFailed), egui::Button::new("")
                .selected(selected).corner_radius(theme::SURFACE_RADIUS).min_size(egui::vec2(ui.available_width(), height)));
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, response.enabled(), selected, &label));
            let description = [block.map(|reason| reason.label().to_owned()), boost.map(|_| crate::glossary::BOOSTED_WORKFLOW.to_owned())].into_iter().flatten().collect::<Vec<_>>().join(". ");
            if !description.is_empty() { ui.ctx().accesskit_node_builder(response.id, |node| node.set_description(description.as_str())); }
            let color = ui.style().interact(&response).fg_stroke.color;
            let color = if response.enabled() { color } else { ui.visuals().disable(color) };
            let center = response.rect.center();
            ui.painter().galley(egui::pos2(center.x - galley.size().x / 2.0, response.rect.top() + 5.0), galley.clone(), color);
            let count = usize::from(selected) + usize::from(block.is_some()) + usize::from(multiple);
            let cue_width = if count == 0 { 0.0 } else { (if selected { 8.0 } else { 0.0 }) + (if block.is_some() { 18.0 } else { 0.0 }) + (if multiple { 8.0 } else { 0.0 }) + (count - 1) as f32 * 4.0 };
            let mut x = center.x - cue_width / 2.0;
            let y = response.rect.top() + 5.0 + galley.size().y + 2.0 + 9.0;
            if selected { ui.painter().circle_filled(egui::pos2(x + 4.0, y), 4.0, color); x += 12.0; }
            if block.is_some() { paint_workflow_marker(ui, egui::Rect::from_center_size(egui::pos2(x + 9.0, y), egui::vec2(18.0, 18.0)), false, block, color); x += 22.0; }
            if multiple {
                let stroke = egui::Stroke::new(1.5, color);
                ui.painter().line_segment([egui::pos2(x + 1.0, y - 2.0), egui::pos2(x + 4.0, y + 1.0)], stroke);
                ui.painter().line_segment([egui::pos2(x + 4.0, y + 1.0), egui::pos2(x + 7.0, y - 2.0)], stroke);
            }
            paint_workflow_boost(ui, &response, boost, None);
            let hover = if description.is_empty() { label.clone() } else { format!("{label}\n{description}") };
            let response = response.on_hover_text(&hover).on_disabled_hover_text(&hover);
            if response.gained_focus() { response.scroll_to_me(Some(egui::Align::Center)); }
            if multiple {
                let popup = egui::Popup::menu(&response).width((ui.ctx().content_rect().width() - 48.0).clamp(160.0, 360.0));
                let was_open = popup.is_open();
                if response.layer_id.order == egui::Order::Foreground {
                    ui.ctx().set_sublayer(response.layer_id, egui::LayerId::new(egui::Order::Foreground, popup.get_id()));
                }
                popup.show(|ui| {
                    ui.set_max_width((ui.ctx().content_rect().width() - 48.0).clamp(160.0, 360.0));
                    ui.label(RichText::new(self.workflow_type_label(workflow, activity)).strong());
                    egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx())).max_height((ui.ctx().content_rect().height() - 96.0).max(44.0)).show(ui, |ui| {
                        for choice in choices {
                            ui.push_id(&choice.task_id, |ui| self.workflow_pass_button(ui, &[*choice], activity, variant, title, true));
                        }
                    });
                });
                if was_open && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) { response.request_focus(); }
            } else if response.clicked() {
                self.request_transition(PendingTransition::WorkflowVariant(workflow.task_id.clone(), variant));
                if chooser { ui.close(); }
            }
        });
    }
    pub(crate) fn workflow_panel_toggle(&mut self, ui: &mut egui::Ui) {
        let (label, hover) = if self.work.workflow_panel_collapsed {
            ("Expand workflow panel", "Expand workflow panel")
        } else {
            ("Collapse workflow panel", "Collapse workflow panel")
        };
        let response = ui
            .add(egui::Button::new("").min_size(egui::vec2(44.0, 44.0)))
            .on_hover_text(hover);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        paint_side_panel_toggle_icon(
            ui,
            response.rect,
            self.work.workflow_panel_collapsed,
            false,
            ui.style().interact(&response).fg_stroke.color,
        );
        if response.clicked() {
            self.trigger_user_action(labello_domain::UserAction::ToggleWorkflowPanel);
            ui.ctx()
                .request_discard("workflow panel visibility changed");
        }
    }

    pub(crate) fn task_panel(&mut self, ui: &mut egui::Ui) {
        if self.workspace_bars_loading() {
            let opacity = ui.opacity();
            ui.disable();
            ui.set_opacity(opacity);
        }
        let workflows = self.workflow_choices();
        if workflows.is_empty() {
            theme::inline_message(
                ui,
                theme::Intent::Warning,
                "No enabled one-class workflows configured.",
            );
        }
        if self.work.availability.error.is_some() && ui.button("Retry availability").clicked() {
            self.work.availability.last_attempt = None;
            self.request_assignment_availability();
        }
        self.class_workflow_groups(ui, &workflows);
    }

    fn workflow_boost_window(&self, task_id: &labello_domain::TaskId) -> Option<i64> {
        if self.view != AppView::Annotate || self.datasets.stats_error.is_some() { return None; }
        self.datasets.stats.scoring_focus.as_ref()
            .filter(|focus| focus.task_id.as_ref() == Some(task_id) && focus.contains(labello_domain::now()))
            .map(|focus| focus.starts_at.timestamp_millis())
    }

}

fn paint_migration_type_badge(ui: &egui::Ui, rect: egui::Rect) {
    let center = rect.right_bottom() - egui::vec2(2.0, 2.0);
    ui.painter().circle_filled(center, 6.0, theme::SURFACE);
    let stroke = egui::Stroke::new(1.5, theme::TEXT);
    for (a, b) in [((-3.0, 0.0), (3.0, 0.0)), ((1.0, -2.0), (3.0, 0.0)), ((1.0, 2.0), (3.0, 0.0))] {
        ui.painter().line_segment([center + egui::vec2(a.0, a.1), center + egui::vec2(b.0, b.1)], stroke);
    }
}

pub(crate) fn paint_side_panel_toggle_icon(
    ui: &egui::Ui,
    rect: egui::Rect,
    expanding: bool,
    panel_on_right: bool,
    color: egui::Color32,
) {
    let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(25.0, 16.0));
    let panel_at_left = expanding != panel_on_right;
    let (panel, chevron_x) = if panel_at_left {
        (
            egui::Rect::from_min_size(icon.min, egui::vec2(16.0, icon.height())),
            icon.right() - 3.0,
        )
    } else {
        (
            egui::Rect::from_min_size(
                egui::pos2(icon.left() + 9.0, icon.top()),
                egui::vec2(16.0, icon.height()),
            ),
            icon.left() + 3.0,
        )
    };
    let painter = ui.painter();
    painter.rect_stroke(
        panel,
        egui::CornerRadius::same(2),
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );
    let panel_fill = if panel_on_right {
        egui::Rect::from_min_max(
            egui::pos2(panel.right() - 6.0, panel.top() + 2.5),
            panel.max - egui::vec2(2.5, 2.5),
        )
    } else {
        egui::Rect::from_min_max(
            panel.min + egui::vec2(2.5, 2.5),
            egui::pos2(panel.left() + 6.0, panel.bottom() - 2.5),
        )
    };
    painter.rect_filled(panel_fill, egui::CornerRadius::same(1), color);
    let direction = if panel_at_left { 1.0 } else { -1.0 };
    for y in [-4.0, 4.0] {
        painter.line_segment(
            [
                egui::pos2(chevron_x - direction * 3.0, icon.center().y + y),
                egui::pos2(chevron_x, icon.center().y),
            ],
            egui::Stroke::new(1.5, color),
        );
    }
}
