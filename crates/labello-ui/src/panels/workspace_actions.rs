impl LabelloApp {
    fn review_bottom_actions(&mut self, ui: &mut egui::Ui) {
        let ready = self.work.assignment.is_some()
            && !self.loading.saving && !self.loading.image && !self.work.migration.busy
            && self.work.pending_transition.is_none();
        let compact = LayoutMode::for_width(ui.ctx().content_rect().width()) != LayoutMode::Wide;
        let secondary = |app: &mut Self, ui: &mut egui::Ui| {
            let removable = app.migration_review_removal().is_some();
            let count = (if app.work.previous_assignment.is_some() { 3.0 } else { 2.0 })
                + if removable { 1.0 } else { 0.0 };
            let width = compact.then(|| ((ui.available_width() - (count - 1.0) * ui.spacing().item_spacing.x) / count).floor().max(44.0));
            app.previous_review_action(ui, width);
            app.discard_review_action(ui, width);
            if removable && workspace_action_button(ui, ready && app.work.review_corrections.submission.is_none(), "Remove item", WorkspaceActionIcon::Remove, width, theme::Intent::Error).clicked() {
                app.remove_migration_review_item();
            }
            if workspace_action_button(ui, ready, "Skip", WorkspaceActionIcon::Skip, width, theme::Intent::Neutral).clicked() {
                app.trigger_user_action(labello_domain::UserAction::SkipAssignment);
            }
        };
        if compact {
            ui.vertical(|ui| {
                ui.horizontal(|ui| self.review_decision_buttons(ui, false, true));
                ui.horizontal(|ui| secondary(self, ui));
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                self.review_decision_buttons(ui, false, false);
                ui.separator();
                secondary(self, ui);
            });
        }
    }

    fn previous_review_action(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        if self.view == AppView::Review && self.work.previous_assignment.is_some()
            && workspace_action_button(ui, !self.loading.saving && !self.loading.image && !self.work.migration.busy && self.work.pending_transition.is_none(),
                "Previous", WorkspaceActionIcon::Previous, width, theme::Intent::Neutral).on_hover_text("Return to the immediately previous skipped or completed review.").clicked()
        {
            self.trigger_user_action(labello_domain::UserAction::PreviousImage);
        }
    }

    fn discard_review_action(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        let ready = self.has_review_corrections() && self.work.review_corrections.submission.is_none() && !self.loading.saving && !self.loading.image && !self.work.migration.busy && self.work.pending_transition.is_none();
        let label = if LayoutMode::for_width(ui.ctx().content_rect().width()) == LayoutMode::Wide { "Discard corrections" } else { "Discard" };
        if workspace_action_button(ui, ready, label, WorkspaceActionIcon::Discard, width, theme::Intent::Neutral).clicked() { self.discard_all_review_corrections(); }
    }

    pub(crate) fn workspace_actions(&mut self, ui: &mut egui::Ui, _layout: LayoutMode) {
        if !self.work_view() {
            return;
        }
        if self.view == AppView::Review {
            self.review_bottom_actions(ui);
            return;
        }
        if self.manual_migration_active() {
            self.migration_workspace_actions(ui, false);
            return;
        }
        let ready = (self.work.assignment.is_some() || self.runtime.api.is_none())
            && !self.loading.saving
            && !self.loading.image
            && self.work.pending_transition.is_none();
        let primary_width = ((ui.available_size_before_wrap().x - 44.0 - 3.0 * ui.spacing().item_spacing.x) / 3.0).floor().max(44.0);
        if self.view == AppView::Annotate {
            let show_previous = self.work.previous_assignment.is_some()
                && !matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry);
            if show_previous {
                if workspace_toolbar_button(ui, self.runtime.api.is_some() && ready, "Previous", WorkspaceActionIcon::Previous, Some(primary_width), theme::Intent::Neutral)
                    .on_hover_text("Return to the last skipped or submitted assignment.")
                    .clicked()
                {
                    self.trigger_user_action(labello_domain::UserAction::PreviousImage);
                }
            } else if workspace_toolbar_button(ui, ready && matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry), "Save", WorkspaceActionIcon::Save, Some(primary_width), theme::Intent::Neutral)
                .on_hover_text("Save edits and keep this assignment active.")
                .clicked()
            {
                self.trigger_user_action(labello_domain::UserAction::SaveAnnotations);
            }
            if workspace_toolbar_button(ui, ready, "Submit & next", WorkspaceActionIcon::Next, Some(primary_width), theme::Intent::Accent)
                .on_hover_text("Save, complete this assignment, and claim another.")
                .clicked()
            {
                self.trigger_user_action(labello_domain::UserAction::NextImage);
            }
        }
        if workspace_toolbar_button(ui, ready, "Skip", WorkspaceActionIcon::Skip, Some(primary_width), theme::Intent::Neutral)
            .on_hover_text("Release this assignment and claim another.")
            .clicked()
        {
            self.trigger_user_action(labello_domain::UserAction::SkipAssignment);
        }
        if self.view == AppView::Annotate {
            let actions = self.annotation_secondary_actions(ui.ctx(), ready, false);
            self.dispatch_workspace_secondary(workspace_secondary_actions(ui, &actions, "More actions"));
        }
    }

    pub(crate) fn compact_workspace_actions(&mut self, ui: &mut egui::Ui) {
        if self.view == AppView::Review {
            self.review_bottom_actions(ui);
            return;
        }
        if self.manual_migration_active() {
            ui.horizontal_wrapped(|ui| self.migration_workspace_actions(ui, true));
            return;
        }
        let ready = (self.work.assignment.is_some() || self.runtime.api.is_none())
            && !self.loading.saving && !self.loading.image && self.work.pending_transition.is_none();
        ui.horizontal_wrapped(|ui| {
            let primary_width = (ui.available_width() - 44.0 - ui.spacing().item_spacing.x).max(44.0);
            if self.view == AppView::Annotate
                && workspace_toolbar_button(ui, ready, "Submit & next", WorkspaceActionIcon::Next, Some(primary_width), theme::Intent::Accent).clicked()
            {
                self.trigger_user_action(labello_domain::UserAction::NextImage);
            }
            let actions = if self.view == AppView::Annotate {
                self.annotation_secondary_actions(ui.ctx(), ready, true)
            } else {
                vec![self.workspace_secondary_action(ui.ctx(), labello_domain::UserAction::SkipAssignment, "Skip", ready, "Release this assignment and claim another.")]
            };
            let label = if self.view == AppView::Annotate { "More actions" } else { "More" };
            self.dispatch_workspace_secondary(workspace_secondary_actions(ui, &actions, label));
        });
    }

    fn annotation_secondary_actions(&self, ctx: &egui::Context, ready: bool, compact: bool) -> Vec<WorkspaceAction> {
        use labello_domain::UserAction;
        let mut actions = Vec::new();
        if compact || (self.work.previous_assignment.is_some() && matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry)) {
            actions.push(self.workspace_secondary_action(ctx, UserAction::PreviousImage, "Previous assignment", ready && (!compact || (self.work.previous_assignment.is_some() && self.runtime.api.is_some())), "Return to the last skipped or submitted assignment."));
        }
        actions.push(self.workspace_secondary_action(ctx, UserAction::UndoEdit, "Undo", ready && !self.work.undo_stack.is_empty(), "Undo the last edit."));
        actions.push(self.workspace_secondary_action(ctx, UserAction::RedoEdit, "Redo", ready && !self.work.redo_stack.is_empty(), "Redo the last undone edit."));
        if compact {
            actions.push(self.workspace_secondary_action(ctx, UserAction::SaveAnnotations, "Save", ready && matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry), "Save edits and keep this assignment active."));
            actions.push(self.workspace_secondary_action(ctx, UserAction::SkipAssignment, "Skip", ready, "Release this assignment and claim another."));
        }
        actions
    }

    fn drawer_panel_buttons(&mut self, ui: &mut egui::Ui, icon_only: bool) {
        self.drawer_panel_button(ui, Drawer::Workflow, "Workflow", false, icon_only);
        self.drawer_panel_button(ui, Drawer::Inspector, "Inspector", true, icon_only);
    }

    fn drawer_panel_button(
        &mut self,
        ui: &mut egui::Ui,
        drawer: Drawer,
        label: &'static str,
        panel_on_right: bool,
        icon_only: bool,
    ) {
        let selected = self.work.drawer == Some(drawer);
        let action = match drawer {
            Drawer::Workflow => labello_domain::UserAction::ToggleWorkflowPanel,
            Drawer::Inspector => labello_domain::UserAction::ToggleInspectorPanel,
        };
        let shortcut = self.shortcut_text(ui.ctx(), action);
        let icon_id = ui.id().with(("drawer-panel-icon", label));
        let icon_width = if icon_only { 20.0 } else { 25.0 };
        let icon = egui::Atom::custom(icon_id, egui::vec2(icon_width, 16.0));
        let content = if icon_only {
            egui::Atoms::new(icon)
        } else {
            egui::Atoms::new((icon, RichText::new(label)))
        };
        let choice = egui::Button::new(content)
            .selected(selected)
            .min_size(egui::vec2(if icon_only { 44.0 } else { 0.0 }, 44.0))
            .gap(theme::SPACE_2)
            .atom_ui(ui);
        let icon_rect = choice.rect(icon_id);
        let response = choice
            .response
            .on_hover_text(format!("{label} ({shortcut})"));
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                selected,
                label,
            )
        });
        if let Some(icon_rect) = icon_rect {
            paint_side_panel_toggle_icon(
                ui,
                icon_rect,
                !selected,
                panel_on_right,
                ui.style().interact(&response).fg_stroke.color,
            );
        }
        if response.clicked() {
            self.trigger_user_action(action);
        }
    }
}

