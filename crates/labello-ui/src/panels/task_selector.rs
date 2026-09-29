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
            Self::Boxes => crate::glossary::BOUNDING_BOX_ANNOTATION,
            Self::Migration => crate::glossary::MIGRATION,
            Self::MissingObjects => crate::glossary::ADD_MISSING_OBJECTS,
            Self::Skeleton => crate::glossary::SKELETON_ANNOTATION,
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::Boxes => crate::glossary::BOXES,
            Self::Migration => crate::glossary::MIGRATE,
            Self::MissingObjects => crate::glossary::MISSING_OBJECTS,
            Self::Skeleton => crate::glossary::SKELETON,
        }
    }
}

struct WorkflowTileLayout {
    galley: std::sync::Arc<egui::Galley>,
    label_row_height: f32,
    button_height: f32,
}

impl LabelloApp {
    const WORKFLOW_ICON_SIZE: f32 = 28.0;
    const WORKFLOW_PILL_HEIGHT: f32 = 52.0;
    const WORKFLOW_MARKER_WIDTH: f32 = 20.0;

    pub(crate) fn workflow_panel_width(&self, _ctx: &egui::Context) -> f32 {
        340.0
    }

    pub(crate) fn workflow_queue_status(&self) -> Option<String> {
        (matches!(self.view, AppView::Annotate | AppView::Review) && self.work.assignment.is_some())
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
        if self.work.availability.error.is_some() && ui.button("Retry availability").clicked() {
            self.work.availability.last_attempt = None;
            self.request_assignment_availability();
        }
        self.class_workflow_groups(ui, &workflows);
    }

    pub(crate) fn workflow_entry_label(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) -> String {
        let activity = activity.or_else(|| (self.view == AppView::Review).then(|| self.workflow_primary_activity(workflow)));
        let Some(activity) = activity else { return workflow.label(); };
        format!(
            "{} · {}",
            self.workflow_activity_label(workflow, activity),
            workflow.label()
        )
    }

    fn workflow_activity_label(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: WorkflowActivity,
    ) -> String {
        let class = self
            .work
            .tasks
            .iter()
            .find(|task| task.task_id == workflow.task_id)
            .and_then(|task| task.class_ids.first())
            .map(|id| self.class_name(id))
            .unwrap_or_default();
        let action = if self.view == AppView::Review {
            match activity {
                WorkflowActivity::Boxes => crate::glossary::BOUNDING_BOX_REVIEW,
                _ => crate::glossary::SKELETON_REVIEW,
            }
        } else { activity.label() };
        format!("{class}: {action}")
    }

