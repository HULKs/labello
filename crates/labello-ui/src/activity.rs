use eframe::egui;
use labello_client::CurrentUserActivity;
use labello_domain::{DatasetId, Timestamp, UserId, UtcActivityWindow};
use web_time::{Duration, Instant};

use crate::{LabelloApp, app::UiCommand, live_protocol::RequestIdentity};

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct ActivityState {
    pub identity: Option<(String, UserId, DatasetId)>,
    pub value: Option<CurrentUserActivity>,
    pub pending_request: Option<u64>,
    pub last_attempt: Option<Instant>,
    pub server_clock: Option<(Timestamp, Instant)>,
    pub error: Option<String>,
    pub refresh_after_load: bool,
}

impl ActivityState {
    fn expected_window(&self) -> Option<UtcActivityWindow> {
        self.server_clock.map(|(server_time, received)| {
            UtcActivityWindow::containing(
                server_time + chrono::Duration::from_std(received.elapsed()).unwrap_or_default(),
            )
        })
    }

    fn expire_previous_day(&mut self) -> bool {
        if self.value.as_ref().is_some_and(|value| {
            self.expected_window()
                .is_some_and(|window| value.window != window)
        }) {
            self.value = None;
            self.error = None;
            self.last_attempt = None;
            return true;
        }
        false
    }
}

