impl LabelloApp {
    pub(crate) fn export_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Export dataset");
        ui.label("Download verified ground truth and original images for Ultralytics. Native review history and IDs are not restored on re-import.");
        let busy = self.admin.export.pending.is_some();
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if theme::quiet_button(
                ui,
                !busy,
                egui::Button::new(if self.admin.export.error.is_some() {
                    "Retry export status"
                } else {
                    "Refresh exports"
                }),
            )
            .clicked()
            {
                action = Some(
                    self.admin
                        .export
                        .retry
                        .clone()
                        .unwrap_or(ExportAction::Load),
                );
            }
            if busy {
                ui.spinner();
                ui.label(if self.admin.export.loaded {
                    "Refreshing export data..."
                } else {
                    "Loading export capabilities and history..."
                });
            }
        });
        if let Some(error) = &self.admin.export.error {
            theme::inline_message(
                ui,
                theme::Intent::Error,
                format!(
                    "{}: {error}",
                    if self.admin.export.loaded {
                        "Export refresh or request failed; showing saved data"
                    } else {
                        "Export data unavailable"
                    }
                ),
            );
        }
        if let Some(notice) = &self.admin.export.notice {
            theme::inline_message(ui, theme::Intent::Info, notice);
        }
        let available = self
            .admin
            .export
            .capabilities
            .as_ref()
            .is_some_and(|c| c.available);
        if !self.admin.export.loaded {
            if !busy && self.admin.export.error.is_none() {
                ui.label("Loading export capabilities and history...");
            }
        } else if !available {
            ui.label("Dataset export is unavailable on this server.");
        } else {
            let metadata = self
                .datasets
                .admin_baseline
                .as_ref()
                .expect("loaded admin configuration")
                .clone();
            let saved = self.export_config_saved();
            if !saved {
                theme::inline_message(
                    ui,
                    theme::Intent::Warning,
                    "Save or discard staged Admin changes before preflight or Start.",
                );
            }
            let before = self.admin.export.options.clone();
            ui.add_space(theme::SPACE_3);
            ui.heading("Selection");
            ui.add_enabled_ui(!busy, |ui| {
                let label = ui.label("Export profile");
                egui::ComboBox::from_id_salt("export-profile").width(ui.available_width().min(360.0))
                    .selected_text(profile_label(self.admin.export.options.profile))
                    .show_ui(ui, |ui| {
                        for profile in [ExportProfile::UltralyticsYoloDetectV1, ExportProfile::UltralyticsYoloPoseV1] {
                            ui.selectable_value(&mut self.admin.export.options.profile, profile, profile_label(profile));
                        }
                    }).response.labelled_by(label.id);
                if before.profile != self.admin.export.options.profile {
                    self.admin.export.options.classes.clear();
                    self.admin.export.options.split_choices.clear();
                }
                let label = ui.label("Split for images without split provenance");
                egui::ComboBox::from_id_salt("export-fallback-split").width(ui.available_width().min(360.0))
                    .selected_text(self.admin.export.options.fallback_split.as_str())
                    .show_ui(ui, |ui| {
                        for split in [ExportSplit::Train, ExportSplit::Val, ExportSplit::Test] {
                            ui.selectable_value(&mut self.admin.export.options.fallback_split, split, split.as_str());
                        }
                    }).response.labelled_by(label.id);
                ui.small("Existing unambiguous train/val/test membership is preserved. Conflicts require an explicit image choice during preflight.");
                ui.label(egui::RichText::new("Tasks and classes").strong());
                let mut compatible = false;
                for task in &metadata.tasks {
                    if task.annotation_type != self.admin.export.options.profile.annotation_type() { continue; }
                    compatible = true;
                    ui.label(egui::RichText::new(&task.name).strong());
                    for class in metadata.label_classes.iter().filter(|class| task.allows_class(&class.class_id)) {
                        let selection = labello_domain::ExportClassSelection { task_id: task.task_id.clone(), class_id: class.class_id.clone() };
                        let mut selected = self.admin.export.options.classes.contains(&selection);
                        let label = format!("{} / {} [{} · {}]", task.name, class.name, task.task_id, class.class_id);
                        let response = ui.checkbox(&mut selected, &label).on_hover_text(format!("Task {} · Class {}", task.task_id, class.class_id));
                        if response.changed() {
                            if selected { self.admin.export.options.classes.insert(selection); }
                            else { self.admin.export.options.classes.remove(&selection); }
                        }
                    }
                }
                if !compatible { ui.label("No tasks match this export profile. Choose another profile or configure compatible tasks."); }
            });
            if before != self.admin.export.options {
                self.admin.export.reviewed = false;
            }
            let validation = self.admin.export.options.class_mapping(&metadata);
            if let Err(error) = validation {
                theme::inline_message(ui, theme::Intent::Warning, error.to_string());
            }
            let retained = self
                .admin
                .export
                .retained_capture()
                .map(|job| job.job_id.clone());
            if let Some(id) = &retained {
                ui.label("Cancel the retained capture before creating a new preflight. Completed archives remain in history until expiry.");
                if self.admin.export.selected.as_ref() != Some(id)
                    && theme::quiet_button(ui, !busy, egui::Button::new("Review retained capture"))
                        .clicked()
                {
                    self.admin.export.select_job(id);
                }
            }
            if theme::primary_button(
                ui,
                !busy && saved && validation.is_ok() && retained.is_none(),
                egui::Button::new("Run export preflight"),
            )
            .clicked()
            {
                action = Some(ExportAction::Preflight(self.admin.export.options.clone()));
            }
            ui.add_space(theme::SPACE_3);
            if let Some(job) = self.admin.export.selected_job().cloned() {
                self.export_job_view(ui, &job, busy, &mut action);
            } else if self.admin.export.jobs.is_empty() {
                ui.label(
                    "No exports yet. Choose a profile and task/class mappings, then run preflight.",
                );
            }
            if !self.admin.export.jobs.is_empty() {
                ui.add_space(theme::SPACE_3);
                egui::CollapsingHeader::new("Export history")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.small(
                            "Select a retained job to inspect its captured options and result.",
                        );
                        let jobs = self.admin.export.jobs.clone();
                        for job in jobs {
                            let label = format!(
                                "{} · {} · {}",
                                phase_label(job.phase),
                                profile_label(job.options.profile),
                                job.created_at.format("%m-%d %H:%M UTC")
                            );
                            let response = ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::selectable(
                                        self.admin.export.selected.as_ref() == Some(&job.job_id),
                                        label,
                                    )
                                    .wrap(),
                                )
                                .on_hover_text(&job.job_id);
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    !busy,
                                    format!("Export {}: {}", job.job_id, phase_label(job.phase)),
                                )
                            });
                            if response.clicked() {
                                self.admin.export.select_job(&job.job_id);
                            }
                        }
                    });
            }
            if let Some(capabilities) = &self.admin.export.capabilities {
                ui.small(format!("Up to {} images and {} of original images per export; retained for {} hours. Limits are checked by the server.", capabilities.limits.max_images, crate::admin::human_bytes(capabilities.limits.max_source_bytes), capabilities.limits.retention_seconds / 3600));
            }
        }
        if let Some(action) = action {
            self.request_export(action);
        }
    }

    fn export_job_view(
        &mut self,
        ui: &mut egui::Ui,
        job: &ExportJob,
        busy: bool,
        action: &mut Option<ExportAction>,
    ) {
        ui.heading("Captured export");
        ui.label(format!("Status: {}", phase_label(job.phase)));
        ui.label(format!("Profile: {}", profile_label(job.options.profile)));
        ui.small(format!(
            "Expires {} UTC",
            job.expires_at.format("%Y-%m-%d %H:%M")
        ));
        if job.phase.is_active() {
            ui.spinner();
        }
        if let Some(failure) = job.failure {
            theme::inline_message(ui, theme::Intent::Error, failure.to_string());
        }
        if let Some(summary) = &job.summary {
            ui.label(format!(
                "{} images · {} objects · {} verified empty images",
                summary.included_images, summary.objects, summary.empty_images
            ));
            ui.label(format!(
                "{} omitted images · {} blocking images · {} originals",
                summary.omitted_images,
                summary.blocking_images,
                crate::admin::human_bytes(summary.source_bytes)
            ));
            if !summary.can_start() {
                theme::inline_message(
                    ui,
                    theme::Intent::Warning,
                    "This preflight cannot start. Resolve blockers or choose complete ground-truth coverage, then cancel this capture and run a new preflight.",
                );
            }
            egui::CollapsingHeader::new("Class mapping and omissions").show(ui, |ui| {
                for class in &summary.classes {
                    ui.label(format!("{}: {} · task {} · class {}", class.index, class.name, class.selection.task_id, class.selection.class_id));
                    if let Some(spec) = &class.skeleton { ui.label(format!("Keypoint order: {}", spec.keypoints.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))); }
                }
                for (reason, count) in &summary.omission_counts { ui.label(format!("{count}: {}", omission_label(*reason))); }
                for sample in &summary.omitted_samples { ui.label(format!("{}: {}", sample.image_id, omission_label(sample.reason))); }
                if summary.omitted_images > summary.omitted_samples.len() { ui.small("Only bounded omission examples are shown. Complete omission records are included in the manifest."); }
            });
            for blocker in &summary.blockers {
                ui.label(format!("Image {}: {}", blocker.image_id, blocker.reason));
                if blocker.reason
                    == labello_client::ExportFailure::Policy(
                        labello_domain::ExportPolicyError::SplitConflict,
                    )
                {
                    let label = ui.label(format!("Split for image {}", blocker.image_id));
                    let mut choice = self
                        .admin
                        .export
                        .options
                        .split_choices
                        .get(&blocker.image_id)
                        .copied();
                    ui.add_enabled_ui(!busy, |ui| {
                        egui::ComboBox::from_id_salt((
                            "export-image-split",
                            blocker.image_id.as_str(),
                        ))
                        .width(ui.available_width().min(280.0))
                        .selected_text(choice.map(|s| s.as_str()).unwrap_or("Choose split"))
                        .show_ui(ui, |ui| {
                            for split in [ExportSplit::Train, ExportSplit::Val, ExportSplit::Test] {
                                ui.selectable_value(&mut choice, Some(split), split.as_str());
                            }
                        })
                        .response
                        .labelled_by(label.id);
                    });
                    if let Some(choice) = choice {
                        self.admin
                            .export
                            .options
                            .split_choices
                            .insert(blocker.image_id.clone(), choice);
                    }
                }
            }
            if summary.blocking_images > summary.blockers.len() {
                ui.small("Only the first 100 blockers are shown. A new preflight may reveal further blockers.");
            }
        }
        if job.options != self.admin.export.options {
            self.admin.export.reviewed = false;
            theme::inline_message(
                ui,
                theme::Intent::Warning,
                "The selection differs from this captured preflight. Start is disabled.",
            );
        }
        if job.phase == ExportPhase::Ready {
            ui.add_enabled(
                !busy && job.options == self.admin.export.options,
                egui::Checkbox::new(
                    &mut self.admin.export.reviewed,
                    "I reviewed the captured export summary",
                ),
            );
        }
        ui.horizontal_wrapped(|ui| {
            if job.phase == ExportPhase::Ready && theme::primary_button(ui, !busy && self.export_config_saved() && self.admin.export.reviewed && job.options == self.admin.export.options && job.summary.as_ref().is_some_and(|s| s.can_start()), egui::Button::new("Start export")).clicked() { *action = Some(ExportAction::Start(job.job_id.clone())); }
            if matches!(job.phase, ExportPhase::Capturing | ExportPhase::Ready | ExportPhase::Blocked | ExportPhase::Building)
                && theme::danger_button(ui, !busy, egui::Button::new("Cancel export")).on_hover_text("Cancel this capture or archive build. Source annotations and completed archives are unchanged.").clicked() { *action = Some(ExportAction::Cancel(job.job_id.clone())); }
            if job.phase == ExportPhase::Succeeded && theme::primary_button(ui, !busy, egui::Button::new("Download export archive")).clicked() { *action = Some(ExportAction::Download(job.job_id.clone())); }
        });
        if job.phase == ExportPhase::Succeeded {
            if let Some(bytes) = job.archive_bytes {
                ui.label(format!(
                    "Archive ready · {}",
                    crate::admin::human_bytes(bytes)
                ));
            }
            ui.small("Download authorization is checked again. Your browser handles the archive transfer.");
        }
        if matches!(job.phase, ExportPhase::Failed | ExportPhase::Cancelled) {
            ui.label("Choose or keep the export selection and run preflight again to retry.");
        }
    }
}

fn profile_label(profile: ExportProfile) -> &'static str {
    match profile {
        ExportProfile::UltralyticsYoloDetectV1 => "YOLO detect v1",
        ExportProfile::UltralyticsYoloPoseV1 => "YOLO pose v1",
    }
}

fn phase_label(phase: ExportPhase) -> &'static str {
    match phase {
        ExportPhase::Capturing => "Capturing preflight",
        ExportPhase::Ready => "Ready for review",
        ExportPhase::Blocked => "Blocked",
        ExportPhase::Building => "Building archive",
        ExportPhase::Cancelling => "Cancelling",
        ExportPhase::Cancelled => "Cancelled",
        ExportPhase::Failed => "Failed",
        ExportPhase::Succeeded => "Archive ready",
    }
}

fn omission_label(reason: labello_domain::ExportOmissionReason) -> &'static str {
    use labello_domain::ExportOmissionReason::*;
    match reason {
        Unfinished => "unfinished task",
        ExcludedCoverage => "excluded coverage",
        IncompleteCoverage => "incomplete coverage",
        UnverifiedAnnotations => "unverified annotations",
        ChangedReviewPolicy => "review policy changed",
        UnresolvedMigration => "unresolved migration",
    }
}
