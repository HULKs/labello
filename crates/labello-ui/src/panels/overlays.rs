impl LabelloApp {
    fn tutorial_overlay(&mut self, ctx: &egui::Context) {
        if !self.work.show_tutorial || !self.work_view() {
            return;
        }
        let Some((title, text)) = self.selected_task().map(|task| {
            (
                task.instructions.title.clone(),
                task.instructions.example_text.clone(),
            )
        }) else {
            return;
        };
        let screen = ctx.content_rect();
        let layout = LayoutMode::for_width(screen.width());
        let shell_height = 56.0 + self.workspace_context_height(ctx, layout, screen.size());
        let action_height = self.workspace_actions_height(ctx, layout, screen.size());
        let workspace = egui::Rect::from_min_max(
            egui::pos2(screen.left(), screen.top() + shell_height),
            egui::pos2(screen.right(), screen.bottom() - action_height),
        );
        let mut open = true;
        egui::Window::new(crate::glossary::TUTORIAL)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(
                egui::Align2::RIGHT_TOP,
                egui::vec2(-12.0, shell_height + 12.0),
            )
            .max_width((workspace.width() - 24.0).clamp(240.0, 420.0))
            .max_height((workspace.height() - 24.0).clamp(80.0, 560.0))
            .constrain_to(workspace)
            .show(ctx, |ui| {
                ui.heading(title);
                egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx())).show(ui, |ui| ui.label(text));
            });
        if !open {
            self.work.show_tutorial = false;
        }
    }

    fn draft_recovery_modal(&mut self, ctx: &egui::Context) {
        let Some(recovery) = self.runtime.persistence.recovery.clone() else {
            return;
        };
        let (title, timestamp, validation) = match recovery {
            crate::persistence::DraftRecovery::Work(draft, validation) => {
                ("Unsaved assignment draft", draft.updated_at, validation)
            }
            crate::persistence::DraftRecovery::Admin(draft, validation) => {
                ("Unsaved admin draft", draft.updated_at, validation)
            }
        };
        let response = theme::modal(ctx, egui::Id::new("draft-recovery-modal")).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(240.0, 560.0));
                ui.heading(title);
                ui.label(format!(
                    "Saved {}",
                    timestamp.format("%Y-%m-%d %H:%M:%S UTC")
                ));
                match validation {
                    crate::persistence::DraftValidation::Valid => {
                        ui.label(
                            "The server assignment and base event sequence match exactly. Recover or discard this draft.",
                        );
                        ui.horizontal_wrapped(|ui| {
                            if theme::primary_button(
                                ui,
                                true,
                                egui::Button::new("Recover draft"),
                            )
                            .clicked()
                            {
                                self.recover_browser_draft();
                            }
                            if theme::danger_button(
                                ui,
                                true,
                                egui::Button::new("Discard draft"),
                            )
                            .clicked()
                            {
                                self.discard_browser_draft();
                            }
                        });
                    }
                    crate::persistence::DraftValidation::Expired(message)
                    | crate::persistence::DraftValidation::Conflict(message) => {
                        theme::inline_message(ui, theme::Intent::Warning, message);
                        ui.label(
                            "Recovery is disabled so this draft cannot overwrite newer server state.",
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.add_enabled(false, egui::Button::new("Recover draft"));
                            if ui.button("Discard status").clicked() {
                                self.discard_browser_draft();
                            }
                            if ui.button("Keep status").clicked() {
                                self.runtime.persistence.recovery = None;
                            }
                        });
                    }
                }
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Window, true, "Draft recovery dialog")
        });
    }

    fn transition_modal(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.work.pending_transition.clone() else {
            return;
        };
        if self.loading.image && matches!(pending, PendingTransition::PreviousAssignment(_)) {
            return;
        }
        if (self.loading.saving || self.loading.image) && !self.assignment_has_work()
            && (matches!(pending, PendingTransition::View(_) | PendingTransition::About | PendingTransition::Workflow(_))
                || (self.view == AppView::Review && matches!(pending, PendingTransition::PreviousAssignment(_)))) {
            return;
        }
        let current = self
            .selected_workflow()
            .map(|workflow| workflow.label())
            .unwrap_or_else(|| "No workflow".to_string());
        let destination = self.transition_label(&pending);
        let discards_migration_draft =
            self.manual_migration_active() && self.migration_has_unsaved_input();
        let discards_corrections = self.has_review_corrections();
        let discards_missing = self.has_missing_object_draft();
        let discards_review =
            self.review_revision_active() && !self.work.staged_review_decisions.is_empty();
        let discards_edits = matches!(
            pending,
            PendingTransition::NextAssignment | PendingTransition::PreviousAssignment(_)
        ) && self.view == AppView::Annotate
            && (matches!(self.work.save_status, SaveStatus::Dirty | SaveStatus::Retry)
                || discards_migration_draft);
        if pending == PendingTransition::NextAssignment && !discards_edits && !discards_review && !discards_missing && !discards_corrections {
            return;
        }
        let modal_title = if discards_corrections {
            "Discard reviewer correction?"
        } else if discards_missing {
            "Discard missing-object locations?"
        } else if discards_review {
            "Discard staged review decisions?"
        } else if discards_migration_draft {
            "Unsaved migration draft"
        } else if discards_edits {
            "Unsaved annotation changes"
        } else {
            "Switch active assignment?"
        };
        let response =
            theme::modal(ctx, egui::Id::new("assignment-transition-modal")).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(240.0, 560.0));
                let action_rows = if self.view == AppView::Annotate { 3.0 } else { 2.0 };
                let action_height = action_rows * (ui.spacing().interact_size.y + ui.spacing().item_spacing.y);
                egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                    .max_height((ctx.content_rect().height() - 64.0 - action_height).max(48.0))
                    .show(ui, |ui| {
                ui.heading(modal_title);
                ui.label(format!("Current workflow: {current}"));
                ui.label(format!("Pending destination: {destination}"));
                if discards_corrections { ui.label("Leaving discards unsaved corrections. Cancel keeps them on this assignment."); }
                if discards_missing { ui.label("Leaving discards these unsent missing-object locations. Cancel keeps them on this assignment."); }
                if discards_review {
                    ui.label("Leaving discards staged replacement decisions. The previous effective outcome remains unchanged.");
                }
                if discards_edits {
                    theme::inline_message(ui, theme::Intent::Warning, if discards_migration_draft {
                        "Continuing will discard migration keypoints or exclusion input that has not been saved."
                    } else {
                        "Skipping now will discard annotation changes that have not been saved."
                    });
                }
                });
                if self.loading.image && matches!(pending, PendingTransition::PreviousAssignment(_)) {
                    ui.spinner();
                    ui.label("Opening previous assignment...");
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if self.view == AppView::Annotate
                        && !discards_migration_draft
                        && theme::primary_button(
                            ui,
                            !self.loading.saving && !self.loading.image,
                            egui::Button::new("Submit and switch"),
                        )
                        .clicked()
                    {
                        self.submit_pending_transition();
                    }
                    let release = theme::danger_button(
                        ui,
                        !self.loading.saving && !self.loading.image,
                        egui::Button::new(if discards_review {
                            "Discard revision and switch"
                        } else if discards_edits {
                            if discards_migration_draft {
                                "Discard draft and switch"
                            } else {
                                "Discard edits and skip"
                            }
                        } else {
                            "Release and switch"
                        }),
                    );
                    if release.clicked() {
                        self.release_pending_transition();
                    }
                    let cancel = theme::quiet_button(ui, !self.saving_blocks_interaction() && !self.loading.image, egui::Button::new(crate::glossary::CANCEL));
                    if cancel.clicked() {
                        self.cancel_pending_transition();
                    }
                });
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Window,
                true,
                "Assignment transition dialog",
            )
        });
        if response.should_close() {
            self.cancel_pending_transition();
        }
    }

    fn migration_companion_reconciliation_modal(&mut self, ctx: &egui::Context) {
        let mut dismissed = false;
        let response = theme::modal(ctx, egui::Id::new("migration-companion-reconciliation-modal"))
            .show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(240.0, 560.0));
                let height = (ctx.content_rect().height() - 64.0).max(100.0);
                if ctx.content_rect().height() < 480.0 {
                    ui.set_height(height);
                }
                egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx())).max_height(height).show(ui, |ui| {
                    ui.heading("Reconcile companion box?");
                    ui.label("Create or regenerate the box from the saved skeleton. This replaces the current box geometry and reopens its correction and review workflow. Earlier versions and reviews remain in history. Your unsaved skeleton draft is retained.");
                    ui.horizontal_wrapped(|ui| {
                        let regenerate = theme::danger_button(ui, !self.work.migration.busy,
                            egui::Button::new("Regenerate companion box"));
                        if regenerate.has_focus() { regenerate.scroll_to_me(Some(egui::Align::Center)); }
                        if regenerate.clicked()
                            && let Some(annotation_id) = self.work.migration.pending_companion_reconciliation.take()
                        { self.work.migration.companion_focus_return = None; self.request_reconcile_migration_companion(annotation_id); }
                        let cancel = theme::quiet_button(ui, true, egui::Button::new(crate::glossary::CANCEL));
                        if cancel.has_focus() { cancel.scroll_to_me(Some(egui::Align::Center)); }
                        if cancel.clicked() { dismissed = true; }
                    });
                });
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Window, true, "Reconcile companion box")
        });
        if dismissed || response.should_close() {
            self.work.migration.pending_companion_reconciliation = None;
            ctx.request_repaint();
        }
    }
    fn migration_revisit_discard_modal(&mut self, ctx: &egui::Context) {
        let response =
            theme::modal(ctx, egui::Id::new("migration-revisit-discard-modal")).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(240.0, 560.0));
                ui.heading("Discard current migration draft?");
                theme::inline_message(
                    ui,
                    theme::Intent::Warning,
                    if self.work.migration.pending_reload_discard {
                        "Reloading will discard unsaved migration keypoints or exclusion input and load the current server state."
                    } else {
                        "Opening the selected object will discard migration keypoints or exclusion input after activation succeeds."
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    if theme::danger_button(
                        ui,
                        !self.work.migration.busy,
                        egui::Button::new(if self.work.migration.pending_reload_discard {
                            "Discard draft and reload"
                        } else {
                            "Discard draft and edit object"
                        }),
                    )
                    .clicked()
                    {
                        self.confirm_pending_migration_revisit();
                    }
                    if theme::quiet_button(
                        ui,
                        !self.work.migration.busy,
                        egui::Button::new(crate::glossary::CANCEL),
                    )
                    .clicked()
                    {
                        self.cancel_pending_migration_revisit();
                    }
                });
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Window,
                true,
                "Discard current migration draft",
            )
        });
        if response.should_close() {
            self.cancel_pending_migration_revisit();
        }
    }

    fn admin_discard_modal(&mut self, ctx: &egui::Context) {
        let mut discard = false;
        let response = theme::modal(ctx, egui::Id::new("discard-admin-changes")).show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 48.0).clamp(240.0, 480.0));
            ui.heading("Discard staged Admin changes?");
            ui.label("All unsaved configuration and permission edits will be lost.");
            ui.horizontal_wrapped(|ui| {
                if theme::danger_button(ui, true, egui::Button::new(crate::glossary::DISCARD_CHANGES)).clicked() {
                    discard = true;
                }
                if theme::quiet_button(ui, true, egui::Button::new("Keep editing")).clicked() {
                    self.admin.confirm_discard = false;
                }
            });
        });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Window,
                true,
                "Discard staged Admin changes",
            )
        });
        if discard {
            self.admin.prelabels.model_checks.clear();
            self.datasets.admin_config = self.datasets.admin_baseline.clone();
            self.datasets.users = self.datasets.users_baseline.clone();
            self.clear_admin_draft();
            self.admin.confirm_discard = false;
            self.runtime.notice = Some("Staged admin changes discarded".to_string());
        } else if response.should_close() {
            self.admin.confirm_discard = false;
        }
    }

    fn transition_label(&self, transition: &PendingTransition) -> String {
        match transition {
            PendingTransition::About => "Setup > About".to_string(),
            PendingTransition::Dataset(_, _) => "Another dataset".to_string(),
            PendingTransition::Logout => crate::glossary::SIGN_OUT.to_string(),
            PendingTransition::NextAssignment => "Next assignment".to_string(),
            PendingTransition::PreviousAssignment(_) => "Previous assignment".to_string(),
            PendingTransition::Workflow(task_id) => self
                .workflow_choices()
                .into_iter()
                .find(|workflow| workflow.task_id == *task_id)
                .map(|workflow| workflow.label())
                .unwrap_or_else(|| task_id.to_string()),
            PendingTransition::View(view) => view_label(*view).to_string(),
        }
    }

    fn settings_modal(&mut self, ctx: &egui::Context) {
        if !self.work.show_settings {
            return;
        }
        if self.work.shortcut_settings.confirm_discard {
            self.shortcut_discard_modal(ctx);
            return;
        }
        if self.work.shortcut_settings.draft.is_none() {
            let mut draft = self.work.keybindings.clone();
            draft.normalize();
            self.work.shortcut_settings.baseline = Some(draft.clone());
            self.work.shortcut_settings.draft = Some(draft);
        }
        if !self.loading.keybindings && self.work.shortcut_settings.recording_pan_drag {
            let (escape, modifiers) = ctx.input_mut(|input| {
                let escape = input
                    .events
                    .iter()
                    .rposition(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: egui::Key::Escape,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    })
                    .map(|index| {
                        input.events.remove(index);
                        true
                    })
                    .unwrap_or(false);
                (escape, input.modifiers)
            });
            if escape {
                self.work.shortcut_settings.recording_pan_drag = false;
            } else if let Some(modifier) = pan_drag_modifier_from_input(modifiers) {
                if let Some(draft) = self.work.shortcut_settings.draft.as_mut() {
                    draft.pan_drag_modifier = modifier;
                }
                self.work.shortcut_settings.recording_pan_drag = false;
            }
        }
        if !self.loading.keybindings
            && let Some(action) = self.work.shortcut_settings.recording
        {
            let captured = ctx.input_mut(|input| {
                let index = input.events.iter().rposition(|event| match event {
                    egui::Event::Key { pressed: true, repeat: false, .. } => true,
                    egui::Event::PointerButton { button, pressed: true, .. } =>
                        crate::pointer_input::binding_name(*button).is_some(),
                    _ => false,
                })?;
                match input.events.remove(index) {
                    egui::Event::Key { key, modifiers, .. } => Some((key.name().to_string(), modifiers)),
                    egui::Event::PointerButton { button, modifiers, .. } =>
                        Some((crate::pointer_input::binding_name(button)?.to_string(), modifiers)),
                    _ => None,
                }
            });
            if let Some((key, modifiers)) = captured {
                if key == "Escape" {
                    self.work.shortcut_settings.recording = None;
                } else if let Some(draft) = self.work.shortcut_settings.draft.as_mut() {
                    let chord = labello_domain::KeyChord {
                        key,
                        ctrl: false,
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        command: modifiers.command || modifiers.ctrl,
                    };
                    draft.bindings.insert(action, chord);
                    self.work.shortcut_settings.recording = None;
                }
            }
        }
        let screen = ctx.content_rect();
        let short = Self::short_viewport(screen.size());
        let max_height = (screen.height() - 48.0).max(180.0);
        let width = (screen.width() - 48.0).clamp(240.0, 720.0);
        let mut record = None;
        let mut record_pan_drag = false;
        let mut reset_binding = None;
        let mut reset_pan_drag = false;
        let mut save = false;
        let mut cancel = false;
        let mut reset_all = false;
        let mut show_conflicts = false;
        let response = theme::modal(ctx, egui::Id::new("settings-modal")).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_max_height(max_height);
            let header = ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = theme::quiet_button(ui, !self.loading.keybindings,
                        egui::Button::new("×").min_size(egui::vec2(44.0, 44.0)))
                        .on_hover_text("Close keyboard shortcuts");
                    close.widget_info(|| egui::WidgetInfo::labeled(
                        egui::WidgetType::Button, !self.loading.keybindings, "Close keyboard shortcuts"));
                    cancel = close.clicked();
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 44.0),
                        egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.heading("Keyboard shortcuts");
                        });
                });
            });
            let content_height = (max_height - header.response.rect.height() - ui.spacing().item_spacing.y).max(64.0);
            let mut contents = |ui: &mut egui::Ui| {
                ui.label(
                    RichText::new("Record a key, right-click, or mouse button 4/5.")
                        .color(theme::MUTED),
                ).on_hover_text("Mouse bindings work over the canvas. Left and middle buttons stay reserved for editing and panning.");
                if let Some(error) = &self.work.shortcut_settings.error {
                    theme::inline_message(
                        ui,
                        theme::Intent::Error,
                        format!("Could not save shortcuts: {error}"),
                    );
                }
                ui.add_space(6.0);
                let search_label = ui.label("Search actions");
                ui.add_sized(
                    [ui.available_width(), theme::COMPACT_TEXT_FIELD_HEIGHT],
                    theme::singleline_text_edit(&mut self.work.shortcut_settings.search)
                        .hint_text("Search actions, categories, or keys"),
                )
                .labelled_by(search_label.id);
                ui.add_space(8.0);
                let conflicts = self
                    .work
                    .shortcut_settings
                    .draft
                    .as_ref()
                    .map(|draft| draft.conflicts())
                    .unwrap_or_default();
                let conflicting_actions = conflicts
                    .iter()
                    .flat_map(|(_, actions)| actions.iter().copied())
                    .collect::<std::collections::BTreeSet<_>>();
                let query = self
                    .work
                    .shortcut_settings
                    .search
                    .trim()
                    .to_ascii_lowercase();
                let compact_footer = ui.available_width() < 420.0;
                let scroll_height = if compact_footer {
                    (screen.height() - 360.0).clamp(64.0, 520.0)
                } else if screen.height() < 700.0 {
                    (screen.height() - 380.0).clamp(120.0, 520.0)
                } else {
                    (screen.height() - 300.0).clamp(180.0, 520.0)
                };
                let scroll_height = (scroll_height
                    - if conflicts.is_empty() { 0.0 } else { 120.0 }
                    - if self.work.shortcut_settings.error.is_some() { 100.0 } else { 0.0 })
                    .max(64.0);
                let mut visible_action_count = 0;
                let mut action_list = |ui: &mut egui::Ui| {
                    let mut current_category = "";
                    for action in ordered_shortcut_actions() {
                        if !self.auth.prelabel_available
                            && matches!(
                                action,
                                labello_domain::UserAction::SelectPreviousPrelabel
                                    | labello_domain::UserAction::SelectNextPrelabel
                                    | labello_domain::UserAction::AcceptPrelabel
                                    | labello_domain::UserAction::DiscardPrelabel
                            )
                        {
                            continue;
                        }
                        let label = action_label(&action);
                        let category = action_category(action);
                        let description = action_description(action);
                        let chord = self
                            .work
                            .shortcut_settings
                            .draft
                            .as_ref()
                            .and_then(|draft| draft.bindings.get(&action))
                            .cloned();
                        let pan_drag_modifier = self
                            .work
                            .shortcut_settings
                            .draft
                            .as_ref()
                            .filter(|_| action == labello_domain::UserAction::TogglePanMode)
                            .map(|draft| draft.pan_drag_modifier);
                        let recording = self.work.shortcut_settings.recording == Some(action);
                        let conflict = conflicting_actions.contains(&action);
                        let named_buttons = if action == labello_domain::UserAction::MarkKeypointAbsent {
                            self.selected_task().and_then(|task| task.skeleton.as_ref())
                                .map(|spec| spec.keypoints.iter().map(|point| format!("Mark {} as not present", point.name))
                                    .collect::<Vec<_>>().join(" "))
                                .unwrap_or_default()
                        } else { String::new() };
                        if !shortcut_matches_query(
                            ctx,
                            action,
                            chord.as_ref(),
                            pan_drag_modifier,
                            conflict,
                            &query,
                            &named_buttons,
                        ) {
                            continue;
                        }
                        visible_action_count += 1;
                        if category != current_category {
                            if !current_category.is_empty() {
                                ui.add_space(8.0);
                            }
                            current_category = category;
                            ui.heading(RichText::new(category).size(16.0));
                        }
                        theme::card_frame().show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.push_id(action, |ui| {
                                let mut controls = |ui: &mut egui::Ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        let text = if recording {
                                            "Press shortcut…".to_string()
                                        } else {
                                            chord.as_ref().map(|chord| format_chord(ctx, chord))
                                                .unwrap_or_else(|| "Unassigned".to_string())
                                        };
                                        let response = ui.add_enabled(
                                            !self.loading.keybindings,
                                            egui::Button::new(&text).selected(recording)
                                                .wrap().min_size(egui::vec2(140.0, 44.0)),
                                        ).on_hover_text(format!("Record shortcut for {label}. Escape cancels recording."));
                                        response.widget_info(|| egui::WidgetInfo::selected(
                                            egui::WidgetType::Button, !self.loading.keybindings, recording,
                                            format!("Record shortcut for {label}: {text}"),
                                        ));
                                        if response.clicked() { record = Some(action); }
                                        let response = ui.add_enabled(!self.loading.keybindings,
                                            egui::Button::new(crate::glossary::RESET).min_size(egui::vec2(64.0, 44.0)));
                                        response.widget_info(|| egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button, !self.loading.keybindings, format!("Reset {label}")));
                                        if response.clicked() { reset_binding = Some(action); }
                                    });
                                };
                                let name_and_description = |ui: &mut egui::Ui| {
                                    ui.label(RichText::new(label).strong());
                                    ui.add(egui::Label::new(RichText::new(description).small().color(theme::MUTED)).wrap());
                                };
                                // Allocate the text column before laying out controls. A nested vertical
                                // UI in horizontal_wrapped otherwise takes the entire row width.
                                if ui.available_width() >= 600.0 {
                                    let text_width = ui.available_width() - 256.0 - ui.spacing().item_spacing.x;
                                    ui.horizontal_top(|ui| {
                                        ui.allocate_ui_with_layout(egui::vec2(text_width, 0.0),
                                            egui::Layout::top_down(egui::Align::Min), |ui| {
                                                ui.set_width(text_width);
                                                name_and_description(ui);
                                            });
                                        controls(ui);
                                    });
                                } else {
                                    name_and_description(ui);
                                    ui.add_space(6.0);
                                    controls(ui);
                                }
                                if recording {
                                    ui.small("Press a key, right-click, or mouse button 4/5. Escape cancels recording.");
                                }
                                let names = action_button_names(action);
                                if !names.is_empty() {
                                    egui::CollapsingHeader::new("Button names and contexts")
                                        .id_salt("button-names")
                                        .show(ui, |ui| { ui.add(egui::Label::new(names).wrap()); });
                                }
                            });
                            if let Some(pan_drag_modifier) = pan_drag_modifier {
                                ui.separator();
                                ui.label(RichText::new("Pan drag").strong());
                                ui.small("Hold the modifier and left-drag. Middle-drag also pans.");
                                ui.horizontal_wrapped(|ui| {
                                    let recording = self.work.shortcut_settings.recording_pan_drag;
                                    let text = if recording { "Press modifier…".to_string() } else { pan_drag_modifier.to_string() };
                                    let response = ui.add_enabled(!self.loading.keybindings,
                                        egui::Button::new(&text).selected(recording).wrap().min_size(egui::vec2(140.0, 44.0)));
                                    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button,
                                        !self.loading.keybindings, recording, format!("Record shortcut for Pan drag: {text}")));
                                    if response.clicked() { record_pan_drag = true; }
                                    let response = ui.add_enabled(!self.loading.keybindings,
                                        egui::Button::new(crate::glossary::RESET).min_size(egui::vec2(64.0, 44.0)));
                                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button,
                                        !self.loading.keybindings, "Reset Pan drag shortcut"));
                                    if response.clicked() { reset_pan_drag = true; }
                                });
                                if self.work.shortcut_settings.recording_pan_drag {
                                    ui.small("Press a modifier. Escape cancels recording.");
                                }
                            }
                            if conflict {
                                let peers = conflicts.iter()
                                    .filter(|(_, actions)| actions.contains(&action))
                                    .flat_map(|(_, actions)| actions.iter().copied())
                                    .filter(|other| *other != action && action.can_conflict_with(*other))
                                    .collect::<std::collections::BTreeSet<_>>();
                                for other in peers {
                                    ui.add(egui::Label::new(RichText::new(format!(
                                        "Conflicts with {} in {}.", action_label(&other), shortcut_conflict_context(action, other)
                                    )).color(theme::DANGER)).wrap());
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                };
                if short {
                    action_list(ui);
                } else {
                    egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                        .max_height(scroll_height)
                        .show(ui, |ui| action_list(ui));
                }
                if visible_action_count == 0 && !query.is_empty() {
                    ui.label(RichText::new("No shortcuts match your search.").color(theme::MUTED));
                }
                if !conflicts.is_empty() {
                    theme::inline_message(
                        ui,
                        theme::Intent::Error,
                        format!(
                            "Resolve {} shortcut conflict(s) before saving.",
                            conflicts.len()
                        ),
                    );
                }
                if !conflicts.is_empty() && query != "conflict" {
                    show_conflicts = ui.button("Show conflicting shortcuts").clicked();
                }
                let dirty =
                    self.work.shortcut_settings.draft != self.work.shortcut_settings.baseline;
                let mut restore_defaults = |ui: &mut egui::Ui| {
                    if ui
                        .add_enabled(
                            !self.loading.keybindings,
                            egui::Button::new("Restore all defaults"),
                        )
                        .clicked()
                    {
                        reset_all = true;
                    }
                };
                let mut decision_actions = |ui: &mut egui::Ui| {
                    if theme::primary_button(
                        ui,
                        dirty && conflicts.is_empty() && !self.loading.keybindings,
                        egui::Button::new(if self.loading.keybindings {
                            "Saving…"
                        } else {
                            crate::glossary::SAVE_CHANGES
                        }),
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if theme::quiet_button(
                        ui,
                        !self.loading.keybindings,
                        egui::Button::new(crate::glossary::CANCEL),
                    )
                    .clicked()
                    {
                        cancel = true;
                    }
                };
                if compact_footer {
                    ui.vertical(|ui| {
                        restore_defaults(ui);
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                decision_actions(ui);
                            });
                        });
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        restore_defaults(ui);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            decision_actions(ui);
                        });
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if dirty && conflicts.is_empty() {
                        ui.label(RichText::new(crate::glossary::UNSAVED_CHANGES).color(theme::AMBER));
                    }
                });
            };
            egui::ScrollArea::vertical().scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                .id_salt("settings-modal-scroll")
                .max_height(content_height)
                .show(ui, |ui| contents(ui));
        });
        response
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Window, true, crate::glossary::SETTINGS));
        if show_conflicts {
            self.work.shortcut_settings.search = "conflict".to_string();
        }
        if let Some(action) = record {
            self.work.shortcut_settings.recording = Some(action);
            self.work.shortcut_settings.recording_pan_drag = false;
        }
        if record_pan_drag {
            self.work.shortcut_settings.recording = None;
            self.work.shortcut_settings.recording_pan_drag = true;
        }
        if let Some(action) = reset_binding {
            let defaults =
                labello_domain::KeybindingSet::defaults_for(self.config.user_id.clone());
            let default = defaults.bindings.get(&action).cloned();
            if let (Some(draft), Some(default)) =
                (self.work.shortcut_settings.draft.as_mut(), default)
            {
                draft.bindings.insert(action, default);
            }
        }
        if reset_pan_drag
            && let Some(draft) = self.work.shortcut_settings.draft.as_mut()
        {
            draft.pan_drag_modifier = labello_domain::PanDragModifier::default();
            self.work.shortcut_settings.recording_pan_drag = false;
        }
        if reset_all {
            self.work.shortcut_settings.draft = Some(labello_domain::KeybindingSet::defaults_for(
                self.config.user_id.clone(),
            ));
            self.work.shortcut_settings.recording = None;
            self.work.shortcut_settings.recording_pan_drag = false;
        }
        if save {
            self.request_keybindings_save();
        }
        let dirty = self.work.shortcut_settings.draft != self.work.shortcut_settings.baseline;
        if cancel || (!self.loading.keybindings && response.should_close()) {
            self.work.shortcut_settings.recording = None;
            self.work.shortcut_settings.recording_pan_drag = false;
            if dirty {
                self.work.shortcut_settings.confirm_discard = true;
                self.work.show_settings = true;
            } else {
                self.work.show_settings = false;
                self.work.shortcut_settings.draft = None;
                self.work.shortcut_settings.baseline = None;
                self.work.shortcut_settings.error = None;
            }
        }
    }

    fn shortcut_discard_modal(&mut self, ctx: &egui::Context) {
        let response =
            theme::modal(ctx, egui::Id::new("discard-shortcut-settings")).show(ctx, |ui| {
                ui.heading("Discard shortcut changes?");
                ui.label("Your recorded shortcuts have not been saved.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Keep editing").clicked() {
                        self.work.shortcut_settings.confirm_discard = false;
                    }
                    if ui.button(crate::glossary::DISCARD_CHANGES).clicked() {
                        self.work.shortcut_settings.confirm_discard = false;
                        self.work.shortcut_settings.draft = None;
                        self.work.shortcut_settings.baseline = None;
                        self.work.shortcut_settings.error = None;
                        self.work.shortcut_settings.recording = None;
                        self.work.shortcut_settings.recording_pan_drag = false;
                        self.work.show_settings = false;
                    }
                });
            });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Window, true, "Discard shortcut changes")
        });
        if response.should_close() {
            self.work.shortcut_settings.confirm_discard = false;
        }
    }

}
