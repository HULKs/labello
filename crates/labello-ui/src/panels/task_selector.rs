#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowActivity {
    Boxes,
    Migration,
    MissingObjects,
    Skeleton,
}

impl WorkflowActivity {
    fn label(self) -> &'static str {
        match self {
            Self::Boxes => "Bounding box annotation",
            Self::Migration => "Migration",
            Self::MissingObjects => "Add missing objects",
            Self::Skeleton => "Skeleton annotation",
        }
    }
}

impl LabelloApp {
    const WORKFLOW_ICON_SIZE: f32 = 28.0;
    const WORKFLOW_PILL_HEIGHT: f32 = 52.0;
    const WORKFLOW_MARKER_WIDTH: f32 = 20.0;

    pub(crate) fn workflow_panel_width(&self, ctx: &egui::Context) -> f32 {
        if self.view == AppView::Annotate {
            return 340.0;
        }
        let workflows = self.workflow_choices();
        let style = ctx.style_of(ctx.theme());
        let label_font = egui::TextStyle::Button.resolve(&style);
        let widest_content = ctx.fonts_mut(|fonts| {
            workflows
                .iter()
                .map(|workflow| {
                    let label_width = fonts
                        .layout_no_wrap(workflow.label(), label_font.clone(), theme::TEXT)
                        .size()
                        .x;
                    Self::WORKFLOW_MARKER_WIDTH
                        + theme::SPACE_2
                        + Self::WORKFLOW_ICON_SIZE
                        + theme::SPACE_2
                        + label_width
                        + 2.0 * theme::SPACE_3
                })
                .fold(0.0, f32::max)
        });

        let measured_width = widest_content + 2.0 * theme::SPACE_4 + 2.0;
        if workflows.is_empty() {
            measured_width.max(LayoutMode::TASK_PANEL_WIDTH)
        } else {
            measured_width
        }
    }

    pub(crate) fn workflow_queue_status(&self) -> Option<String> {
        (matches!(self.view, AppView::Annotate | AppView::Review)
            && self.work.assignment.is_some())
        .then(|| {
            let status = format!(
                "Loaded assignment queue: {}/{}",
                self.work.queue.len(),
                self.work.queue.queue_size()
            );
            let suffix = match self.work.queue.wait_reason() {
                Some(crate::queue::QueueWaitReason::ImbalanceLimit) => " (imbalance limit)",
                Some(crate::queue::QueueWaitReason::NoAvailableWork) => " (no available work)",
                Some(crate::queue::QueueWaitReason::Failed) => " (refill failed; retrying)",
                None => "",
            };
            format!("{status}{suffix}")
        })
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
        if self.view == AppView::Annotate {
            self.annotation_workflow_groups(ui, &workflows);
        } else {
            for workflow in &workflows {
                ui.push_id(&workflow.task_id, |ui| {
                    self.workflow_entry(ui, workflow, None)
                });
            }
        }
        if let Some(error) = self.work.availability.error.clone() {
            theme::inline_message(
                ui,
                theme::Intent::Warning,
                "Assignment availability could not be checked. Workflows remain selectable.",
            );
            ui.small(error);
            if ui.button("Retry availability").clicked() {
                self.work.availability.last_attempt = None;
                self.request_assignment_availability();
            }
        }
    }

    pub(crate) fn workflow_entry_label(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) -> String {
        let Some(activity) = activity else {
            return workflow.label();
        };
        let class = self
            .work
            .tasks
            .iter()
            .find(|task| task.task_id == workflow.task_id)
            .and_then(|task| task.class_ids.first())
            .map(|id| self.class_name(id))
            .unwrap_or_default();
        format!("{class}: {} · {}", activity.label(), workflow.label())
    }

