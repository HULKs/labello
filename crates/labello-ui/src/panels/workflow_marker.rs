#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowMarkerReason {
    Saving,
    ImageLoading,
    SessionLoading,
    DatasetLoading,
    SigningOut,
    MigrationBusy,
    StatisticsOpen,
    Transition,
    Checking,
    CheckFailed,
    Unavailable(labello_domain::WorkflowUnavailableReason),
}

impl WorkflowMarkerReason {
    pub(crate) fn label(self) -> &'static str {
        use labello_domain::WorkflowUnavailableReason as R;
        match self {
            Self::Saving => "Saving changes",
            Self::ImageLoading => "Loading image",
            Self::SessionLoading => "Loading session",
            Self::DatasetLoading => "Loading dataset",
            Self::SigningOut => "Signing out",
            Self::MigrationBusy => "Updating migration",
            Self::StatisticsOpen => "Statistics dialog open",
            Self::Transition => "Workflow transition pending",
            Self::Checking => "Checking for available work",
            Self::CheckFailed => "Availability unknown",
            Self::Unavailable(reason) => match reason {
                R::BalanceLimit => "Other workflows need to catch up",
                R::ReviewDisabled => "Review is disabled for this workflow",
                R::EmptyDataset => "This dataset has no images",
                R::AnnotationFinished => "No annotation work remaining",
                R::NothingAwaitingReview => "No work awaiting review",
                R::ClaimedByOthers => "Available work is assigned to others",
                R::ReviewRevision => "Work is locked for review revision",
                R::ImportExcluded => "Imported images are excluded from annotation",
                R::ReviewFinalized => "Review is already complete",
                R::WorkflowDisabled => "This workflow is disabled",
                R::ObjectsPending => "Objects must be finished before Overview",
                R::PreparationPending => "Dataset model is preparing objects",
                R::PreparationFailed => "Dataset model needs administrator attention",
                R::OverviewLimit => "Finish pending Overview work first",
                R::NoObjects => "No objects remaining",
                R::Unavailable => "No assignments available",
            },
        }
    }
}

impl LabelloApp {
    pub(crate) fn workflow_interaction_block(&self) -> Option<WorkflowMarkerReason> {
        use WorkflowMarkerReason as M;
        let navigating = matches!(self.work.pending_transition,
            Some(PendingTransition::NextAssignment | PendingTransition::PreviousAssignment(_)))
            && (self.loading.image || (self.loading.saving && self.work.save_status != SaveStatus::Saving));
        if self.saving_blocks_interaction() && !navigating { return Some(M::Saving); }
        if self.loading.session { return Some(M::SessionLoading); }
        if self.loading.logout { return Some(M::SigningOut); }
        if self.loading.dataset { return Some(M::DatasetLoading); }
        // Assignment release shares the save guard, but it is a navigation barrier.
        if navigating { return Some(M::Transition); }
        if self.loading.image { return Some(M::ImageLoading); }
        if self.work.migration.busy { return Some(M::MigrationBusy); }
        if self.work.pending_transition.is_some() { return Some(M::Transition); }
        if self.navigation.statistics.open { return Some(M::StatisticsOpen); }
        self.workspace_bars_loading().then_some(M::Checking)
    }

    pub(crate) fn workflow_marker_reason(
        &self,
        task_id: &labello_domain::TaskId,
    ) -> Option<WorkflowMarkerReason> {
        use WorkflowMarkerReason as M;
        if let Some(reason) = self.workflow_interaction_block() { return Some(reason); }
        let kind = self.assignment_kind()?;
        let availability = &self.work.availability;
        if availability.dataset_id.as_ref() != Some(&self.config.dataset_id)
            || availability.kind.as_ref() != Some(&kind)
        {
            return None;
        }
        // Retained known-unavailable cards remain disabled during a refresh.
        // Their restriction must remain visible rather than suggesting checking alone blocks them.
        if self.displayed_workflow_availability(task_id) == Some(false) {
            return Some(M::Unavailable(
                availability
                    .reasons
                    .get(task_id)
                    .copied()
                    .unwrap_or(labello_domain::WorkflowUnavailableReason::Unavailable),
            ));
        }
        if availability.error.is_some() { return Some(M::CheckFailed); }
        (availability.loading && self.work.current.is_none() && !availability.resolved && self.initial_workspace_load()).then_some(M::Checking)
    }
}

