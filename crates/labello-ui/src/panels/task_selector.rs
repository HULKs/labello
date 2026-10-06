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
    const WORKFLOW_ICON_SIZE: f32 = 24.0;
    const WORKFLOW_PASS_GAP: f32 = 6.0;
    const WORKFLOW_PASS_WIDTH: f32 = 64.0;
    const WORKFLOW_PASS_INSET: f32 = 4.0;

    /// Fits the widest unwrapped class, activity or task name between a compact minimum and 360 points.
    pub(crate) fn workflow_panel_width(&self, ctx: &egui::Context) -> f32 {
        let workflows = self.workflow_choices();
        if workflows.is_empty() { return 360.0; }
        let measure = |text: String, size: f32| ctx.fonts_mut(|fonts| fonts.layout_no_wrap(text, egui::FontId::proportional(size), theme::TEXT).size().x);
        let class_of = |workflow: &crate::app::WorkflowChoice| self.work.tasks.iter().find(|task| task.task_id == workflow.task_id).and_then(|task| task.class_ids.first()).cloned();
        let cells = 2.0 * Self::WORKFLOW_PASS_WIDTH + Self::WORKFLOW_PASS_GAP + Self::WORKFLOW_PASS_INSET;
        let content = workflows.iter().map(|workflow| {
            let activity = self.workflow_primary_activity(workflow);
            let shared = workflows.iter().filter(|other| class_of(other) == class_of(workflow) && self.workflow_primary_activity(other) == activity).count() > 1;
            let label = if shared { measure(workflow.label(), 13.0) } else { measure(activity.short_label().into(), 14.0) };
            let heading = class_of(workflow).map_or(0.0, |class| measure(self.class_name(&class), 15.0) + 24.0);
            (Self::WORKFLOW_ICON_SIZE + 16.0 + label + cells).max(heading)
        }).fold(0.0, f32::max);
        // Side frame margins plus room for the floating scroll bar.
        (content + 2.0 * theme::SPACE_4 + theme::SPACE_2).clamp(240.0, 360.0).ceil()
    }

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

    fn workflow_pass_columns(row: egui::Rect) -> [egui::Rect; 2] {
        let width = Self::WORKFLOW_PASS_WIDTH;
        let overview = egui::Rect::from_min_max(egui::pos2(row.right() - Self::WORKFLOW_PASS_INSET - width, row.top()), egui::pos2(row.right() - Self::WORKFLOW_PASS_INSET, row.bottom()));
        [overview.translate(egui::vec2(-(width + Self::WORKFLOW_PASS_GAP), 0.0)), overview]
    }

    /// Names the pass columns once; it stays outside the scroll area so cells remain identifiable.
    pub(crate) fn workflow_pass_header(&self, ui: &mut egui::Ui) {
        if !self.workflow_choices().iter().any(|workflow| self.workflow_is_split(workflow)) { return; }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
        for (column, title) in Self::workflow_pass_columns(rect).into_iter().zip([crate::glossary::OBJECTS, crate::glossary::OVERVIEW]) {
            ui.painter().text(column.center(), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(12.0), theme::TEXT_MUTED);
        }
    }

    fn class_workflow_groups(&mut self, ui: &mut egui::Ui, workflows: &[crate::app::WorkflowChoice]) {
        let mut groups: Vec<(labello_domain::ClassId, Vec<crate::app::WorkflowChoice>)> = Vec::new();
        for workflow in workflows {
            let Some(class_id) = self.work.tasks.iter().find(|task| task.task_id == workflow.task_id).and_then(|task| task.class_ids.first()).cloned() else { continue; };
            if let Some((_, entries)) = groups.iter_mut().find(|(id, _)| id == &class_id) { entries.push(workflow.clone()); }
            else { groups.push((class_id, vec![workflow.clone()])); }
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        for (index, (class_id, entries)) in groups.into_iter().enumerate() {
            ui.push_id(&class_id, |ui| {
                if index > 0 { ui.add_space(8.0); }
                let name = self.class_name(&class_id);
                let heading = ui.add(egui::Label::new(RichText::new(&name).size(15.0).strong()).wrap());
                ui.ctx().accesskit_node_builder(heading.id, |node| { node.set_role(egui::accesskit::Role::Heading); node.set_label(name.as_str()); });
                let rule = heading.rect.right() + 8.0;
                if rule + 16.0 < ui.max_rect().right() {
                    ui.painter().hline(rule..=ui.max_rect().right(), heading.rect.center().y, egui::Stroke::new(1.0, theme::BORDER_STRONG));
                }
                for activity in [WorkflowActivity::Boxes, WorkflowActivity::Migration, WorkflowActivity::Skeleton] {
                    let choices: Vec<_> = entries.iter().filter(|entry| self.workflow_primary_activity(entry) == activity).collect();
                    if choices.is_empty() { continue; }
                    ui.push_id(activity.short_label(), |ui| {
                        if let [choice] = choices[..] {
                            self.workflow_row(ui, choice, activity, true);
                        } else {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
                            self.workflow_activity_label(ui, rect, &choices[0].annotation_type, activity);
                            for choice in choices {
                                ui.push_id(&choice.task_id, |ui| self.workflow_row(ui, choice, activity, false));
                            }
                        }
                    });
                }
            });
        }
    }

    fn workflow_activity_label(&self, ui: &mut egui::Ui, row: egui::Rect, annotation_type: &AnnotationType, activity: WorkflowActivity) {
        let size = Self::WORKFLOW_ICON_SIZE;
        let icon = egui::Rect::from_min_size(egui::pos2(row.left(), row.center().y - size / 2.0), egui::vec2(size, size));
        workflow_type_icon(ui, ui.id().with("type"), icon, annotation_type);
        if activity == WorkflowActivity::Migration { paint_migration_type_badge(ui, icon); }
        let galley = ui.painter().layout_no_wrap(activity.short_label().into(), egui::FontId::proportional(14.0), theme::TEXT);
        ui.painter().galley(egui::pos2(icon.right() + 8.0, row.center().y - galley.size().y / 2.0), galley, theme::TEXT);
    }

    /// One workflow: its activity (or, beneath a shared activity label, its task name) and pass cells.
    fn workflow_row(&mut self, ui: &mut egui::Ui, workflow: &crate::app::WorkflowChoice, activity: WorkflowActivity, primary: bool) {
        let width = ui.available_width();
        let lead = Self::WORKFLOW_ICON_SIZE + 8.0;
        let probe = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, 44.0));
        let label_width = (Self::workflow_pass_columns(probe)[0].left() - probe.left() - lead - 8.0).max(24.0);
        let galley = (!primary).then(|| ui.painter().layout(workflow.label(), egui::FontId::proportional(13.0), theme::TEXT, label_width));
        let row = egui::Rect::from_min_size(probe.min, egui::vec2(width, galley.as_ref().map_or(44.0, |galley| (galley.size().y + 12.0).max(44.0))));
        let [objects, overview] = Self::workflow_pass_columns(row);
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            ui.expand_to_include_rect(row);
            match galley {
                None => self.workflow_activity_label(ui, row, &workflow.annotation_type, activity),
                Some(galley) => ui.painter().galley(egui::pos2(row.left() + lead, row.center().y - galley.size().y / 2.0), galley, theme::TEXT),
            }
            if self.workflow_is_split(workflow) {
                self.workflow_pass_cell(ui, objects, workflow, activity, WorkflowVariant::Objects, crate::glossary::OBJECTS);
                self.workflow_pass_cell(ui, overview, workflow, activity, WorkflowVariant::Overview, crate::glossary::OVERVIEW);
            } else {
                // Unsplit workflows annotate or review the complete image, which is the Overview queue.
                let title = if self.view == AppView::Review { crate::glossary::REVIEW } else { crate::glossary::ANNOTATE };
                self.workflow_pass_cell(ui, overview, workflow, activity, WorkflowVariant::Overview, title);
            }
        });
    }

    fn workflow_pass_cell(&mut self, ui: &mut egui::Ui, rect: egui::Rect, workflow: &crate::app::WorkflowChoice, activity: WorkflowActivity, variant: WorkflowVariant, title: &str) {
        let selected = self.work.selected_task_id.as_ref() == Some(&workflow.task_id) && self.work.workflow.variant == variant;
        let block = self.workflow_pass_block(workflow, variant);
        let boost = self.workflow_boost_window(&workflow.task_id);
        let label = format!("{} · {title}", self.workflow_entry_identity(workflow, Some(activity)));
        let enabled = block.is_none() || block == Some(WorkflowMarkerReason::CheckFailed);
        let response = ui.scope_builder(egui::UiBuilder::new().max_rect(rect).id_salt(variant == WorkflowVariant::Objects), |ui| {
            ui.add_enabled(enabled, egui::Button::new("").selected(selected).frame_when_inactive(selected)
                .corner_radius(theme::SURFACE_RADIUS).min_size(rect.size()))
        }).inner;
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, response.enabled(), selected, &label));
        let description = [block.map(|reason| reason.label().to_owned()), boost.map(|_| crate::glossary::BOOSTED_WORKFLOW.to_owned())].into_iter().flatten().collect::<Vec<_>>().join(". ");
        if !description.is_empty() { ui.ctx().accesskit_node_builder(response.id, |node| node.set_description(description.as_str())); }
        let color = ui.style().interact(&response).fg_stroke.color;
        let color = if response.enabled() { color } else { ui.visuals().disable(color) };
        let center = response.rect.center();
        let cue_width = if selected && block.is_some() { 30.0 } else if block.is_some() { 18.0 } else { 8.0 };
        let x = center.x - cue_width / 2.0;
        if selected { ui.painter().circle_filled(egui::pos2(x + 4.0, center.y), 4.0, color); }
        if block.is_some() {
            let left = if selected { x + 12.0 } else { x };
            paint_workflow_marker(ui, egui::Rect::from_min_size(egui::pos2(left, center.y - 9.0), egui::vec2(18.0, 18.0)), false, block, color);
        }
        if !selected && block.is_none() { ui.painter().circle_stroke(center, 4.0, egui::Stroke::new(1.5, theme::TEXT_MUTED)); }
        paint_workflow_boost(ui, &response, boost);
        let hover = if description.is_empty() { label.clone() } else { format!("{label}\n{description}") };
        let response = response.on_hover_text(&hover).on_disabled_hover_text(&hover);
        if response.gained_focus() { response.scroll_to_me(Some(egui::Align::Center)); }
        if response.clicked() {
            self.request_transition(PendingTransition::WorkflowVariant(workflow.task_id.clone(), variant));
        }
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
