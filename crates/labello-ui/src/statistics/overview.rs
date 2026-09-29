use crate::app::{LabelloApp, UiCommand, UiRequestError};
use eframe::egui;
use labello_domain::DatasetId;
use web_time::Instant;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum StatisticsScope {
    #[default]
    Workspace,
    All,
    Dataset(DatasetId),
}

#[derive(Default)]
pub(crate) struct OverviewState {
    pub scope: StatisticsScope,
    pub rows: Vec<labello_client::DatasetStatistics>,
    pub total: labello_domain::DatasetStats,
    pub pending: Option<u64>,
    pub attempted: Option<Instant>,
    pub completed: Option<Instant>,
    pub error: Option<String>,
}

impl LabelloApp {
    pub(crate) fn statistics_scope_selector(&mut self, ui: &mut egui::Ui) {
        let mut scope = self.datasets.overview.scope.clone();
        let label = match &scope {
            StatisticsScope::All => "All accessible datasets".to_owned(),
            StatisticsScope::Workspace => self
                .datasets
                .metadata
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| self.config.dataset_id.to_string()),
            StatisticsScope::Dataset(id) => self
                .datasets
                .summaries
                .iter()
                .find(|row| &row.dataset_id == id)
                .map(|row| row.name.clone())
                .unwrap_or_else(|| id.to_string()),
        };
        egui::ComboBox::from_id_salt("statistics-scope")
            .selected_text(label)
            .width(ui.available_width().min(400.0))
            .truncate()
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut scope, StatisticsScope::All, "All accessible datasets");
                for row in self
                    .datasets
                    .summaries
                    .iter()
                    .filter(|row| !row.roles.is_empty())
                {
                    let value = if row.dataset_id == self.config.dataset_id
                        && self.datasets.metadata.is_some()
                    {
                        StatisticsScope::Workspace
                    } else {
                        StatisticsScope::Dataset(row.dataset_id.clone())
                    };
                    ui.selectable_value(
                        &mut scope,
                        value,
                        format!("{} ({})", row.name, row.dataset_id),
                    );
                }
            })
            .response
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Statistics for")
            });
        if scope != self.datasets.overview.scope {
            self.select_statistics_scope(scope);
        }
    }

    pub(crate) fn select_statistics_scope(&mut self, scope: StatisticsScope) {
        if let Some(id) = self.datasets.overview.pending {
            self.runtime.active_requests.remove(&id);
        }
        self.datasets.overview = OverviewState {
            scope,
            ..Default::default()
        };
        self.datasets.leaderboard = Default::default();
        self.request_stats();
    }

    pub(crate) fn request_overview(&mut self) {
        if self.runtime.api.is_none()
            || self.datasets.overview.scope == StatisticsScope::Workspace
            || self.datasets.overview.pending.is_some()
        {
            return;
        }
        let request = self.request_identity(None);
        self.datasets.overview.pending = Some(request.request_id);
        self.datasets.overview.attempted = Some(Instant::now());
        self.queue_command(UiCommand::Overview { request });
    }

    pub(crate) fn refresh_overview_if_due(&mut self) {
        if self.statistics_visible()
            && self
                .datasets
                .overview
                .attempted
                .is_none_or(|time| time.elapsed() >= self.statistics_refresh_interval())
        {
            self.request_overview();
        }
    }

    pub(crate) fn accept_overview(
        &mut self,
        id: u64,
        result: Result<Vec<labello_client::DatasetStatistics>, UiRequestError>,
    ) {
        if self.datasets.overview.pending != Some(id) {
            return;
        }
        self.datasets.overview.pending = None;
        match result {
            Ok(rows) => {
                if let StatisticsScope::Dataset(selected) = &self.datasets.overview.scope
                    && !rows.iter().any(|row| &row.dataset_id == selected)
                {
                    self.datasets.overview.rows.clear();
                    self.datasets.overview.completed = None;
                    self.datasets.overview.error = Some(
                        "This dataset is no longer accessible. Choose another statistics scope."
                            .into(),
                    );
                    return;
                }
                self.datasets.overview.total =
                    labello_domain::aggregate_statistics(rows.iter().map(|row| &row.stats));
                self.datasets.overview.rows = rows;
                self.datasets.overview.completed = Some(Instant::now());
                self.datasets.overview.error = None;
            }
            Err(error) => {
                // A failed aggregate may include revoked access. Hide cached data
                // until the server has established the complete authorized set.
                self.datasets.overview.rows.clear();
                self.datasets.overview.completed = None;
                self.datasets.overview.error = Some(error.to_string());
            }
        }
    }
}
