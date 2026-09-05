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
            let phase = if context.correction.is_some() {
                "Correction mode".to_string()
            } else if matches!(
                context.phase,
                crate::review_context::ReviewContextPhase::FullImage { .. }
            ) {
                "Final check".to_string()
            } else {
                context.phase_label()
            };
            Self {
                identity,
                type_and_phase: Some((context.type_label().to_string(), phase)),
                accessible: format!(
                    "Review details: {}. Open Inspector for full details.",
                    context.accessible_summary()
                ),
            }
        } else {
            let message = if app.loading.image || app.loading.dataset || app.loading.session {
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
}

impl ReviewBarText {
    fn measure(ctx: &egui::Context, content: &ReviewBarContent, width: f32) -> Self {
        let width = width.floor().max(44.0);
        let inner_width = (width - 12.0).max(1.0);
        let font = egui::TextStyle::Body.resolve(&ctx.global_style());
        let layout = |text: String, truncate: bool| {
            let mut job =
                egui::text::LayoutJob::simple(text, font.clone(), theme::TEXT, inner_width);
            if truncate {
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                job.wrap.overflow_character = Some('…');
            }
            ctx.fonts_mut(|fonts| fonts.layout_job(job))
        };
        let mut lines = vec![layout(content.identity.clone(), true)];
        if let Some((kind, phase)) = &content.type_and_phase {
            // Keep the full type and phase. Only the identity line may truncate.
            lines.push(layout(format!("{kind} · {phase}"), false));
        }
        let height = (lines.iter().map(|line| line.size().y).sum::<f32>() + 8.0).max(44.0);
        Self {
            lines,
            width,
            height,
        }
    }
}

impl LabelloApp {
    fn review_summary_width(&self, ctx: &egui::Context, layout: LayoutMode, available: f32) -> f32 {
        if layout == LayoutMode::Wide {
            available.min(340.0)
        } else {
            let spacing = ctx.global_style().spacing.item_spacing.x;
            let availability =
                if self.work.availability.loading && self.work.availability.tasks.is_empty() {
                    18.0 + spacing
                } else {
                    0.0
                };
            available - 44.0 - spacing - availability
        }
    }

    pub(crate) fn review_context_bar_height(
        &self,
        ctx: &egui::Context,
        layout: LayoutMode,
        viewport_width: f32,
    ) -> f32 {
        let content = ReviewBarContent::from_app(self);
        let width = self.review_summary_width(ctx, layout, viewport_width - 28.0);
        let text = ReviewBarText::measure(ctx, &content, width);
        text.height
            + 12.0
            + if layout == LayoutMode::Wide {
                0.0
            } else {
                44.0
            }
    }

    fn review_context_bar(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        let content = ReviewBarContent::from_app(self);
        let width = self.review_summary_width(ui.ctx(), layout, ui.available_width());
        let text = ReviewBarText::measure(ui.ctx(), &content, width);
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
                    self.assignment_availability_spinner(ui);
                });
                ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
            })
        } else {
            ui.horizontal(|ui| {
                self.review_details_button(ui, &content, &text);
                ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                if !self.manual_migration_active() {
                    ui.separator();
                    self.workspace_actions(ui, layout);
                }
                if let Some(current) = self.work.current.as_ref()
                    && ui.available_width() >= 80.0
                {
                    ui.add_sized(
                        [ui.available_width().min(160.0), 44.0],
                        egui::Label::new(&current.image.file_name).truncate(),
                    )
                    .on_hover_text(&current.image.file_name);
                }
                self.assignment_availability_spinner(ui);
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
            let mut pos = rect.min;
            for line in &text.lines {
                ui.painter().galley(pos, line.clone(), theme::TEXT);
                pos.y += line.size().y;
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
            if LayoutMode::for_width(ui.ctx().content_rect().width()) == LayoutMode::Wide {
                self.work.inspector_panel_collapsed = false;
            } else {
                self.work.drawer = Some(Drawer::Inspector);
                self.work.review_details_focus_return = Some(response.id);
            }
        }
    }
}