fn drawer_panel_labels_fit(ui: &egui::Ui) -> bool {
    let spacing = ui.spacing().item_spacing.x;
    panel_label_button_width(ui, "Workflow")
        + panel_label_button_width(ui, "Inspector")
        + spacing
        <= ui.available_size_before_wrap().x + 0.5
}

fn panel_label_button_width(ui: &egui::Ui, label: &str) -> f32 {
    25.0 + theme::SPACE_2 + text_button_width(ui, label)
}

fn text_button_width(ui: &egui::Ui, label: &str) -> f32 {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let text_width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(label.to_owned(), font_id, theme::TEXT)
            .size()
            .x
    });
    (text_width + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x)
}

#[derive(Clone, Copy)]
pub(crate) enum WorkspaceActionIcon { Approve, Reject, Previous, Discard, Skip, Fit, Save, Next, Undo, Redo, Add, Remove, Pan, Refocus }

pub(crate) fn workspace_action_button(ui: &mut egui::Ui, enabled: bool, label: &str, icon: WorkspaceActionIcon, width: Option<f32>, intent: theme::Intent) -> egui::Response {
    let width = width.unwrap_or_else(|| text_button_width(ui, label).min(ui.available_size_before_wrap().x.max(44.0)));
    let icon_only = text_button_width(ui, label) > width;
    let button = egui::Button::new(if icon_only { "" } else { label }).min_size(egui::vec2(width, 44.0));
    let response = match intent {
        theme::Intent::Accent if enabled => theme::primary_button(ui, enabled, button),
        theme::Intent::Error => theme::danger_button(ui, enabled, button),
        _ => theme::quiet_button(ui, enabled, button),
    }.on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if icon_only { paint_workspace_action_icon(ui, &response, icon); }
    response
}