fn paint_workflow_marker(
    ui: &egui::Ui,
    rect: egui::Rect,
    selected: bool,
    reason: Option<WorkflowMarkerReason>,
    reason_color: egui::Color32,
) {
    use WorkflowMarkerReason as M;
    use labello_domain::WorkflowUnavailableReason as R;
    let painter = ui.painter();
    let center = rect.center();
    let origin = center - egui::vec2(9.0, 9.0);
    let point = |x, y| origin + egui::vec2(x, y);
    let stroke = egui::Stroke::new(1.5, reason_color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)], closed| {
        let points = points.iter().map(|&(x, y)| point(x, y)).collect();
        painter.add(if closed {
            egui::Shape::closed_line(points, stroke)
        } else {
            egui::Shape::line(points, stroke)
        });
    };
    let check = || path(&[(3.0, 9.0), (7.0, 13.0), (15.0, 5.0)], false);
    let shield = || {
        path(
            &[
                (9.0, 1.0),
                (16.0, 4.0),
                (15.0, 11.0),
                (9.0, 17.0),
                (3.0, 11.0),
                (2.0, 4.0),
            ],
            true,
        )
    };
    let image = || {
        painter.rect_stroke(
            egui::Rect::from_min_max(point(2.0, 3.0), point(16.0, 15.0)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        path(&[(3.0, 13.0), (7.0, 8.0), (11.0, 12.0), (14.0, 9.0)], false);
    };
    let lock = || {
        painter.rect_filled(
            egui::Rect::from_min_max(point(9.0, 9.0), point(18.0, 18.0)),
            1.0,
            theme::PANEL,
        );
        painter.rect_stroke(
            egui::Rect::from_min_max(point(10.0, 11.0), point(17.0, 17.0)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        path(
            &[
                (11.0, 11.0),
                (11.0, 8.0),
                (13.5, 6.0),
                (16.0, 8.0),
                (16.0, 11.0),
            ],
            false,
        );
    };
    match reason {
        None => {}
        Some(M::Saving | M::ImageLoading | M::SessionLoading | M::DatasetLoading | M::SigningOut | M::MigrationBusy | M::Checking) => {
            // A static segmented spinner communicates waiting without continuous animation.
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + direction * 5.0, center + direction * 8.0],
                    egui::Stroke::new(
                        1.5,
                        reason_color.gamma_multiply(0.35 + i as f32 * 0.09),
                    ),
                );
            }
        }
        Some(M::StatisticsOpen) => {
            painter.rect_stroke(egui::Rect::from_min_max(point(2.0, 3.0), point(16.0, 15.0)), 2.0, stroke, egui::StrokeKind::Inside);
            line((2.0, 7.0), (16.0, 7.0));
        }
        Some(M::Transition) => {
            path(
                &[
                    (3.0, 2.0),
                    (15.0, 2.0),
                    (15.0, 5.0),
                    (3.0, 13.0),
                    (3.0, 16.0),
                    (15.0, 16.0),
                    (15.0, 13.0),
                    (3.0, 5.0),
                ],
                true,
            );
        }
        Some(M::CheckFailed) => {
            painter.circle_stroke(center, 8.0, stroke);
            path(
                &[
                    (6.0, 6.0),
                    (7.0, 4.0),
                    (11.0, 4.0),
                    (12.0, 6.0),
                    (9.0, 9.0),
                    (9.0, 11.0),
                ],
                false,
            );
            painter.circle_filled(point(9.0, 14.0), 0.9, reason_color);
        }
        Some(M::Unavailable(R::BalanceLimit | R::OverviewLimit)) => {
            line((9.0, 1.0), (9.0, 16.0));
            line((4.0, 16.0), (14.0, 16.0));
            line((2.0, 4.0), (16.0, 4.0));
            for x in [4.0, 14.0] {
                path(&[(x, 4.0), (x - 3.0, 11.0), (x + 3.0, 11.0)], true);
            }
        }
        Some(M::Unavailable(R::ReviewDisabled | R::WorkflowDisabled)) => {
            shield();
            let cross_stroke = egui::Stroke::new(2.0, theme::TEXT);
            painter.line_segment([point(6.5, 6.0), point(11.5, 11.0)], cross_stroke);
            painter.line_segment([point(11.5, 6.0), point(6.5, 11.0)], cross_stroke);
        }
        Some(M::Unavailable(R::EmptyDataset | R::NoObjects)) => {
            line((0.0, 6.0), (0.0, 18.0));
            line((0.0, 18.0), (13.0, 18.0));
            image();
        }
        Some(M::Unavailable(R::AnnotationFinished)) => check(),
        Some(M::Unavailable(R::NothingAwaitingReview)) => {
            path(
                &[
                    (1.0, 10.0),
                    (4.0, 3.0),
                    (14.0, 3.0),
                    (17.0, 10.0),
                    (17.0, 16.0),
                    (1.0, 16.0),
                ],
                true,
            );
            path(
                &[
                    (1.0, 10.0),
                    (6.0, 10.0),
                    (7.0, 12.0),
                    (11.0, 12.0),
                    (12.0, 10.0),
                    (17.0, 10.0),
                ],
                false,
            );
        }
        Some(M::Unavailable(R::ClaimedByOthers)) => {
            painter.circle_stroke(point(6.0, 5.0), 3.0, stroke);
            path(
                &[
                    (1.0, 17.0),
                    (1.0, 12.0),
                    (4.0, 10.0),
                    (8.0, 10.0),
                    (11.0, 12.0),
                ],
                false,
            );
            lock();
        }
        Some(M::Unavailable(R::ReviewRevision | R::ObjectsPending)) => {
            shield();
            lock();
        }
        Some(M::Unavailable(R::ImportExcluded)) => {
            image();
            line((1.0, 17.0), (17.0, 1.0));
        }
        Some(M::Unavailable(R::ReviewFinalized)) => {
            shield();
            painter.add(egui::Shape::line(
                vec![point(5.5, 8.5), point(8.0, 11.0), point(12.5, 6.5)],
                egui::Stroke::new(2.0, theme::TEXT),
            ));
        }
        Some(M::Unavailable(R::Unavailable | R::PreparationPending | R::PreparationFailed)) => {
            painter.circle_stroke(center, 8.0, stroke);
            line((5.0, 9.0), (13.0, 9.0));
        }
    }
    if selected {
        let dot = if reason.is_some() {
            point(17.0, 0.0)
        } else {
            center
        };
        // Paint outside the disabled child UI to keep the committed selection legible.
        painter.circle_filled(dot, 4.0, theme::TEXT);
    }
}
