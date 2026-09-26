use eframe::egui;
use labello_client::{PresentUser, ServerPresence};
use labello_domain::UserId;
use web_time::{Duration, Instant};

use crate::{
    LabelloApp,
    app::{SaveStatus, UiCommand, UiMessage, UiRequestError},
    live_protocol::RequestIdentity,
    theme,
};

const INTERVAL: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct PresenceState {
    pub identity: Option<(String, UserId)>,
    pub value: Option<ServerPresence>,
    pub pending_request: Option<u64>,
    pub last_attempt: Option<Instant>,
    pub failures: u32,
    pub error: Option<String>,
}

impl PresenceState {
    pub fn failed(&mut self, error: String) {
        self.failures = self.failures.saturating_add(1);
        self.error = Some(error);
    }
}

impl LabelloApp {
    fn presence_available(&self) -> bool {
        self.runtime.api.is_some()
            && self
                .auth
                .account
                .as_ref()
                .is_some_and(|a| a.user_id == self.config.user_id)
    }

    pub(crate) fn refresh_presence_if_due(&mut self, ctx: &egui::Context) {
        if !self.presence_available() {
            return;
        }
        let identity = (
            self.config.api_base_url.clone(),
            self.config.user_id.clone(),
        );
        if self.runtime.presence.identity.as_ref() != Some(&identity) {
            self.runtime.presence = PresenceState {
                identity: Some(identity),
                ..Default::default()
            };
        }
        if self
            .runtime
            .presence
            .last_attempt
            .is_none_or(|last| last.elapsed() >= INTERVAL)
        {
            self.request_presence();
        }
        if self.runtime.presence.pending_request.is_none() {
            let wait = self
                .runtime
                .presence
                .last_attempt
                .map(|last| INTERVAL.saturating_sub(last.elapsed()))
                .unwrap_or(INTERVAL);
            ctx.request_repaint_after(wait);
        }
    }

    pub(crate) fn request_presence(&mut self) {
        if !self.presence_available() || self.runtime.presence.pending_request.is_some() {
            return;
        }
        let request = self.request_identity(None);
        self.runtime.presence.identity = Some((
            self.config.api_base_url.clone(),
            self.config.user_id.clone(),
        ));
        self.runtime.presence.pending_request = Some(request.request_id);
        self.runtime.presence.last_attempt = Some(Instant::now());
        self.queue_command(UiCommand::Presence { request });
    }

    pub(crate) fn accept_presence(
        &mut self,
        request: RequestIdentity,
        result: Result<ServerPresence, UiRequestError>,
    ) {
        let state = &mut self.runtime.presence;
        if state.pending_request != Some(request.request_id)
            || state.identity.as_ref()
                != Some(&(
                    self.config.api_base_url.clone(),
                    self.config.user_id.clone(),
                ))
        {
            return;
        }
        state.pending_request = None;
        match result {
            Ok(mut value) => {
                value.users.sort_by(|a, b| a.user_id.cmp(&b.user_id));
                value.users.dedup_by(|a, b| a.user_id == b.user_id);
                state.value = Some(value);
                state.failures = 0;
                state.error = None;
            }
            Err(error) => state.failed(error.to_string()),
        }
    }

    pub fn presence_visibility_notifier(&self, ctx: egui::Context) -> std::rc::Rc<dyn Fn()> {
        let tx = self.runtime.tx.clone();
        std::rc::Rc::new(move || {
            let _ = tx.send(UiMessage::PresenceVisibilityRegained);
            ctx.request_repaint();
        })
    }