    fn workflow_activity_block(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) -> Option<&'static str> {
        match activity {
            Some(WorkflowActivity::MissingObjects) => {
                if self.work.selected_task_id.as_ref() != Some(&workflow.task_id)
                    || !self.manual_migration_active()
                {
                    Some("Open Migration for this workflow and resolve its bounding boxes first.")
                } else if !matches!(
                    self.work.migration.cursor,
                    Some(labello_domain::MigrationCursor::FullImage)
                ) {
                    Some("Resolve the remaining bounding boxes before adding missing objects.")
                } else if self.work.migration.inspected_group_id.is_some() {
                    Some("Return to full-image confirmation before adding missing objects.")
                } else {
                    None
                }
            }
            Some(WorkflowActivity::Migration)
                if self.work.selected_task_id.as_ref() == Some(&workflow.task_id)
                    && self.manual_migration_active()
                    && matches!(
                        self.work.migration.cursor,
                        Some(labello_domain::MigrationCursor::FullImage)
                    )
                    && self.work.current_state.as_ref().is_none_or(|state| {
                        state
                            .migration_target_sets
                            .get(&workflow.task_id)
                            .is_none_or(|set| set.targets.is_empty())
                    }) =>
            {
                Some("This image has no bounding boxes to migrate. Add missing objects instead.")
            }
            _ => None,
        }
    }

    fn workflow_primary_activity(&self, workflow: &crate::app::WorkflowChoice) -> WorkflowActivity {
        if workflow.annotation_type == AnnotationType::BoundingBox {
            return WorkflowActivity::Boxes;
        }
        let configured = self
            .work
            .tasks
            .iter()
            .find(|task| task.task_id == workflow.task_id)
            .is_some_and(|task| task.manual_box_guide_migration.is_some());
        let direct_assignment = self.work.selected_task_id.as_ref() == Some(&workflow.task_id)
            && self.work.current_state.is_some()
            && !self.manual_migration_active();
        if configured && !direct_assignment {
            WorkflowActivity::Migration
        } else {
            WorkflowActivity::Skeleton
        }
    }

    fn annotation_workflow_groups(
        &mut self,
        ui: &mut egui::Ui,
        workflows: &[crate::app::WorkflowChoice],
    ) {
        let mut groups: Vec<(labello_domain::ClassId, Vec<crate::app::WorkflowChoice>)> =
            Vec::new();
        for workflow in workflows {
            let Some(class_id) = self
                .work
                .tasks
                .iter()
                .find(|task| task.task_id == workflow.task_id)
                .and_then(|task| task.class_ids.first())
                .cloned()
            else {
                continue;
            };
            if let Some((_, entries)) = groups.iter_mut().find(|(id, _)| id == &class_id) {
                entries.push(workflow.clone());
            } else {
                groups.push((class_id, vec![workflow.clone()]));
            }
        }
        for (class_id, entries) in groups {
            ui.push_id(&class_id, |ui| {
                let frame = theme::inset_frame();
                let width = (ui.available_width() - frame.total_margin().sum().x).max(1.0);
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    let heading = ui.add(
                        egui::Label::new(RichText::new(self.class_name(&class_id)).heading())
                            .wrap(),
                    );
                    ui.ctx().accesskit_node_builder(heading.id, |node| {
                        node.set_role(egui::accesskit::Role::Heading);
                        node.set_label(self.class_name(&class_id));
                    });
                    for activity in [
                        WorkflowActivity::Boxes,
                        WorkflowActivity::Migration,
                        WorkflowActivity::MissingObjects,
                        WorkflowActivity::Skeleton,
                    ] {
                        let matching: Vec<_> = entries
                            .iter()
                            .filter(|entry| {
                                let primary = self.workflow_primary_activity(entry);
                                primary == activity
                                    || (activity == WorkflowActivity::MissingObjects
                                        && primary == WorkflowActivity::Migration)
                            })
                            .collect();
                        if matching.is_empty() && activity != WorkflowActivity::Skeleton {
                            self.unconfigured_workflow_entry(ui, &class_id, activity);
                        }
                        for workflow in matching {
                            ui.push_id((&workflow.task_id, activity.label()), |ui| {
                                self.workflow_entry(ui, workflow, Some(activity))
                            });
                        }
                    }
                });
            });
            ui.add_space(theme::SPACE_2);
        }
    }

    fn unconfigured_workflow_entry(
        &self,
        ui: &mut egui::Ui,
        class_id: &labello_domain::ClassId,
        activity: WorkflowActivity,
    ) {
        let reason = match activity {
            WorkflowActivity::Boxes => "Bounding box annotation is not configured for this class.",
            _ if self.work.tasks.iter().any(|task| {
                task.class_ids.first() == Some(class_id)
                    && task.manual_box_guide_migration.is_some()
            }) =>
            {
                "Migration is not active on this image. Use skeleton annotation."
            }
            _ => "Migration is not configured for this class.",
        };
        let response = ui.add_enabled(
            false,
            egui::Button::new(activity.label())
                .min_size(egui::vec2(ui.available_width(), 44.0))
                .truncate(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                false,
                format!("{}: {}", self.class_name(class_id), activity.label()),
            )
        });
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.set_description(reason));
        response.on_disabled_hover_text(reason);
    }

    fn workflow_entry(
        &mut self,
        ui: &mut egui::Ui,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) {
        let task_selected = self.work.selected_task_id.as_ref() == Some(&workflow.task_id);
        let overview = task_selected
            && self.manual_migration_active()
            && matches!(
                self.work.migration.cursor,
                Some(labello_domain::MigrationCursor::FullImage)
            );
        let selected = task_selected
            && match activity {
                Some(WorkflowActivity::MissingObjects) => overview,
                Some(WorkflowActivity::Migration) => !overview,
                _ => true,
            };
        let blocked = self.workflow_activity_block(workflow, activity);
        let ready = !self.saving_blocks_interaction()
            && !self.loading.image
            && !self.work.migration.busy
            && self.work.pending_transition.is_none();
        let label = self.workflow_entry_label(workflow, activity);
        let text: egui::WidgetText = if let Some(activity) = activity {
            let width = (ui.available_width()
                - 2.0 * ui.spacing().button_padding.x
                - Self::WORKFLOW_MARKER_WIDTH
                - Self::WORKFLOW_ICON_SIZE
                - 2.0 * theme::SPACE_2)
                .max(1.0);
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = width;
            job.wrap.max_rows = 3;
            job.append(
                activity.label(),
                0.0,
                egui::TextFormat {
                    font_id: egui::TextStyle::Button.resolve(ui.style()),
                    color: egui::Color32::PLACEHOLDER,
                    ..Default::default()
                },
            );
            job.append(
                &format!("\n{}", workflow.label()),
                0.0,
                egui::TextFormat {
                    font_id: egui::TextStyle::Small.resolve(ui.style()),
                    color: egui::Color32::PLACEHOLDER,
                    ..Default::default()
                },
            );
            ui.fonts_mut(|fonts| fonts.layout_job(job)).into()
        } else {
            RichText::new(workflow.label()).strong().into()
        };
        // These controls operate on the current assignment, not a new queue claim.
        let current_migration_action = overview
            && matches!(
                activity,
                Some(WorkflowActivity::Migration | WorkflowActivity::MissingObjects)
            );
        let reason = self
            .workflow_marker_reason(&workflow.task_id)
            .filter(|reason| {
                !current_migration_action || !matches!(reason, WorkflowMarkerReason::Unavailable(_))
            });
        let unavailable = !current_migration_action
            && self.displayed_workflow_availability(&workflow.task_id) == Some(false);
        let icon_id = ui.id().with(("workflow-type", &workflow.task_id));
        let marker_id = ui.id().with(("workflow-selection", &workflow.task_id));
        let button = egui::Button::new((
            egui::Atom::custom(
                marker_id,
                egui::vec2(Self::WORKFLOW_MARKER_WIDTH, Self::WORKFLOW_ICON_SIZE),
            ),
            egui::Atom::custom(
                icon_id,
                egui::vec2(Self::WORKFLOW_ICON_SIZE, Self::WORKFLOW_ICON_SIZE),
            ),
            text,
        ))
        .selected(selected)
        .frame(true)
        .frame_when_inactive(true)
        .corner_radius(theme::SURFACE_RADIUS)
        .min_size(egui::vec2(
            ui.available_width(),
            if activity.is_some() {
                64.0
            } else {
                Self::WORKFLOW_PILL_HEIGHT
            },
        ))
        .gap(theme::SPACE_2)
        .truncate();
        let choice = ui
            .add_enabled_ui(ready && !unavailable && blocked.is_none(), |ui| {
                button.atom_ui(ui)
            })
            .inner;
        choice.response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                ready && !unavailable && blocked.is_none(),
                selected,
                label.clone(),
            )
        });
        let response_id = choice.response.id;
        let queue_status = selected.then(|| self.workflow_queue_status()).flatten();
        let mut accessibility_description = match (reason, queue_status.as_ref()) {
            (Some(reason), Some(queue)) => Some(format!("{}. {queue}", reason.label())),
            (Some(reason), None) => Some(reason.label().to_owned()),
            (None, _) => queue_status.clone(),
        };
        if let Some(blocked) = blocked {
            accessibility_description = Some(blocked.to_owned());
        }
        if let Some(description) = accessibility_description {
            ui.ctx().accesskit_node_builder(response_id, |node| {
                node.set_description(description);
            });
        }
        if let Some(icon_rect) = choice.rect(icon_id) {
            workflow_type_icon(ui, icon_id, icon_rect, &workflow.annotation_type);
        }
        if let Some(marker_rect) = choice.rect(marker_id) {
            paint_workflow_marker(ui, marker_rect, selected, reason);
        }
        let mut hover_text = format!(
            "{} workflow\nPrevious: {} · Next: {}",
            annotation_type_label(&workflow.annotation_type),
            self.shortcut_text(ui.ctx(), labello_domain::UserAction::SelectPreviousWorkflow,),
            self.shortcut_text(ui.ctx(), labello_domain::UserAction::SelectNextWorkflow,)
        );
        if let Some(reason) = reason {
            hover_text = reason.label().to_owned();
        }
        if let Some(queue_status) = queue_status.as_ref() {
            hover_text.push('\n');
            hover_text.push_str(queue_status);
        }
        if let Some(blocked) = blocked {
            hover_text = blocked.to_owned();
        }
        hover_text = format!("{label}\n{hover_text}");
        let show_hover = |ui: &mut egui::Ui| {
            let width = (ui.ctx().content_rect().width() - 2.0 * theme::SPACE_4)
                .max(1.0)
                .min(ui.spacing().tooltip_width);
            ui.set_max_width(width);
            ui.label(&hover_text);
        };
        let response = choice
            .response
            .on_hover_ui(show_hover)
            .on_disabled_hover_ui(show_hover);
        if response.gained_focus() {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.clicked() {
            match activity {
                Some(WorkflowActivity::MissingObjects)
                    if !self.work.migration.adding_missing_object =>
                {
                    self.trigger_missing_migration_object_action();
                }
                Some(WorkflowActivity::Migration) if overview => {
                    self.revisit_first_migration_object();
                }
                _ if !task_selected => {
                    self.request_transition(PendingTransition::Workflow(workflow.task_id.clone()))
                }
                _ => {}
            }
        }
        if self.view == AppView::Annotate
            && activity != Some(WorkflowActivity::MissingObjects)
            && self.datasets.stats_error.is_none()
            && let Some(focus) = &self.datasets.stats.scoring_focus
            && focus.task_id.as_ref() == Some(&workflow.task_id)
            && focus.contains(labello_domain::now())
        {
            let minutes =
                ((focus.ends_at - labello_domain::now()).num_seconds().max(0) as u64).div_ceil(60);
            ui.label(
                RichText::new(format!("Focus · +25% · {minutes} min left")).color(theme::ACCENT),
            );
        }
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
