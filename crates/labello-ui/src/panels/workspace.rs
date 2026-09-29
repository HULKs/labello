impl LabelloApp {
    pub(crate) fn central(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        match self.view {
            AppView::Inspect => { self.dataset_inspector(ui); return; }
            AppView::Setup => {
                centered_scroll(ui, 1100.0, |ui| self.setup_view(ui, layout));
                return;
            }
            AppView::Admin => {
                centered_scroll(ui, 1100.0, |ui| self.admin_view(ui, layout));
                return;
            }
            AppView::Stats => {
                centered_scroll(ui, 1100.0, |ui| self.stats_view(ui, layout));
                return;
            }
            AppView::Annotate | AppView::Review => {}
        }
        let canvas_rect = ui.available_rect_before_wrap();
        self.workspace_canvas(ui);
        self.workflow_change_notice(ui.ctx(), canvas_rect);
        self.saved_workflow_reason_notice(ui.ctx(), canvas_rect);
    }

    fn workflow_change_notice(&mut self, ctx: &egui::Context, canvas: egui::Rect) {
        let Some(message) = self.work.workflow.change_notice.clone() else { return; };
        let frame = theme::inset_frame();
        let width = (canvas.width() - 16.0 - frame.total_margin().sum().x).clamp(80.0, 480.0);
        let height = (canvas.height() - 16.0 - frame.total_margin().sum().y).clamp(44.0, 160.0);
        let notice = egui::Area::new(egui::Id::new("workflow-change-notice"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(canvas.center().x, canvas.top() + 8.0))
            .pivot(egui::Align2::CENTER_TOP)
            .constrain_to(canvas)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal_top(|ui| {
                        let close = ui.add_sized(egui::vec2(44.0, 44.0), egui::Button::new("×")).on_hover_text("Dismiss workflow change");
                        close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Dismiss workflow change"));
                        if close.clicked() { self.work.workflow.change_notice = None; }
                        egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height)
                            .scroll_source(crate::pointer_input::scroll_source(ctx))
                            .show(ui, |ui| {
                                let label = ui.add(egui::Label::new(message).wrap());
                                ctx.accesskit_node_builder(label.id, |node| node.set_role(egui::accesskit::Role::Status));
                            });
                    });
                });
            });
        notice.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Workflow change notice"));
    }

    fn saved_workflow_reason_notice(&mut self, ctx: &egui::Context, canvas: egui::Rect) {
        self.clear_reason_notice_outside_scope();
        if !self.reason_notice_visible() { return; }
        let width = (canvas.width() - 16.0).clamp(200.0, 520.0);
        egui::Area::new(egui::Id::new("saved-workflow-reasons"))
            .order(egui::Order::Middle)
            .fixed_pos(canvas.left_top() + egui::vec2(8.0, 8.0))
            .constrain_to(canvas)
            .show(ctx, |ui| {
                ui.set_width(width);
                self.reason_notice_contents(ui, width, canvas.height());
            });
    }

    pub(crate) fn overlays(&mut self, ctx: &egui::Context, layout: LayoutMode) {
        if self.runtime.persistence.recovery.is_some() {
            self.navigation.statistics = Default::default();
            self.draft_recovery_modal(ctx);
            return;
        }
        if self
            .work
            .migration
            .pending_companion_reconciliation
            .is_some()
        {
            self.migration_companion_reconciliation_modal(ctx);
            return;
        }
        if self.navigation.statistics.open {
            self.statistics_overlay(ctx);
            return;
        }
        if self.work.migration.pending_revisit_target.is_some() || self.work.migration.pending_reload_discard {
            self.migration_revisit_discard_modal(ctx);
            return;
        }
        if self.work.pending_transition.is_some() && self.workflow_context().is_none() {
            self.transition_modal(ctx);
            return;
        }
        if self.admin.confirm_discard {
            self.admin_discard_modal(ctx);
            return;
        }
        if self.work.show_settings {
            self.settings_modal(ctx);
            return;
        }
        if self.navigation.drawer_open {
            self.application_navigation_drawer(ctx);
            return;
        }
        if self.view == AppView::Inspect { self.inspection_drawer(ctx, layout); }
        if layout != LayoutMode::Wide && self.work_view() {
            let screen = ctx.content_rect();
            let compact = layout == LayoutMode::Compact;
            let width = if self.work.drawer == Some(Drawer::Workflow) {
                self.workflow_panel_width(ctx)
                    .min((screen.width() - 48.0).max(240.0))
            } else if compact {
                (screen.width() - 96.0).max(240.0)
            } else {
                308.0_f32.min(screen.width() - 48.0)
            };
            let max_height = if compact {
                (screen.height() * 0.7)
                    .clamp(180.0, 560.0)
                    .min(screen.height() - 48.0)
            } else {
                (screen.height() - 48.0).max(180.0)
            };
            if let Some(drawer) = self.work.drawer {
                let (title, align, offset) = match drawer {
                    Drawer::Workflow => {
                        (crate::glossary::WORKFLOW, egui::Align2::LEFT_CENTER, egui::vec2(12.0, 0.0))
                    }
                    Drawer::Inspector => (
                        crate::glossary::INSPECTOR,
                        egui::Align2::RIGHT_CENTER,
                        egui::vec2(-12.0, 0.0),
                    ),
                };
                let id = egui::Id::new("workspace-drawer");
                let area = egui::Modal::default_area(id)
                    .anchor(align, offset)
                    .default_width(width)
                    .constrain_to(screen);
                let mut close = false;
                let response = theme::modal(ctx, id).area(area).show(ctx, |ui| {
                    ui.set_width(width);
                    ui.set_max_height(max_height);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button =
                                ui.add(egui::Button::new(crate::glossary::CLOSE).min_size(egui::vec2(64.0, 44.0)));
                            button.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Close {title}"),
                                )
                            });
                            if let Some(invoker) = self.work.work_panel_focus_return
                                && ui.ctx().memory(|memory| memory.focused()).is_none_or(|focused| focused == invoker)
                            {
                                button.request_focus();
                            }
                            close = button.clicked();
                        });
                    });
                    egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                        .max_height((max_height - 54.0).max(80.0))
                        .show(ui, |ui| match drawer {
                            Drawer::Workflow => self.task_panel(ui),
                            Drawer::Inspector => self.right_panel(ui, false),
                        });
                });
                response.response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Window, true, title)
                });
                if close || response.should_close() {
                    self.work.drawer = None;
                }
            }
        }
        self.tutorial_overlay(ctx);
    }

    pub(crate) fn workspace_context_bar(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        if self.workspace_bars_blank() { return; }
        if self.workspace_bars_loading() {
            let opacity = ui.opacity();
            ui.disable();
            ui.set_opacity(opacity);
        }
        self.shared_context_bar(ui, layout);
    }

    fn describe_assignment_availability_spinner(response: egui::Response) {
        response.on_hover_text("Checking assignment availability…").widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ProgressIndicator,
                true,
                "Loading workflow assignment availability",
            )
        });
    }

    fn canvas_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let show_refocus = self.context_has_refocus();
            if self.view != AppView::Review {
            let pan_shortcut =
                self.shortcut_text(ui.ctx(), labello_domain::UserAction::TogglePanMode);
            let pan_drag_shortcut =
                format!("{}+left-drag", self.work.keybindings.pan_drag_modifier);
            let pan_required = self.work.canvas.pan_mode_required();
            let pan_width = 44.0;
            let pan_icon = text_button_width(ui, crate::glossary::PAN) > pan_width;
            let pan = egui::Button::new(if pan_icon { "" } else { crate::glossary::PAN })
                .selected(self.work.canvas.pan_mode())
                .min_size(egui::vec2(pan_width, 44.0));
            let pan_response = ui.add_enabled(self.work.canvas.can_pan() && !pan_required, pan);
            pan_response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, self.work.canvas.can_pan() && !pan_required, self.work.canvas.pan_mode(), crate::glossary::PAN));
            if pan_icon { paint_workspace_action_icon(ui, &pan_response, WorkspaceActionIcon::Pan); }
            if pan_response
                .on_disabled_hover_text(if pan_required {
                    "Pan mode stays active during review."
                } else {
                    "Zoom in before enabling Pan mode."
                })
                .on_hover_text(if pan_required {
                    format!(
                        "Pan mode stays active during review. {pan_drag_shortcut} or middle-drag."
                    )
                } else {
                    format!("Pan ({pan_shortcut}). {pan_drag_shortcut} or middle-drag.")
                })
                .clicked()
            {
                self.trigger_user_action(labello_domain::UserAction::TogglePanMode);
            }

            }

            if show_refocus {
                let refocus_shortcut =
                    self.shortcut_text(ui.ctx(), labello_domain::UserAction::RefocusObject);
                let can_refocus = self.refocus_annotation().is_some();
                let refocus_label = format!("Refocus object {refocus_shortcut}");
                let response = workspace_action_button(ui, can_refocus, crate::glossary::REFOCUS, WorkspaceActionIcon::Refocus, Some(44.0), theme::Intent::Neutral)
                    .on_disabled_hover_text("Select an object to refocus.")
                    .on_hover_text(format!(
                        "Refocus object ({refocus_shortcut}). Center and zoom to the active object."
                    ));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        can_refocus,
                        refocus_label.clone(),
                    )
                });
                if response.clicked() {
                    self.trigger_user_action(labello_domain::UserAction::RefocusObject);
                }
            }

            let fit_shortcut = self.shortcut_text(ui.ctx(), labello_domain::UserAction::FitImage);
            if workspace_action_button(ui, true, crate::glossary::FIT, WorkspaceActionIcon::Fit, Some(44.0), theme::Intent::Neutral)
                .on_hover_text(format!("Fit ({fit_shortcut}). Or double-click canvas."))
                .clicked()
            {
                self.trigger_user_action(labello_domain::UserAction::FitImage);
            }

        });
    }
}