    pub(crate) fn presence_summary(&self, ui: &mut egui::Ui) {
        let state = &self.runtime.presence;
        let users = state
            .value
            .as_ref()
            .map(|v| v.users.as_slice())
            .unwrap_or_default();
        let label = if state.failures >= 3 {
            Some("Presence unavailable")
        } else if state.value.is_none() {
            Some("Checking presence…")
        } else if users.is_empty() {
            Some("No active labellers")
        } else {
            None
        };
        let full = label
            .map(str::to_owned)
            .unwrap_or_else(|| users.iter().map(user_detail).collect::<Vec<_>>().join("\n"));
        let response = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("workspace-presence")),
                |ui| {
                    if let Some(label) = label {
                        let width = ui
                            .painter()
                            .layout_no_wrap(
                                label.to_owned(),
                                egui::TextStyle::Body.resolve(ui.style()),
                                theme::TEXT_MUTED,
                            )
                            .size()
                            .x
                            + 2.0 * ui.spacing().button_padding.x;
                        ui.add_sized(
                            [width.min(ui.available_width()), 44.0],
                            egui::Button::new(egui::RichText::new(label).color(theme::TEXT_MUTED))
                                .frame(false)
                                .truncate(),
                        )
                    } else {
                        presence_avatars(ui, users)
                    }
                },
            )
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Labelling presence: {full}"),
            )
        });
        response
            .clone()
            .on_hover_ui(|ui| workspace_status_details(ui, &full));
        let popup = egui::Popup::menu(&response);
        let restore_focus =
            popup.is_open() && ui.input(|input| input.key_pressed(egui::Key::Escape));
        popup.show(|ui| {
            workspace_status_details(ui, &full);
        });
        if restore_focus {
            response.request_focus();
        }
    }

    pub(crate) fn connection_status(&self) -> (egui::Color32, String) {
        let state = &self.runtime.presence;
        let connection = if state.failures >= 3 {
            "Disconnected"
        } else if state.failures > 0 {
            "Reconnecting"
        } else if state.value.is_none() {
            "Checking connection"
        } else {
            "Connected"
        };
        let status = match self.work.save_status {
            SaveStatus::Idle => "Idle",
            SaveStatus::Dirty => "Unsaved edits",
            SaveStatus::Saved => "Saved",
            SaveStatus::Saving => "Saving",
            SaveStatus::Retry => "Save failed. Retry saving.",
        };
        let mut detail = format!("{connection}\n{status}");
        for error in [
            state.error.as_ref(),
            self.runtime.storage_error.as_ref(),
            self.runtime.error.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            detail.push('\n');
            detail.push_str(error);
        }
        if let Some(notice) = &self.runtime.notice {
            detail.push('\n');
            detail.push_str(notice);
        }
        let color = if state.failures >= 3
            || self.runtime.error.is_some()
            || self.runtime.storage_error.is_some()
            || self.work.save_status == SaveStatus::Retry
        {
            theme::DANGER
        } else if state.failures > 0
            || state.value.is_none()
            || matches!(
                self.work.save_status,
                SaveStatus::Dirty | SaveStatus::Saving
            )
        {
            theme::WARNING
        } else {
            theme::SUCCESS
        };
        (color, detail)
    }

    pub(crate) fn connection_indicator(&self, ui: &mut egui::Ui) {
        let (color, detail) = self.connection_status();
        let response = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("workspace-connection-status")),
                |ui| ui.add_sized([44.0, 44.0], egui::Button::new("").frame(false)),
            )
            .inner;
        ui.painter()
            .circle_filled(response.rect.center(), 4.5, color);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Connection status: {detail}"),
            )
        });
        response
            .clone()
            .on_hover_ui(|ui| workspace_status_details(ui, &detail));
        egui::Popup::menu(&response).show(|ui| {
            workspace_status_details(ui, &detail);
        });
    }
}

fn user_detail(user: &PresentUser) -> String {
    format!(
        "{}: {}",
        user.presence_name(),
        user.datasets
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn workspace_status_details(ui: &mut egui::Ui, details: &str) {
    ui.set_max_width((ui.ctx().content_rect().width() - 32.0).clamp(44.0, 320.0));
    egui::ScrollArea::vertical()
        .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
        .max_height((ui.ctx().content_rect().height() * 0.6).max(44.0))
        .show(ui, |ui| {
            ui.add(egui::Label::new(details).wrap());
        });
}

// One stable focus target exposes every user, including those hidden by overflow.
fn presence_avatars(ui: &mut egui::Ui, users: &[PresentUser]) -> egui::Response {
    const SIZE: f32 = 28.0;
    const GAP: f32 = 4.0;
    let padding = ui.spacing().button_padding.x;
    let available = (ui.available_width() - 2.0 * padding).max(0.0);
    let mut visible = users.len().min(((available + GAP) / (SIZE + GAP)) as usize);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let (overflow, width) = loop {
        let hidden = users.len() - visible;
        let overflow = (hidden > 0).then(|| {
            ui.painter()
                .layout_no_wrap(format!("+{hidden}"), font.clone(), theme::TEXT_MUTED)
        });
        let avatars_width = visible as f32 * (SIZE + GAP) - if visible > 0 { GAP } else { 0.0 };
        let width = avatars_width
            + overflow.as_ref().map_or(0.0, |text| {
                text.size().x + if visible > 0 { GAP } else { 0.0 }
            });
        if width <= available || visible == 0 {
            break (overflow, width);
        }
        visible -= 1;
    };
    let response = ui.add_sized(
        [
            (width + 2.0 * padding).max(44.0).min(ui.available_width()),
            44.0,
        ],
        egui::Button::new("").frame(false),
    );
    let painter = ui
        .painter()
        .with_clip_rect(response.rect.intersect(ui.clip_rect()));
    let mut avatar_ui = ui.new_child(egui::UiBuilder::new().max_rect(response.rect));
    avatar_ui.set_clip_rect(painter.clip_rect());
    let mut x = response.rect.center().x - width / 2.0;
    for user in &users[..visible] {
        let rect = egui::Rect::from_center_size(
            egui::pos2(x + SIZE / 2.0, response.rect.center().y),
            egui::Vec2::splat(SIZE),
        );
        crate::avatar::paint(
            &avatar_ui,
            user.github_user_id.as_deref(),
            &user.presence_name(),
            rect,
        );
        x += SIZE + GAP;
    }
    if let Some(text) = overflow {
        painter.galley(
            egui::pos2(x, response.rect.center().y - text.size().y / 2.0),
            text,
            theme::TEXT_MUTED,
        );
    }
    response
}
