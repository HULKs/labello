// Presentation survives an image transition; assignments, drafts and command ownership do not.
#[derive(Default)]
pub(crate) struct WorkspaceBars {
    scope: Option<BarScope>,
    presentation: Option<BarPresentation>,
    settled: bool,
}

#[derive(PartialEq, Eq)]
struct BarScope {
    auth_epoch: u64,
    workspace_epoch: u64,
    view: AppView,
    dataset: labello_domain::DatasetId,
    task: Option<labello_domain::TaskId>,
}

#[derive(Clone)]
struct BarPresentation {
    summary: WorkspaceSummary,
    inspector: Option<crate::review_context::ReviewContext>,
    migration: Option<crate::manual_migration::MigrationBarPresentation>,
    previous_image: bool,
    availability_loading: bool,
    review_removable: bool,
    review_primary: &'static str,
    review_next: &'static str,
    prelabel_progress: Option<String>,
    annotation_primary: crate::prelabel_review::PrelabelPrimaryAction,
}

impl LabelloApp {
    pub(crate) fn workspace_bars_loading(&self) -> bool {
        self.loading.session
            || self.loading.dataset
            || self.loading.image
            || self.loading.logout
            || ((self.work.current.is_none() || self.work.retired_image)
                && self.work.availability.loading
                && self.work.availability.load_after_resolution)
    }

    pub(crate) fn sync_workspace_bars(&mut self) {
        let scope = BarScope {
            auth_epoch: self.auth_epoch,
            workspace_epoch: self.workspace_epoch,
            view: self.view,
            dataset: self.config.dataset_id.clone(),
            task: self.work.selected_task_id.clone(),
        };
        let scope_changed = self.navigation.workspace_bars.scope.as_ref() != Some(&scope);
        if self.work.retired_image && (scope_changed || !self.work_view()
            || self.loading.session || self.loading.logout || self.loading.dataset
            || !self.workspace_bars_loading()) {
            self.clear_current_image();
        }
        if scope_changed
            || !self.work_view()
            || self.loading.session
            || self.loading.logout
        {
            self.navigation.workspace_bars = WorkspaceBars {
                scope: Some(scope),
                presentation: None,
                settled: false,
            };
        }
        if self.workspace_bars_loading() {
            return;
        }
        if !(self.work.current.is_none() && self.work.availability.loading
            && !self.work.availability.resolved) {
            self.navigation.workspace_bars.settled = true;
        }
        self.navigation.workspace_bars.presentation = self
            .work
            .current
            .as_ref()
            .filter(|_| self.runtime.api.is_none() || self.work.assignment.is_some())
            .map(|_| BarPresentation {
                prelabel_progress: self.prelabel_progress(),
                annotation_primary: self.prelabel_primary_action(),
                summary: WorkspaceSummary::from_app(self),
                inspector: self.review_context(),
                migration: self
                    .manual_migration_active()
                    .then(|| self.migration_bar_presentation()),
                previous_image: self.workflow_context().is_some() || self.work.previous_assignment.is_some(),
                availability_loading: self.navigation.workspace_bars.presentation.is_none()
                    && self.work.availability.loading && self.work.availability.tasks.is_empty(),
                review_removable: self.migration_review_removal().is_some(),
                review_primary: if self.focused_review_changed() {
                    crate::glossary::SUBMIT_CORRECTION
                } else {
                    crate::glossary::APPROVE
                },
                review_next: if self.review_position() + 1 == self.review_object_targets().len() {
                    crate::glossary::OVERVIEW
                } else {
                    crate::glossary::NEXT_OBJECT
                },
            });
    }

    pub(crate) fn initial_workspace_load(&self) -> bool {
        !self.navigation.workspace_bars.settled
    }

    pub(crate) fn displayed_inspector_context(&self) -> Option<crate::review_context::ReviewContext> {
        if self.workspace_bars_loading() {
            self.navigation.workspace_bars.presentation.as_ref().and_then(|bar| bar.inspector.clone())
        } else {
            self.review_context()
        }
    }

    pub(crate) fn workspace_bars_blank(&self) -> bool {
        self.workspace_bars_loading() && self.navigation.workspace_bars.presentation.is_none()
    }

    fn bar_prelabel_progress(&self) -> Option<String> {
        self.navigation.workspace_bars.presentation.as_ref().and_then(|bar| bar.prelabel_progress.clone())
    }

    fn bar_annotation_primary(&self) -> crate::prelabel_review::PrelabelPrimaryAction {
        self.navigation.workspace_bars.presentation.as_ref().map(|bar| bar.annotation_primary).unwrap_or_default()
    }

    fn bar_availability_loading(&self) -> bool {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .is_some_and(|bar| bar.availability_loading)
    }

    fn displayed_workspace_summary(&self) -> WorkspaceSummary {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .map(|bar| bar.summary.clone())
            .unwrap_or_else(|| {
                if self.workspace_bars_blank() {
                    // Reserve the normal two-line workspace summary without showing a placeholder.
                    WorkspaceSummary {
                        progress: " ".into(),
                        identity: Some(" ".into()),
                        annotation_type: None,
                        accessible: String::new(),
                        submitter: None,
                        show_avatar: self.view == AppView::Review,
                    }
                } else {
                    WorkspaceSummary::from_app(self)
                }
            })
    }

    pub(crate) fn bar_migration_active(&self) -> bool {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .map_or_else(
                || {
                    self.selected_task()
                        .is_some_and(|task| task.manual_box_guide_migration.is_some())
                },
                |bar| bar.migration.is_some(),
            )
    }

    pub(crate) fn displayed_migration_bar(
        &self,
    ) -> crate::manual_migration::MigrationBarPresentation {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .and_then(|bar| bar.migration.clone())
            .unwrap_or_else(|| self.migration_bar_presentation())
    }

    pub(crate) fn bar_has_previous_image(&self) -> bool {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .is_some_and(|bar| bar.previous_image)
    }

    fn bar_review_removable(&self) -> bool {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .is_some_and(|bar| bar.review_removable)
    }

    fn bar_review_primary_label(&self) -> &'static str {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .map_or(crate::glossary::APPROVE, |bar| bar.review_primary)
    }

    fn bar_review_next_label(&self) -> &'static str {
        self.navigation
            .workspace_bars
            .presentation
            .as_ref()
            .map_or(crate::glossary::NEXT_OBJECT, |bar| bar.review_next)
    }
}