impl LabelloApp {
    pub(crate) fn activity_available(&self) -> bool {
        self.work_view()
            && self.runtime.api.is_some()
            && self
                .auth
                .account
                .as_ref()
                .is_some_and(|account| account.user_id == self.config.user_id)
            && self
                .datasets
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.dataset_id == self.config.dataset_id)
    }

    pub(crate) fn refresh_activity_if_due(&mut self, ctx: &egui::Context) {
        if !self.activity_available() {
            return;
        }
        let identity = (
            self.config.api_base_url.clone(),
            self.config.user_id.clone(),
            self.config.dataset_id.clone(),
        );
        if self.datasets.activity.identity.as_ref() != Some(&identity) {
            self.datasets.activity = ActivityState {
                identity: Some(identity),
                ..Default::default()
            };
        }
        self.datasets.activity.expire_previous_day();
        if self
            .datasets
            .activity
            .last_attempt
            .is_none_or(|last| last.elapsed() >= REFRESH_INTERVAL)
        {
            self.request_activity();
        }
        let activity = &self.datasets.activity;
        if activity.pending_request.is_none() {
            ctx.request_repaint_after(
                activity
                    .last_attempt
                    .map(|last| REFRESH_INTERVAL.saturating_sub(last.elapsed()))
                    .unwrap_or(REFRESH_INTERVAL),
            );
        }
        if let Some((server_time, received)) = activity.server_clock {
            let window = UtcActivityWindow::containing(server_time);
            let until_midnight = (window.end - server_time)
                .to_std()
                .unwrap_or_default()
                .saturating_sub(received.elapsed());
            if !until_midnight.is_zero() {
                ctx.request_repaint_after(until_midnight);
            }
        }
    }

    pub(crate) fn request_activity(&mut self) {
        if !self.activity_available() || self.datasets.activity.pending_request.is_some() {
            return;
        }
        self.datasets.activity.identity = Some((
            self.config.api_base_url.clone(),
            self.config.user_id.clone(),
            self.config.dataset_id.clone(),
        ));
        let dataset_id = self.config.dataset_id.clone();
        let request = self.request_identity(Some(dataset_id.clone()));
        self.datasets.activity.pending_request = Some(request.request_id);
        self.datasets.activity.last_attempt = Some(Instant::now());
        self.queue_command(UiCommand::CurrentUserActivity {
            request,
            dataset_id,
        });
    }

    pub(crate) fn activity_work_completed(&mut self) {
        if self.datasets.activity.pending_request.is_some() {
            self.datasets.activity.refresh_after_load = true;
        } else {
            self.request_activity();
        }
    }

    pub(crate) fn accept_activity(
        &mut self,
        request: RequestIdentity,
        result: Result<CurrentUserActivity, String>,
    ) {
        let activity = &mut self.datasets.activity;
        if activity.pending_request != Some(request.request_id) {
            return;
        }
        activity.pending_request = None;
        activity.expire_previous_day();
        let transit_bound = chrono::Duration::from_std(
            activity
                .last_attempt
                .map(|started| started.elapsed())
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        match result {
            Ok(value)
                if activity.identity.as_ref()
                    == Some(&(
                        self.config.api_base_url.clone(),
                        self.config.user_id.clone(),
                        self.config.dataset_id.clone(),
                    ))
                    && value.dataset_id == self.config.dataset_id
                    && value.user_id == self.config.user_id
                    && value.window == UtcActivityWindow::containing(value.sampled_at)
                    && value.window.contains(value.sampled_at + transit_bound)
                    && activity
                        .expected_window()
                        .is_none_or(|window| value.window.start >= window.start) =>
            {
                // The round trip is a conservative upper bound on response transit.
                // Expiring slightly early is preferable to showing yesterday as today.
                activity.server_clock = Some((value.sampled_at + transit_bound, Instant::now()));
                activity.value = Some(value);
                activity.error = None;
            }
            Ok(_) => {
                activity.error = Some(
                    "Activity response did not match the current user, dataset or UTC day.".into(),
                );
            }
            Err(error) => activity.error = Some(error),
        }
        if std::mem::take(&mut activity.refresh_after_load) {
            self.request_activity();
        }
    }

    /// Browser visibility is only a refresh hint; the dataset owner coalesces it.
    pub fn activity_visibility_notifier(&self, ctx: egui::Context) -> std::rc::Rc<dyn Fn()> {
        let tx = self.runtime.tx.clone();
        std::rc::Rc::new(move || {
            let _ = tx.send(crate::app::UiMessage::ActivityVisibilityRegained);
            ctx.request_repaint();
        })
    }

    pub(crate) fn activity_retry_in_workspace(&self, ctx: &egui::Context) -> bool {
        self.activity_available()
            && self.datasets.activity.error.is_some()
            && Self::short_viewport(ctx.content_rect().size())
            && ((self.view == crate::app::AppView::Annotate && self.manual_migration_active())
                || (self.view == crate::app::AppView::Review
                    && crate::app::LayoutMode::for_width(ctx.content_rect().width())
                        == crate::app::LayoutMode::Compact))
    }

    pub(crate) fn activity_summary(&mut self, ui: &mut egui::Ui) {
        if !self.activity_available() {
            return;
        }
        let retry_in_workspace = self.activity_retry_in_workspace(ui.ctx());
        let activity = &self.datasets.activity;
        let refreshing = activity.pending_request.is_some();
        let error = activity.error.is_some();
        let mut full = if let Some(value) = &activity.value {
            format!(
                "Annotation tasks submitted today in UTC: {}. Final task reviews completed today in UTC: {}.{}{}",
                value.counts.annotation_tasks_submitted,
                value.counts.final_task_reviews,
                if refreshing { " Refreshing." } else { "" },
                if error {
                    " Stale values: refresh failed."
                } else {
                    ""
                }
            )
        } else if error {
            "Activity today in UTC is unavailable. Retry activity.".into()
        } else {
            "Loading activity today in UTC.".into()
        };
        if retry_in_workspace {
            full.push_str(" Use Retry activity in the workspace actions or More menu.");
        }
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal(|ui| {
            // Allocate the real retry control first; labels use the remaining width.
            if error
                && !retry_in_workspace
                && ui
                    .add_enabled(
                        !refreshing,
                        egui::Button::new("Retry").min_size(egui::vec2(44.0, 44.0)),
                    )
                    .on_hover_text("Retry activity for today in UTC")
                    .clicked()
            {
                self.request_activity();
            }
            let activity = &self.datasets.activity;
            let candidates = if let Some(value) = &activity.value {
                let a = value.counts.annotation_tasks_submitted;
                let r = value.counts.final_task_reviews;
                let suffix = if activity.error.is_some() {
                    " · stale"
                } else if refreshing {
                    " · refreshing"
                } else {
                    ""
                };
                vec![
                    format!("Today · {a} tasks submitted · {r} final reviews{suffix}"),
                    format!("Today · {a} submitted · {r} reviewed{suffix}"),
                    format!("Today · A {a} · R {r}{suffix}"),
                ]
            } else if activity.error.is_some() {
                vec!["Activity unavailable".into()]
            } else {
                vec!["Loading activity…".into()]
            };
            let font = egui::TextStyle::Body.resolve(ui.style());
            let label = candidates
                .iter()
                .find(|text| {
                    ui.painter()
                        .layout_no_wrap((*text).clone(), font.clone(), ui.visuals().text_color())
                        .size()
                        .x
                        <= ui.available_width()
                })
                .unwrap_or(candidates.last().unwrap());
            ui.add(egui::Label::new(label).wrap())
                .on_hover_text(&full)
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &full));
        });
    }
}