    fn workflow_activity_block(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) -> Option<WorkflowMarkerReason> {
        if self.view == AppView::Review { return None; }
        match activity {
            Some(WorkflowActivity::MissingObjects) => {
                if self.work.selected_task_id.as_ref() != Some(&workflow.task_id)
                    || !self.manual_migration_active()
                {
                    Some(WorkflowMarkerReason::MigrationRequired)
                } else if !matches!(
                    self.work.migration.cursor,
                    Some(labello_domain::MigrationCursor::FullImage)
                ) {
                    Some(WorkflowMarkerReason::UnresolvedBoxes)
                } else if self.work.migration.inspected_group_id.is_some() {
                    Some(WorkflowMarkerReason::FullImageRequired)
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
                Some(WorkflowMarkerReason::NoMigrationBoxes)
            }
            _ => None,
        }
    }

    fn workflow_boost_window(&self, task_id: &labello_domain::TaskId) -> Option<i64> {
        if self.view != AppView::Annotate || self.datasets.stats_error.is_some() { return None; }
        self.datasets.stats.scoring_focus.as_ref()
            .filter(|focus| focus.task_id.as_ref() == Some(task_id) && focus.contains(labello_domain::now()))
            .map(|focus| focus.starts_at.timestamp_millis())
    }

    fn workflow_selection_block(
        &self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) -> Option<WorkflowMarkerReason> {
        self.workflow_interaction_block()
            .or_else(|| self.workflow_activity_block(workflow, activity))
            .or_else(|| {
                let current_action = matches!(activity, Some(WorkflowActivity::Migration | WorkflowActivity::MissingObjects))
                    && self.work.selected_task_id.as_ref() == Some(&workflow.task_id)
                    && self.manual_migration_active()
                    && matches!(self.work.migration.cursor, Some(labello_domain::MigrationCursor::FullImage));
                (!current_action && self.displayed_workflow_availability(&workflow.task_id) == Some(false)).then(|| {
                    WorkflowMarkerReason::Unavailable(self.work.availability.reasons.get(&workflow.task_id).copied().unwrap_or(labello_domain::WorkflowUnavailableReason::Unavailable))
                })
            })
    }

    fn workflow_primary_activity(&self, workflow: &crate::app::WorkflowChoice) -> WorkflowActivity {
        if workflow.annotation_type == AnnotationType::BoundingBox {
            return WorkflowActivity::Boxes;
        }
        if self.view == AppView::Review { return WorkflowActivity::Skeleton; }
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

    fn class_workflow_groups(
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
                    let heading = ui.add_sized(
                        [width, 0.0],
                        egui::Label::new(RichText::new(self.class_name(&class_id)).heading())
                            .halign(egui::Align::Center)
                            .wrap(),
                    );
                    ui.ctx().accesskit_node_builder(heading.id, |node| {
                        node.set_role(egui::accesskit::Role::Heading);
                        node.set_label(self.class_name(&class_id));
                    });
                    let activities: Vec<_> = [
                        WorkflowActivity::Boxes,
                        WorkflowActivity::Migration,
                        WorkflowActivity::MissingObjects,
                        WorkflowActivity::Skeleton,
                    ]
                    .into_iter()
                    .filter_map(|activity| {
                        let matching: Vec<_> = entries
                            .iter()
                            .filter(|entry| {
                                let primary = self.workflow_primary_activity(entry);
                                primary == activity
                                    || (self.view == AppView::Annotate && activity == WorkflowActivity::MissingObjects
                                        && primary == WorkflowActivity::Migration)
                            })
                            .collect();
                        (!matching.is_empty()).then_some((activity, matching))
                    })
                    .collect();
                    ui.spacing_mut().item_spacing.x = theme::SPACE_1;
                    ui.columns(activities.len(), |columns| {
                        let galleys: Vec<_> = columns
                            .iter_mut()
                            .zip(&activities)
                            .map(|(column, (activity, _))| {
                                column.spacing_mut().button_padding =
                                    egui::vec2(theme::SPACE_1, theme::SPACE_1);
                                Self::workflow_tile_galley(column, *activity)
                            })
                            .collect();
                        let label_row_height = galleys
                            .iter()
                            .map(|galley| galley.size().y)
                            .fold(0.0, f32::max);
                        let button_height = 2.0 * theme::SPACE_1
                            + theme::SPACE_2
                            + Self::WORKFLOW_ICON_SIZE
                            + theme::SPACE_1
                            + label_row_height
                            + theme::SPACE_1
                            + 18.0
                            + theme::SPACE_1;
                        for ((column, (activity, matching)), galley) in
                            columns.iter_mut().zip(activities).zip(galleys)
                        {
                            column.spacing_mut().button_padding =
                                egui::vec2(theme::SPACE_1, theme::SPACE_1);
                            let workflow = matching
                                .iter()
                                .find(|entry| {
                                    self.work.selected_task_id.as_ref() == Some(&entry.task_id)
                                })
                                .unwrap_or(&matching[0]);
                            column.push_id(activity.label(), |ui| {
                                let layout = WorkflowTileLayout {
                                    galley,
                                    label_row_height,
                                    button_height,
                                };
                                self.workflow_entry(
                                    ui,
                                    workflow,
                                    Some(activity),
                                    &matching,
                                    Some(&layout),
                                )
                            });
                        }
                    });

                });
            });
        }
    }

    fn workflow_tile_galley(
        ui: &mut egui::Ui,
        activity: WorkflowActivity,
    ) -> std::sync::Arc<egui::Galley> {
        let width = (ui.available_width() - 2.0 * ui.spacing().button_padding.x).max(1.0);
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = width;
        job.wrap.max_rows = 3;
        job.halign = egui::Align::Center;
        job.append(
            activity.short_label(),
            0.0,
            egui::TextFormat {
                font_id: if ui.available_width() < 75.0 {
                    egui::TextStyle::Small
                } else {
                    egui::TextStyle::Button
                }
                .resolve(ui.style()),
                color: egui::Color32::PLACEHOLDER,
                ..Default::default()
            },
        );
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    }

    fn workflow_entry(
        &mut self,
        ui: &mut egui::Ui,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
        choices: &[&crate::app::WorkflowChoice],
        tile_layout: Option<&WorkflowTileLayout>,
    ) {
        let multiple = choices.len() > 1;
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
        let blocked = self.workflow_selection_block(workflow, activity);
        let ready = self.workflow_interaction_block().is_none();
        let mut label = self.workflow_entry_label(workflow, activity);
        if multiple && let Some(activity) = activity {
            label = format!(
                "{} · Choose workflow",
                self.workflow_activity_label(workflow, activity)
            );
        }
        let text: egui::WidgetText = RichText::new(workflow.label()).strong().into();
        // These controls operate on the current assignment, not a new queue claim.
        let current_migration_action = overview
            && matches!(
                activity,
                Some(WorkflowActivity::Migration | WorkflowActivity::MissingObjects)
            );
        let reason = if multiple { self.workflow_interaction_block() } else { blocked }
            .or_else(|| self.workflow_marker_reason(&workflow.task_id).filter(|reason| {
                !matches!(reason, WorkflowMarkerReason::Unavailable(_))
                    || ((!multiple || task_selected) && !current_migration_action)
            }));
        let icon_id = ui.id().with(("workflow-type", &workflow.task_id));
        let marker_id = ui.id().with(("workflow-selection", &workflow.task_id));
        let content_id = ui.id().with("workflow-content");
        let button = if let Some(layout) = tile_layout {
            egui::Button::new(egui::Atom::custom(
                content_id,
                egui::vec2(
                    (ui.available_width() - 2.0 * ui.spacing().button_padding.x).max(1.0),
                    layout.button_height - 2.0 * ui.spacing().button_padding.y,
                ),
            ))
        } else {
            egui::Button::new((
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
        }
        .selected(selected)
        .frame(true)
        .frame_when_inactive(true)
        .corner_radius(theme::SURFACE_RADIUS)
        .min_size(egui::vec2(
            ui.available_width(),
            tile_layout.map_or(Self::WORKFLOW_PILL_HEIGHT, |layout| layout.button_height),
        ))
        .gap(theme::SPACE_2)
        .truncate();
        let choice = ui
            .add_enabled_ui(
                ready && (multiple || blocked.is_none()),
                |ui| button.atom_ui(ui),
            )
            .inner;
        choice.response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                choice.response.enabled(),
                selected,
                label.clone(),
            )
        });
        let response_id = choice.response.id;
        let queue_status = selected.then(|| self.workflow_queue_status()).flatten();
        let boosted = choices.iter().find_map(|choice| self.workflow_boost_window(&choice.task_id)
            .map(|window| (window, choice.label())));
        let boost_description = boosted.as_ref().map(|(_, name)| if multiple {
            format!("{}: {name}", crate::glossary::BOOSTED_WORKFLOW)
        } else { crate::glossary::BOOSTED_WORKFLOW.to_owned() });
        let mut accessibility_description = match (reason, queue_status.as_ref()) {
            (Some(reason), Some(queue)) => Some(format!("{}. {queue}", reason.label())),
            (Some(reason), None) => Some(reason.label().to_owned()),
            (None, _) => queue_status.clone(),
        };
        if multiple {
            let count = format!("{} workflows", choices.len());
            accessibility_description = Some(accessibility_description.map_or_else(
                || count.clone(), |reason| format!("{reason}. {count}")));
        }
        if let Some(boost) = &boost_description {
            accessibility_description = Some(accessibility_description.map_or_else(|| boost.clone(), |description| format!("{description}. {boost}")));
        }
        if let Some(description) = accessibility_description {
            ui.ctx().accesskit_node_builder(response_id, |node| {
                node.set_description(description);
            });
        }
        if let (Some(rect), Some(layout)) = (choice.rect(content_id), tile_layout) {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(
                    rect.center().x,
                    rect.top() + theme::SPACE_2 + Self::WORKFLOW_ICON_SIZE / 2.0,
                ),
                egui::vec2(Self::WORKFLOW_ICON_SIZE, Self::WORKFLOW_ICON_SIZE),
            );
            workflow_type_icon(ui, icon_id, icon_rect, &workflow.annotation_type);
            if let Some(activity) = activity {
                paint_workflow_activity_badge(ui, icon_rect, activity);
            }
            let label_top = icon_rect.bottom() + theme::SPACE_1;
            let status_y = label_top + layout.label_row_height + theme::SPACE_1 + 9.0;
            let cue_count =
                usize::from(selected) + usize::from(reason.is_some()) + usize::from(multiple);
            let cue_width = if cue_count == 0 {
                0.0
            } else {
                (if selected { 8.0 } else { 0.0 })
                    + (if reason.is_some() { 18.0 } else { 0.0 })
                    + (if multiple { 8.0 } else { 0.0 })
                    + (cue_count - 1) as f32 * theme::SPACE_1
            };
            let mut cue_x = rect.center().x - cue_width / 2.0;
            if selected {
                ui.painter()
                    .circle_filled(egui::pos2(cue_x + 4.0, status_y), 4.0, theme::TEXT);
                cue_x += 8.0 + theme::SPACE_1;
            }
            if reason.is_some() {
                paint_workflow_marker(
                    ui,
                    egui::Rect::from_center_size(
                        egui::pos2(cue_x + 9.0, status_y),
                        egui::vec2(18.0, 18.0),
                    ),
                    false,
                    reason,
                    if selected {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    },
                );
                cue_x += 18.0 + theme::SPACE_1;
            }
            if multiple {
                let center = egui::pos2(cue_x + 4.0, status_y);
                let color = ui.style().interact(&choice.response).fg_stroke.color;
                let color = if choice.response.enabled() {
                    color
                } else {
                    ui.visuals().disable(color)
                };
                let stroke = egui::Stroke::new(1.5, color);
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-3.0, -1.0),
                        center + egui::vec2(0.0, 2.0),
                    ],
                    stroke,
                );
                ui.painter().line_segment(
                    [
                        center + egui::vec2(0.0, 2.0),
                        center + egui::vec2(3.0, -1.0),
                    ],
                    stroke,
                );
            }
            let color = ui.style().interact(&choice.response).fg_stroke.color;
            ui.painter().galley(
                egui::pos2(rect.center().x, label_top),
                layout.galley.clone(),
                if choice.response.enabled() {
                    color
                } else {
                    ui.visuals().disable(color)
                },
            );
        } else {
            if let Some(icon_rect) = choice.rect(icon_id) {
                workflow_type_icon(ui, icon_id, icon_rect, &workflow.annotation_type);
            }
            if let Some(marker_rect) = choice.rect(marker_id) {
                paint_workflow_marker(ui, marker_rect, selected, reason, theme::TEXT_MUTED);
            }
        }
        paint_workflow_boost(ui, &choice.response, boosted.map(|(window, _)| window), None);
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
        if multiple {
            hover_text = format!("{} workflows{}", choices.len(), reason.map_or_else(String::new, |reason| format!("\n{}", reason.label())));
        }
        if let Some(boost) = boost_description { hover_text.push_str(&format!("\n{boost}")); }
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
        if multiple {
            let popup = egui::Popup::menu(&response)
                .width((ui.ctx().content_rect().width() - 48.0).clamp(160.0, 360.0));
            let was_open = popup.is_open();
            if response.layer_id.order == egui::Order::Foreground {
                // Keep a chooser opened inside a modal drawer above that drawer.
                ui.ctx().set_sublayer(
                    response.layer_id,
                    egui::LayerId::new(egui::Order::Foreground, popup.get_id()),
                );
            }
            popup.show(|ui| {
                ui.set_max_width((ui.ctx().content_rect().width() - 48.0).clamp(160.0, 360.0));
                let height = (ui.ctx().content_rect().height() - 96.0).max(44.0);
                ui.set_max_height(height);
                ui.add(
                    egui::Label::new(
                        RichText::new(label.trim_end_matches(" · Choose workflow")).strong(),
                    )
                    .wrap(),
                );
                ui.small(format!("{} workflows", choices.len()));
                ui.separator();
                egui::ScrollArea::vertical()
                    .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        for choice in choices {
                            let current =
                                self.work.selected_task_id.as_ref() == Some(&choice.task_id);
                            let block = self.workflow_selection_block(choice, activity);
                            let enabled = block.is_none();
                            let marker_id = ui.id().with(("chooser-reason", &choice.task_id));
                            let boost_id = ui.id().with(("chooser-boost", &choice.task_id));
                            let option_atoms = ui.add_enabled_ui(enabled, |ui| {
                                egui::Button::new((
                                    egui::Atom::custom(marker_id, egui::vec2(20.0, 18.0)),
                                    choice.label(),
                                    egui::Atom::custom(boost_id, egui::vec2(16.0, 16.0)),
                                ))
                                .selected(current)
                                .wrap()
                                .min_size(egui::vec2(ui.available_width(), 44.0))
                                .atom_ui(ui)
                            }).inner;
                            if let Some(rect) = option_atoms.rect(marker_id) {
                                paint_workflow_marker(ui, rect, false, block, if current { theme::TEXT } else { theme::TEXT_MUTED });
                            }
                            let boost = self.workflow_boost_window(&choice.task_id);
                            paint_workflow_boost(ui, &option_atoms.response, boost, option_atoms.rect(boost_id).map(|rect| rect.center()));
                            let option = option_atoms.response;
                            option.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Button,
                                    option.enabled(),
                                    current,
                                    self.workflow_entry_label(choice, activity),
                                )
                            });
                            let description = [block.map(|reason| reason.label().to_owned()), boost.map(|_| crate::glossary::BOOSTED_WORKFLOW.to_owned())]
                                .into_iter().flatten().collect::<Vec<_>>().join(". ");
                            let option = if description.is_empty() { option } else {
                                ui.ctx().accesskit_node_builder(option.id, |node| node.set_description(description.as_str()));
                                option.on_hover_text(&description).on_disabled_hover_text(&description)
                            };
                            if option.gained_focus() {
                                option.scroll_to_me(Some(egui::Align::Center));
                            }
                            if option.clicked() {
                                self.activate_workflow_entry(choice, activity);
                                ui.close();
                            }
                        }
                    });
            });
            if was_open
                && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response))
            {
                response.request_focus();
            }
        } else if response.clicked() {
            self.activate_workflow_entry(workflow, activity);
        }
    }

    fn activate_workflow_entry(
        &mut self,
        workflow: &crate::app::WorkflowChoice,
        activity: Option<WorkflowActivity>,
    ) {
        let selected = self.work.selected_task_id.as_ref() == Some(&workflow.task_id);
        let overview = selected
            && self.manual_migration_active()
            && matches!(
                self.work.migration.cursor,
                Some(labello_domain::MigrationCursor::FullImage)
            );
        match activity {
            Some(WorkflowActivity::MissingObjects)
                if self.view == AppView::Annotate && !self.work.migration.adding_missing_object =>
            {
                self.trigger_missing_migration_object_action()
            }
            Some(WorkflowActivity::Migration) if self.view == AppView::Annotate && overview => self.revisit_first_migration_object(),
            _ if !selected => {
                self.request_transition(PendingTransition::Workflow(workflow.task_id.clone()))
            }
            _ => {}
        }
    }
}

fn paint_workflow_activity_badge(ui: &egui::Ui, rect: egui::Rect, activity: WorkflowActivity) {
    if !matches!(
        activity,
        WorkflowActivity::Migration | WorkflowActivity::MissingObjects
    ) {
        return;
    }
    let center = rect.right_bottom() - egui::vec2(2.0, 2.0);
    ui.painter().circle_filled(center, 6.0, theme::SURFACE);
    let stroke = egui::Stroke::new(1.5, theme::TEXT);
    ui.painter().line_segment(
        [center - egui::vec2(3.0, 0.0), center + egui::vec2(3.0, 0.0)],
        stroke,
    );
    if activity == WorkflowActivity::MissingObjects {
        ui.painter().line_segment(
            [center - egui::vec2(0.0, 3.0), center + egui::vec2(0.0, 3.0)],
            stroke,
        );
    } else {
        ui.painter().line_segment(
            [
                center + egui::vec2(1.0, -2.0),
                center + egui::vec2(3.0, 0.0),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [center + egui::vec2(1.0, 2.0), center + egui::vec2(3.0, 0.0)],
            stroke,
        );
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
