#[derive(Clone)]
pub(crate) struct ReviewBarContent {
    identity: String,
    type_and_phase: Option<(String, String)>,
    accessible: String,
    submitter: Option<(String, Option<String>)>,
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
            let submitter = app.review_submitter();
            let attribution = submitter.as_ref().map(|(name, _)| format!(" Submitted by {name}."))
                .unwrap_or_else(|| " Submitter unavailable.".into());
            Self {
                submitter,
                identity: phase,
                type_and_phase: Some((identity, context.type_label().to_string())),
                accessible: format!("Review details: {}.{attribution}", context.accessible_summary()),
            }
        } else {
            let message = if app.work.assignment.is_none() {
                "No active review assignment"
            } else {
                "Review target unavailable"
            };
            Self {
                submitter: None,
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
            lines.push(layout(format!("{phase} · {kind}"), false));
        }
        let height = lines.iter().map(|line| line.size().y).sum::<f32>().max(32.0);
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
    fn review_summary_width(&self, ctx: &egui::Context, layout: LayoutMode, available: f32) -> f32 {
        if layout == LayoutMode::Compact {
            available
        } else if layout == LayoutMode::Wide {
            available.min(380.0)
        } else {
            let spacing = ctx.global_style().spacing.item_spacing.x;
            available - 4.0 * (44.0 + spacing)
        }
    }

    fn review_inline_availability_loading(&self, layout: LayoutMode) -> bool {
        layout != LayoutMode::Wide
            && self.bar_availability_loading()
    }

    pub(crate) fn review_context_bar_height(
        &self,
        ctx: &egui::Context,
        layout: LayoutMode,
        viewport_width: f32,
    ) -> f32 {
        let content = self.displayed_review_bar();
        let width = self.review_summary_width(ctx, layout, viewport_width - 28.0);
        let text = ReviewBarText::measure(ctx, &content, width, self.review_inline_availability_loading(layout));
        if layout == LayoutMode::Compact { text.height + 46.0 } else { text.height.max(44.0) + 14.0 }
    }

    fn review_context_bar(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        let content = self.displayed_review_bar();
        let width = self.review_summary_width(ui.ctx(), layout, ui.available_width());
        let text = ReviewBarText::measure(ui.ctx(), &content, width, self.review_inline_availability_loading(layout));
        let valid = content.type_and_phase.is_some();
        if !valid || self.work.drawer == Some(Drawer::Workflow) {
            self.work.review_details_focus_return = None;
        }
        let response = if layout == LayoutMode::Compact {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.review_summary(ui, &content, &text);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                    self.drawer_panel_buttons(ui, true);
                });
            })
        } else if layout != LayoutMode::Wide {
            ui.horizontal(|ui| {
                self.review_summary(ui, &content, &text);
                ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                self.drawer_panel_buttons(ui, true);
            })
        } else {
            workspace_context_row(ui, self.bar_availability_loading(), |ui| {
                self.review_summary(ui, &content, &text);
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui, layout));
                });
                if let Some(current) = self.displayed_bar_image().as_ref()
                    && ui.available_size_before_wrap().x >= 80.0
                {
                    ui.add_sized(
                        [ui.available_size_before_wrap().x.min(160.0), 44.0],
                        egui::Label::new(&current.file_name).truncate(),
                    )
                    .on_hover_text(&current.file_name);
                }
            })
        };
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Workspace context bar")
        });
    }

    fn review_summary(
        &self, ui: &mut egui::Ui, content: &ReviewBarContent, text: &ReviewBarText,
    ) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(text.width, text.height), egui::Sense::hover());
        let avatar_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y), egui::Vec2::splat(24.0));
        if content.type_and_phase.is_some() && !content.accessible.is_empty() {
            let (name, github_id) = content.submitter.as_ref()
                .map(|(name, id)| (name.as_str(), id.as_deref()))
                .unwrap_or(("?", None));
            crate::avatar::paint(ui, github_id, name, avatar_rect);
        }
        let mut pos = rect.min + egui::vec2(36.0, 0.0);
        for line in &text.lines {
            ui.painter().galley(pos, line.clone(), theme::TEXT);
            pos.y += line.size().y;
        }
        {
            let line_height = text.lines[0].size().y;
            let side = 16.0_f32.min(line_height);
            let spinner_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - side, rect.top()), egui::Vec2::splat(side));
            let mut spinner_ui = ui.new_child(egui::UiBuilder::new()
                .id_salt("review-context-availability").max_rect(spinner_rect));
            // Keep following controls on the same IDs when availability completes.
            if text.availability_loading {
                Self::describe_assignment_availability_spinner(spinner_ui.add(egui::Spinner::new().size(side)));
            }
        }
        ui.ctx().accesskit_node_builder(response.id, |node| node.set_label(content.accessible.clone()));
        response.on_hover_text(&content.accessible).widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &content.accessible)
        });
    }

    fn review_submitter(&self) -> Option<(String, Option<String>)> {
        let assignment = self.work.assignment.as_ref()?;
        let state = self.work.current_state.as_ref()?;
        let task = self.selected_task()?;
        let user = if task.manual_box_guide_migration.is_some() {
            &state.migration_confirmations.get(&task.task_id)?.actor_user_id
        } else {
            &state.review_rounds.get(&task.task_id)?.submitted_by
        };
        let profile = self.work.review_submitters.iter().find(|entry|
            entry.image_id == assignment.image_id && entry.task_id == task.task_id && entry.user_id == *user);
        let name = profile.and_then(|entry| entry.github_login.as_deref())
            .filter(|login| !login.is_empty()).map(|login| format!("@{login}"))
            .unwrap_or_else(|| user.to_string());
        Some((name, profile.and_then(|entry| entry.github_user_id.clone())))
    }
}
