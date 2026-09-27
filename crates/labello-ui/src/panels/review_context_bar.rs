#[derive(Clone)]
pub(crate) struct WorkspaceSummary {
    progress: String,
    identity_and_type: Option<(String, String)>,
    accessible: String,
    submitter: Option<(String, Option<String>)>,
    show_avatar: bool,
}

impl WorkspaceSummary {
    fn from_app(app: &LabelloApp) -> Self {
        if app.view == AppView::Review { return Self::from_review(app); }
        let task = app.selected_task();
        let current = app.work.current.as_ref();
        let progress = if current.is_none() {
            if app.work.assignment.is_some() { "Preview unavailable".into() } else { "No active assignment".into() }
        } else if app.manual_migration_active() {
            app.migration_context_progress()
        } else if let Some(progress) = app.prelabel_progress() {
            progress
        } else {
            let objects = app.annotation_objects();
            objects.iter().position(|object| Some(&object.annotation_id) == app.work.selected_annotation.as_ref())
                .map(|index| format!("Item {} / {}", index + 1, objects.len()))
                .unwrap_or_else(|| "Image overview".into())
        };
        let identity_and_type = task.filter(|_| current.is_some()).map(|task| {
            let class = task.class_ids.first().map(|id| app.class_name(id)).unwrap_or_default();
            let identity = if task.name == class { task.name.clone() } else { format!("{} · {class}", task.name) };
            let kind = match task.annotation_type { AnnotationType::BoundingBox => "Bounding boxes", AnnotationType::Skeleton => "Skeletons" };
            (identity, kind.to_string())
        });
        let mut accessible = format!("Annotation details: {progress}");
        if let Some((identity, kind)) = &identity_and_type { accessible.push_str(&format!(". {kind} · {identity}")); }
        if let Some(current) = current { accessible.push_str(&format!(". Image: {} · {} x {}", current.image.file_name, current.image.width, current.image.height)); }
        Self { progress, identity_and_type, accessible, submitter: None, show_avatar: false }
    }

    fn from_review(app: &LabelloApp) -> Self {
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
            let image = app.work.current.as_ref().map(|current| format!(" Image: {} · {} x {}.", current.image.file_name, current.image.width, current.image.height)).unwrap_or_default();
            Self {
                submitter,
                show_avatar: true,
                progress: phase,
                identity_and_type: Some((identity, context.type_label().to_string())),
                accessible: format!("Review details: {}.{attribution}", context.accessible_summary()) + &image,
            }
        } else {
            let message = if app.work.assignment.is_none() {
                "No active review assignment"
            } else {
                "Review target unavailable"
            };
            Self {
                submitter: None,
                show_avatar: false,
                progress: message.to_string(),
                identity_and_type: None,
                accessible: message.to_string(),
            }
        }
    }
}

struct WorkspaceSummaryText {
    lines: Vec<std::sync::Arc<egui::Galley>>,
    width: f32,
    height: f32,
    availability_loading: bool,
}

impl WorkspaceSummaryText {
    fn measure(ctx: &egui::Context, content: &WorkspaceSummary, width: f32, availability_loading: bool) -> Self {
        let width = width.floor().max(44.0);
        let inset = if content.show_avatar { 36.0 } else { 0.0 };
        let inner_width = (width - inset - 8.0).max(1.0);

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
        let mut lines = vec![layout(content.progress.clone(), true)];
        if let Some((kind, phase)) = &content.identity_and_type {
            // Full identity remains available through the tooltip and inspector.
            lines.push(layout(format!("{phase} · {kind}"), false));
        }
        let height = lines.iter().map(|line| line.size().y).sum::<f32>().max(32.0);
        let content_width = lines.iter().enumerate().map(|(index, line)| {
            line.size().x + if index == 0 && availability_loading { 24.0 } else { 0.0 }
        }).fold(0.0_f32, f32::max);
        let width = (content_width.ceil() + inset + 8.0).min(width);
        Self {
            lines,
            width,
            height,
            availability_loading,
        }
    }
}

impl LabelloApp {
    fn context_has_refocus(&self) -> bool {
        self.view == AppView::Review || self.bar_migration_active() || self.bar_prelabel_progress().is_some()
    }

    fn context_controls_width(&self, spacing: f32) -> f32 {
        let count = if self.view != AppView::Review && self.context_has_refocus() { 3.0 } else { 2.0 };
        count * 44.0 + (count - 1.0) * spacing
    }

