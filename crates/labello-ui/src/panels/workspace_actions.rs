impl LabelloApp {
    pub(crate) fn review_secondary_action_count(&self) -> usize {
        2 + usize::from(self.workflow_context().is_none()) + usize::from(self.bar_has_previous_image()) + usize::from(self.bar_review_removable())
    }

    fn review_bottom_actions(&mut self, ui: &mut egui::Ui) {
        let ready = self.work.assignment.is_some()
            && !self.loading.saving && !self.loading.image && !self.work.migration.busy
            && self.work.pending_transition.is_none();
        let compact = LayoutMode::for_width(ui.ctx().content_rect().width()) != LayoutMode::Wide;
        let secondary = |app: &mut Self, ui: &mut egui::Ui| {
            let removable = app.bar_review_removable();
            let count = app.review_secondary_action_count() as f32;
            let width = compact.then(|| ((ui.available_width() - (count - 1.0) * ui.spacing().item_spacing.x) / count).floor().max(44.0));
            app.review_object_navigation(ui, width);
            app.previous_review_action(ui, width);
            app.discard_review_action(ui, width);
            if removable && workspace_action_button(ui, ready && app.work.review_corrections.submission.is_none(), "Remove item", WorkspaceActionIcon::Remove, width, theme::Intent::Error).clicked() {
                app.remove_migration_review_item();
            }
            if workspace_action_button(ui, ready, crate::glossary::SKIP, WorkspaceActionIcon::Skip, width, theme::Intent::Neutral).clicked() {
                app.trigger_user_action(labello_domain::UserAction::SkipAssignment);
            }
        };
        if compact {
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| secondary(self, ui));
                ui.horizontal(|ui| {
                    self.review_next_object_action(ui, Some(44.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.review_decision_buttons(ui, false, true);
                    });
                });
            });
        } else {
            ui.horizontal(|ui| {
                let primary_width = text_button_width(ui, self.bar_review_primary_label());
                let secondary_width = (ui.available_width() - primary_width - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(secondary_width, 44.0), egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                    secondary(self, ui);
                    self.review_next_object_action(ui, None);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.review_decision_buttons(ui, false, false);
                });
            });
        }
    }

    fn previous_review_action(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        if self.view == AppView::Review && self.bar_has_previous_image()
            && workspace_action_button(ui, (self.previous_work_item().is_some() || self.workflow_context().is_none()) && !self.loading.saving && !self.loading.image && !self.work.migration.busy && self.work.pending_transition.is_none(),
                crate::glossary::PREVIOUS, WorkspaceActionIcon::Previous, width, theme::Intent::Neutral).on_hover_text("Return to the immediately previous eligible assignment.").clicked()
        {
            self.trigger_user_action(labello_domain::UserAction::PreviousImage);
        }
    }

    fn discard_review_action(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        let ready = self.has_review_corrections() && self.work.review_corrections.submission.is_none() && !self.loading.saving && !self.loading.image && !self.work.migration.busy && self.work.pending_transition.is_none();
        let label = crate::glossary::DISCARD_CHANGES;
        if workspace_action_button(ui, ready, label, WorkspaceActionIcon::Discard, width, theme::Intent::Neutral).clicked() { self.discard_all_review_corrections(); }
    }

    pub(crate) fn workspace_actions(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        if !self.work_view() || self.workspace_bars_blank() { return; }
        if self.workspace_bars_loading() {
            let opacity = ui.opacity();
            ui.disable();
            ui.set_opacity(opacity);
        }
        if self.navigation.workspace_bars.presentation.is_none() { return; }
        if self.view == AppView::Review {
            self.review_bottom_actions(ui);
        } else if self.bar_migration_active() {
            self.migration_workspace_actions(ui, layout == LayoutMode::Compact);
        } else {
            self.annotation_bottom_actions(ui);
        }
    }

    pub(crate) fn compact_workspace_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| self.workspace_actions(ui, LayoutMode::Compact));
    }

    fn annotation_bottom_actions(&mut self, ui: &mut egui::Ui) {
        use labello_domain::UserAction;
        let ready = (self.work.assignment.is_some() || self.runtime.api.is_none())
            && !self.saving_blocks_interaction() && !self.loading.image && self.work.pending_transition.is_none();
        let can_commit = ready && !self.loading.saving;
        let dirty = matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry);
        let previous = self.bar_has_previous_image();
        use crate::prelabel_review::PrelabelPrimaryAction;
        let primary = self.bar_annotation_primary();
        let pending = !matches!(primary, PrelabelPrimaryAction::Submit);
        let (primary_label, primary_help) = match primary {
            PrelabelPrimaryAction::Guide => (crate::glossary::NEXT_GUIDE, "Save this box and focus the next source keypoint guide. The final box submits the image."),
            PrelabelPrimaryAction::Confirm => (crate::glossary::CONFIRM_NEXT, "Confirm this object's geometry or deletion and focus the next object."),
            PrelabelPrimaryAction::Focus => (crate::glossary::NEXT_OBJECT, "Focus the next prelabel that needs confirmation."),
            PrelabelPrimaryAction::Submit => (crate::glossary::SUBMIT_NEXT, "Save, complete this assignment, and claim another."),
        };
        let objects = self.annotation_objects();
        let selected = objects.iter().any(|object| !object.deleted && Some(&object.annotation_id) == self.work.selected_annotation.as_ref());
        let save_in_menu = LayoutMode::for_width(ui.ctx().content_rect().width()) == LayoutMode::Compact;
        let count = 5 + usize::from(previous) - usize::from(save_in_menu) - usize::from(self.workflow_context().is_some());
        let width = ((ui.available_width() - 44.0 - count as f32 * ui.spacing().item_spacing.x)
            / count as f32).floor().max(44.0);
        let primary_width = width.min(text_button_width(ui, primary_label));
        let secondary_width = (ui.available_width() - primary_width - ui.spacing().item_spacing.x).max(0.0);
        ui.allocate_ui_with_layout(egui::vec2(secondary_width, 44.0), egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
        ui.push_id("annotation-primary-actions", |ui| {
            for (action, label, icon, enabled, intent, help) in [
                (UserAction::PreviousImage, crate::glossary::PREVIOUS, WorkspaceActionIcon::Previous, can_commit && self.runtime.api.is_some(), theme::Intent::Neutral, "Return to the immediately previous eligible assignment."),
                (UserAction::SelectPreviousObject, crate::glossary::PREVIOUS_OBJECT, WorkspaceActionIcon::Previous, ready && !objects.is_empty(), theme::Intent::Neutral, "Select the previous object in this image, wrapping from the first to the last."),
                (UserAction::SaveAnnotations, crate::glossary::SAVE, WorkspaceActionIcon::Save, can_commit && dirty, theme::Intent::Neutral, "Save confirmed annotations and keep this assignment active."),
                (UserAction::DeleteAnnotation, crate::glossary::DELETE, WorkspaceActionIcon::Remove, ready && selected, theme::Intent::Error, "Delete the selected object. A pending model box still needs Confirm & next."),
                (UserAction::SkipAssignment, crate::glossary::SKIP, WorkspaceActionIcon::Skip, can_commit, theme::Intent::Neutral, "Release this assignment and claim another."),
            ] {
                if action == UserAction::PreviousImage && !previous || action == UserAction::SaveAnnotations && save_in_menu || action == UserAction::SelectPreviousObject && self.workflow_context().is_some() { continue; }
                let enabled = enabled && (action != UserAction::PreviousImage || self.previous_work_item().is_some() || self.workflow_context().is_none());
                ui.push_id(action, |ui| {
                    if workspace_toolbar_button(ui, enabled, label, icon, Some(width), intent)
                        .on_hover_text(format!("{help} ({})", self.shortcut_text(ui.ctx(), action))).clicked() {
                        self.trigger_user_action(action);
                    }
                });
            }
        });
        let mut actions = vec![
            self.workspace_secondary_action(ui.ctx(), UserAction::UndoEdit, crate::glossary::UNDO, ready && !self.work.undo_stack.is_empty(), "Undo the last edit."),
            self.workspace_secondary_action(ui.ctx(), UserAction::RedoEdit, crate::glossary::REDO, ready && !self.work.redo_stack.is_empty(), "Redo the last undone edit."),
        ];
        if save_in_menu {
            actions.insert(0, self.workspace_secondary_action(ui.ctx(), UserAction::SaveAnnotations, crate::glossary::SAVE, can_commit && dirty, "Save confirmed annotations and keep this assignment active."));
        }
        self.dispatch_workspace_secondary(workspace_secondary_actions(ui, &actions, "More actions"));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if workspace_toolbar_button(ui, ready && (pending || !self.loading.saving), primary_label, if pending { WorkspaceActionIcon::Approve } else { WorkspaceActionIcon::Next }, Some(width), theme::Intent::Accent)
                .on_hover_text(format!("{primary_help} ({})", self.shortcut_text(ui.ctx(), UserAction::NextImage))).clicked() {
                self.trigger_user_action(UserAction::NextImage);
            }
        });
    }

    fn review_object_navigation(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        if self.workflow_context().is_some() { return; }
        let ready = self.work.assignment.is_some() && !self.loading.saving && !self.loading.image
            && !self.work.migration.busy && self.work.pending_transition.is_none()
            && self.work.review_corrections.submission.is_none();
        let position = self.review_position();
        if workspace_toolbar_button(ui, ready && position > 0, crate::glossary::PREVIOUS_OBJECT, WorkspaceActionIcon::Previous, width, theme::Intent::Neutral)
            .on_hover_text("Return to the previous object in this image, retaining valid corrections. Stops at the first object.").clicked() {
            self.cycle_review_item(-1);
        }
    }

    fn review_next_object_action(&mut self, ui: &mut egui::Ui, width: Option<f32>) {
        if self.workflow_context().is_some() { return; }
        let position = self.review_position();
        let count = self.review_object_targets().len();
        let ready = self.work.assignment.is_some() && !self.loading.saving && !self.loading.image
            && !self.work.migration.busy && self.work.pending_transition.is_none()
            && self.work.review_corrections.submission.is_none() && position < count;
        if workspace_toolbar_button(ui, ready, self.bar_review_next_label(), WorkspaceActionIcon::Next, width, theme::Intent::Neutral)
            .on_hover_text("Continue within this image, retaining valid corrections, then show the full-image overview.").clicked() {
            self.cycle_review_item(1);
        }
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
        let padding = ui.spacing().button_padding;
        if icon_only { ui.spacing_mut().button_padding.x = 10.0; }
        let choice = egui::Button::new(content)
            .selected(selected)
            .min_size(egui::vec2(if icon_only { 44.0 } else { 0.0 }, 44.0))
            .gap(theme::SPACE_2)
            .atom_ui(ui);
        ui.spacing_mut().button_padding = padding;
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
        {
            if self.work.drawer.is_none()
                && self.work.work_panel_focus_return == Some(response.id)
                && !self.work.show_settings && self.work.pending_transition.is_none()
            {
                if ui.ctx().memory(|memory| memory.top_modal_layer().is_none()) {
                    response.request_focus();
                    self.work.work_panel_focus_return = None;
                } else { ui.ctx().request_repaint(); }
            }
            if response.clicked() { self.work.work_panel_focus_return = Some(response.id); }
        }
        if response.clicked() {
            self.trigger_user_action(action);
        }
    }
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
pub(crate) enum WorkspaceActionIcon { Approve, Previous, Discard, Skip, Fit, Save, Next, Undo, Redo, Remove, Pan, Refocus }

