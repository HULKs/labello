struct ReviewBarContent {
    identity: String,
    type_and_phase: Option<(String, String)>,
    accessible: String,
}

impl ReviewBarContent {
    fn from_app(app: &LabelloApp) -> Self {
        if let Some(context) = app.review_context() {
            let identity = if context.workflow_name == context.class_name {
                context.workflow_name.clone()
            } else {
                format!("{} · {}", context.workflow_name, context.class_name)
            };
            let identity = if context.revision_mode {
                format!("Revising · {identity}")
            } else {
                identity
            };
            let phase = match context.phase {
                crate::review_context::ReviewContextPhase::Object { number, total, .. } => format!("Item {number} / {total}"),
                crate::review_context::ReviewContextPhase::FullImage { .. } => "Image overview".to_string(),
            };
            let phase = if context.unsaved_corrections > 0 { format!("{phase} · {} {}", context.unsaved_corrections, if context.unsaved_corrections == 1 { "correction" } else { "corrections" }) } else { phase };
            Self {
                identity: phase,
                type_and_phase: Some((identity, context.type_label().to_string())),
                accessible: format!("Review details: {}. Toggle Inspector.", context.accessible_summary()),
            }
        } else {
            let message = if app.loading.image
                && matches!(app.work.pending_transition, Some(PendingTransition::PreviousAssignment(_)))
            {
                "Opening previous review…"
            } else if app.loading.image || app.loading.dataset || app.loading.session {
                "Loading review target…"
            } else if app.work.assignment.is_none() {
                "No active review assignment"
            } else {
                "Review target unavailable"
            };
            Self {
                identity: message.to_string(),
                type_and_phase: None,
                accessible: message.to_string(),
            }
        }
    }
}

struct ReviewBarText {
    lines: Vec<std::sync::Arc<egui::Galley>>,
    width: f32,
    height: f32,
    availability_loading: bool,
}

impl ReviewBarText {
    fn measure(ctx: &egui::Context, content: &ReviewBarContent, width: f32, availability_loading: bool) -> Self {
        let width = width.floor().max(44.0);
        let inner_width = (width - 44.0).max(1.0);

        let layout = |text: String, truncate: bool| {
            let line_width = if truncate && availability_loading {
                (inner_width - 24.0).max(1.0)
            } else {
                inner_width
            };
            let mut job =
                egui::text::LayoutJob::simple(text, if truncate { egui::TextStyle::Button } else { egui::TextStyle::Small }.resolve(&ctx.global_style()), if truncate { theme::TEXT } else { theme::TEXT_MUTED }, line_width);
            {
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                job.wrap.overflow_character = Some('…');
            }
            ctx.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let mut lines = vec![layout(content.identity.clone(), true)];
        if let Some((kind, phase)) = &content.type_and_phase {
            // Full identity remains available through the tooltip and inspector.
            lines.push(layout(format!("{kind} · {phase}"), false));
        }
        let height = (lines.iter().map(|line| line.size().y).sum::<f32>() + 8.0).max(44.0);
        let content_width = lines.iter().enumerate().map(|(index, line)| {
            line.size().x + if index == 0 && availability_loading { 24.0 } else { 0.0 }
        }).fold(0.0_f32, f32::max);
        let width = (content_width.ceil() + 44.0).min(width);
        Self {
            lines,
            width,
            height,
            availability_loading,
        }
    }
}

impl LabelloApp {
    fn review_revision_in_compact_context(&self, ctx: &egui::Context) -> bool {
        let viewport = ctx.content_rect().size();
        LayoutMode::for_width(viewport.x) == LayoutMode::Compact
            && Self::short_viewport(viewport)
            && self.review_context().is_some_and(|context| context.revision_mode)
    }

    fn review_summary_width(&self, ctx: &egui::Context, layout: LayoutMode, available: f32) -> f32 {
        if layout == LayoutMode::Wide {
            available.min(380.0)
        } else {
            let spacing = ctx.global_style().spacing.item_spacing.x;
            available - 44.0 - spacing
        }
    }

    fn review_inline_availability_loading(&self, layout: LayoutMode) -> bool {
        layout != LayoutMode::Wide
            && self.work.availability.loading
            && self.work.availability.tasks.is_empty()
    }

    pub(crate) fn review_context_bar_height(
        &self,
        ctx: &egui::Context,
        layout: LayoutMode,
        viewport_width: f32,
    ) -> f32 {
        let content = ReviewBarContent::from_app(self);
        let width = self.review_summary_width(ctx, layout, viewport_width - 28.0);
        let text = ReviewBarText::measure(ctx, &content, width, self.review_inline_availability_loading(layout));
        text.height
            + 4.0
            + if layout == LayoutMode::Wide {
                0.0
            } else {
                44.0
            }
    }