fn paint_workspace_action_icon(ui: &egui::Ui, response: &egui::Response, icon: WorkspaceActionIcon) {
        let center = response.rect.center();
        let stroke = egui::Stroke::new(2.0, ui.style().interact(response).fg_stroke.color);
        let point = |x, y| center + egui::vec2(x, y);
        let line = |a, b| { ui.painter().line_segment([a, b], stroke); };
        match icon {
            WorkspaceActionIcon::Approve => { line(point(-8.0, 0.0), point(-2.0, 6.0)); line(point(-2.0, 6.0), point(9.0, -7.0)); }
            WorkspaceActionIcon::Reject => { line(point(-7.0, -7.0), point(7.0, 7.0)); line(point(-7.0, 7.0), point(7.0, -7.0)); }
            WorkspaceActionIcon::Previous | WorkspaceActionIcon::Undo => { line(point(8.0, 0.0), point(-8.0, 0.0)); line(point(-8.0, 0.0), point(-1.0, -7.0)); line(point(-8.0, 0.0), point(-1.0, 7.0)); }
            WorkspaceActionIcon::Discard => { ui.painter().circle_stroke(point(1.0, 1.0), 8.0, stroke); line(point(-9.0, -8.0), point(-9.0, -1.0)); line(point(-9.0, -1.0), point(-2.0, -1.0)); }
            WorkspaceActionIcon::Skip | WorkspaceActionIcon::Next | WorkspaceActionIcon::Redo => { line(point(-7.0, -7.0), point(4.0, 0.0)); line(point(4.0, 0.0), point(-7.0, 7.0)); line(point(8.0, -8.0), point(8.0, 8.0)); }
            WorkspaceActionIcon::Save => { ui.painter().rect_stroke(egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0)), 1.0, stroke, egui::StrokeKind::Inside); line(point(-5.0, -8.0), point(-5.0, -1.0)); line(point(-5.0, -1.0), point(5.0, -1.0)); line(point(5.0, -1.0), point(5.0, -8.0)); }
            WorkspaceActionIcon::Add => { line(point(-8.0, 0.0), point(8.0, 0.0)); line(point(0.0, -8.0), point(0.0, 8.0)); }
            WorkspaceActionIcon::Remove => { line(point(-8.0, -6.0), point(8.0, -6.0)); line(point(-5.0, -3.0), point(-5.0, 8.0)); line(point(-5.0, 8.0), point(5.0, 8.0)); line(point(5.0, 8.0), point(5.0, -3.0)); }
            WorkspaceActionIcon::Pan => { line(point(-9.0, 0.0), point(9.0, 0.0)); line(point(0.0, -9.0), point(0.0, 9.0)); for (x, y) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] { line(point(x * 9.0, y * 9.0), point(x * 5.0 - y * 3.0, y * 5.0 + x * 3.0)); line(point(x * 9.0, y * 9.0), point(x * 5.0 + y * 3.0, y * 5.0 - x * 3.0)); } }
            WorkspaceActionIcon::Refocus => { ui.painter().circle_stroke(center, 9.0, stroke); ui.painter().circle_stroke(center, 4.0, stroke); }
            WorkspaceActionIcon::Fit => { for (x, y) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] { line(point(x * 9.0, y * 4.0), point(x * 9.0, y * 9.0)); line(point(x * 9.0, y * 9.0), point(x * 4.0, y * 9.0)); } }
        }
}

pub(crate) fn workspace_toolbar_button(ui: &mut egui::Ui, enabled: bool, label: &str, icon: WorkspaceActionIcon, width: Option<f32>, intent: theme::Intent) -> egui::Response {
    let width = width.map(|width| width.min(text_button_width(ui, label)));
    workspace_action_button(ui, enabled, label, icon, width, intent)
}