    fn context_summary_text(&self, ctx: &egui::Context, layout: LayoutMode, available: f32) -> WorkspaceSummaryText {
        let spacing = ctx.global_style().spacing.item_spacing.x;
        let width = if layout == LayoutMode::Compact {
            available
        } else {
            (available - 2.0 * (44.0 + spacing) - self.context_controls_width(spacing) - spacing).min(380.0)
        };
        WorkspaceSummaryText::measure(ctx, &self.displayed_workspace_summary(), width, self.bar_availability_loading())
    }

    pub(crate) fn workspace_summary_height(&self, ctx: &egui::Context, layout: LayoutMode, viewport_width: f32) -> f32 {
        let text = self.context_summary_text(ctx, layout, viewport_width - 28.0);
        if layout == LayoutMode::Compact { text.height + 46.0 } else { text.height.max(44.0) + 14.0 }
    }

    fn shared_context_bar(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        let content = self.displayed_workspace_summary();
        let text = self.context_summary_text(ui.ctx(), layout, ui.available_width());
        let valid = content.identity_and_type.is_some();
        if !valid || self.work.drawer == Some(Drawer::Workflow) {
            self.work.review_details_focus_return = None;
        }
        let compact = layout == LayoutMode::Compact;
        let height = if compact { text.height + 46.0 } else { text.height.max(44.0) };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        let spacing = ui.spacing().item_spacing.x;
        let controls_width = self.context_controls_width(spacing);
        let row_y = if compact { rect.bottom() - 22.0 } else { rect.center().y };
        let group_width = if compact { controls_width } else { text.width + spacing + controls_width };
        let group_left = rect.center().x - group_width * 0.5;
        let summary_rect = egui::Rect::from_min_size(
            egui::pos2(if compact { rect.center().x - text.width * 0.5 } else { group_left },
                if compact { rect.top() } else { row_y - text.height * 0.5 }),
            egui::vec2(text.width, text.height));
        let controls_rect = egui::Rect::from_min_size(
            egui::pos2(if compact { group_left } else { group_left + text.width + spacing }, row_y - 22.0),
            egui::vec2(controls_width, 44.0));
        let left = egui::Rect::from_min_size(egui::pos2(rect.left(), row_y - 22.0), egui::vec2(44.0, 44.0));
        let right = egui::Rect::from_min_size(egui::pos2(rect.right() - 44.0, row_y - 22.0), egui::vec2(44.0, 44.0));
        // Stable child IDs preserve keyboard focus and retained loading geometry.
        ui.scope_builder(egui::UiBuilder::new().id_salt("workflow-toggle").max_rect(left), |ui| {
            if layout == LayoutMode::Wide { self.workflow_panel_toggle(ui); }
            else { self.drawer_panel_button(ui, Drawer::Workflow, crate::glossary::WORKFLOW, false, true); }
        });
        ui.scope_builder(egui::UiBuilder::new().id_salt("workspace-summary").max_rect(summary_rect), |ui| {
            self.workspace_summary(ui, &content, &text);
        });
        ui.scope_builder(egui::UiBuilder::new().id_salt("canvas-controls").max_rect(controls_rect), |ui| {
            ui.add_enabled_ui(valid, |ui| self.canvas_controls(ui));
        });
        ui.scope_builder(egui::UiBuilder::new().id_salt("inspector-toggle").max_rect(right), |ui| {
            if layout == LayoutMode::Wide { self.inspector_panel_toggle(ui); }
            else { self.drawer_panel_button(ui, Drawer::Inspector, crate::glossary::INSPECTOR, true, true); }
        });
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Workspace context bar"));
    }

    fn workspace_summary(
        &self, ui: &mut egui::Ui, content: &WorkspaceSummary, text: &WorkspaceSummaryText,
    ) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(text.width, text.height), egui::Sense::hover());
        let avatar_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y), egui::Vec2::splat(24.0));
        if content.show_avatar && content.identity_and_type.is_some() && !content.accessible.is_empty() {
            let (name, github_id) = content.submitter.as_ref()
                .map(|(name, id)| (name.as_str(), id.as_deref()))
                .unwrap_or(("?", None));
            crate::avatar::paint(ui, github_id, name, avatar_rect);
        }
        let mut pos = rect.min + egui::vec2(if content.show_avatar { 36.0 } else { 0.0 }, 0.0);
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