    fn review_context_bar(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        let content = ReviewBarContent::from_app(self);
        let width = self.review_summary_width(ui.ctx(), layout, ui.available_width());
        let text = ReviewBarText::measure(ui.ctx(), &content, width, self.review_inline_availability_loading(layout));
        let valid = content.type_and_phase.is_some();
        if !valid || self.work.drawer == Some(Drawer::Workflow) {
            self.work.review_details_focus_return = None;
        }
        let response = if layout != LayoutMode::Wide {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    self.review_details_button(ui, &content, &text);
                    self.drawer_panel_button(ui, Drawer::Workflow, "Workflow", false, true);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                    self.previous_review_action(ui);
                    self.discard_review_action(ui);
                });
            })
        } else {
            workspace_context_row(ui, self.work.availability.loading && self.work.availability.tasks.is_empty(), |ui| {
                self.review_details_button(ui, &content, &text);
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                    self.previous_review_action(ui);
                    self.discard_review_action(ui);
                });
                if !self.manual_migration_active() {
                    ui.separator();
                    self.workspace_actions(ui, layout);
                }
                if let Some(current) = self.work.current.as_ref()
                    && ui.available_size_before_wrap().x >= 80.0
                {
                    ui.add_sized(
                        [ui.available_size_before_wrap().x.min(160.0), 44.0],
                        egui::Label::new(&current.image.file_name).truncate(),
                    )
                    .on_hover_text(&current.image.file_name);
                }
            })
        };
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Workspace context bar")
        });
    }

    fn review_details_button(
        &mut self,
        ui: &mut egui::Ui,
        content: &ReviewBarContent,
        text: &ReviewBarText,
    ) {
        let id = ui.id().with("review-context-details");
        let selected = if LayoutMode::for_width(ui.ctx().content_rect().width()) == LayoutMode::Wide
        {
            !self.work.inspector_panel_collapsed
        } else {
            self.work.drawer == Some(Drawer::Inspector)
        };
        let choice = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);
                egui::Button::new(egui::Atom::custom(
                    id,
                    egui::vec2(text.width - 12.0, text.height - 8.0),
                ))
                .selected(selected)
                .min_size(egui::vec2(text.width, text.height))
                .atom_ui(ui)
            })
            .inner;
        if let Some(rect) = choice.rect(id) {
            let mut pos = rect.min + egui::vec2(2.0, 2.0);
            let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0, rect.center().y), egui::vec2(18.0, 18.0));
            paint_side_panel_toggle_icon(ui, icon_rect, !selected, true, theme::TEXT_MUTED);
            for line in &text.lines {
                ui.painter().galley(pos, line.clone(), theme::TEXT);
                pos.y += line.size().y;
            }
            if text.availability_loading {
                let line_height = text.lines[0].size().y;
                let side = 16.0_f32.min(line_height);
                let spinner_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - 28.0 - side, rect.top() + (line_height - side) / 2.0),
                    egui::vec2(side, side),
                );
                // The identity line already reserves this slot; do not advance the row cursor.
                let mut spinner_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("review-context-availability")
                        .max_rect(spinner_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                let spinner = spinner_ui.add(egui::Spinner::new().size(side));
                Self::describe_assignment_availability_spinner(spinner);
            }
        }
        let response = choice.response.on_hover_text(&content.accessible);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                selected,
                &content.accessible,
            )
        });
        if self.work.drawer.is_none()
            && self.work.review_details_focus_return == Some(response.id)
            && !self.work.show_settings
            && self.work.pending_transition.is_none()
        {
            // The modal layer persists for one frame after dismissal. Returning
            // focus before it retires leaves a pending Tab traversal active.
            if ui.ctx().memory(|memory| memory.top_modal_layer().is_none()) {
                response.request_focus();
                self.work.review_details_focus_return = None;
            } else {
                ui.ctx().request_repaint();
            }
        }
        if response.clicked() {
            self.work.show_tutorial = false;
            ui.ctx().request_repaint();
            if LayoutMode::for_width(ui.ctx().content_rect().width()) == LayoutMode::Wide {
                self.work.inspector_panel_collapsed = selected;
            } else {
                self.work.drawer = (!selected).then_some(Drawer::Inspector);
                self.work.review_details_focus_return = Some(response.id);
            }
        }
    }
}