pub(crate) fn workspace_action_button(ui: &mut egui::Ui, enabled: bool, label: &str, icon: WorkspaceActionIcon, width: Option<f32>, intent: theme::Intent) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
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
            WorkspaceActionIcon::Previous | WorkspaceActionIcon::Undo => { line(point(8.0, 0.0), point(-8.0, 0.0)); line(point(-8.0, 0.0), point(-1.0, -7.0)); line(point(-8.0, 0.0), point(-1.0, 7.0)); }
            WorkspaceActionIcon::Discard => { ui.painter().circle_stroke(point(1.0, 1.0), 8.0, stroke); line(point(-9.0, -8.0), point(-9.0, -1.0)); line(point(-9.0, -1.0), point(-2.0, -1.0)); }
            WorkspaceActionIcon::Skip | WorkspaceActionIcon::Next | WorkspaceActionIcon::Redo => { line(point(-7.0, -7.0), point(4.0, 0.0)); line(point(4.0, 0.0), point(-7.0, 7.0)); line(point(8.0, -8.0), point(8.0, 8.0)); }
            WorkspaceActionIcon::Save => { ui.painter().rect_stroke(egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0)), 1.0, stroke, egui::StrokeKind::Inside); line(point(-5.0, -8.0), point(-5.0, -1.0)); line(point(-5.0, -1.0), point(5.0, -1.0)); line(point(5.0, -1.0), point(5.0, -8.0)); }
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
