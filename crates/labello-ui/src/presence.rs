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

/// Integrations without a motion preference adapter keep decorative motion off.
pub fn set_reduced_motion(ctx: &egui::Context, reduced: bool) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("reduced-motion"), reduced));
    ctx.request_repaint();
}

fn presence_text(ui: &egui::Ui, text: &str, animate: bool) -> std::sync::Arc<egui::Galley> {
    let mut galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        theme::TEXT_MUTED,
    );
    let reduced = ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::new("reduced-motion"))
            .unwrap_or(true)
    });
    if !animate || reduced || !ui.is_visible() {
        return galley;
    }
    // Two seconds of smooth brightening, then six seconds with no motion.
    let phase = ui.input(|input| input.time).rem_euclid(8.0);
    if phase >= 2.0 {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(8.0 - phase));
        return galley;
    }
    ui.ctx().request_repaint_after(Duration::from_millis(33));
    brighten_presence_text(std::sync::Arc::make_mut(&mut galley), phase as f32 / 2.0);
    galley
}

fn brighten_presence_text(galley: &mut egui::Galley, progress: f32) {
    let band = 28.0;
    let center = egui::lerp(-band..=galley.size().x + band, progress);
    for placed in &mut galley.rows {
        let row = std::sync::Arc::make_mut(&mut placed.row);
        for vertex in &mut row.visuals.mesh.vertices {
            // Tilt the band slightly while leaving glyph geometry and UVs untouched.
            let x = vertex.pos.x + placed.pos.x + (vertex.pos.y + placed.pos.y) * 0.2;
            let distance = ((x - center) / band).abs().min(1.0);
            let weight = (1.0 + (distance * std::f32::consts::PI).cos()) * 0.5;
            vertex.color = theme::TEXT_MUTED.lerp_to_gamma(theme::TEXT, weight * 0.22);
        }
    }
}

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
        self.work_view()
            && self.runtime.api.is_some()
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
        let names = users
            .iter()
            .map(PresentUser::presence_name)
            .collect::<Vec<_>>()
            .join(" · ");
        let mut shows_names = false;
        let label = if state.failures >= 3 {
            "Presence unavailable".to_owned()
        } else if state.value.is_none() {
            "Checking presence…".to_owned()
        } else if users.is_empty() {
            "No active labellers".to_owned()
        } else {
            let font = egui::TextStyle::Body.resolve(ui.style());
            if ui
                .painter()
                .layout_no_wrap(names.clone(), font, theme::TEXT_MUTED)
                .size()
                .x
                <= ui.available_width() - 2.0 * ui.spacing().button_padding.x
            {
                shows_names = true;
                names
            } else {
                if users.len() == 1 {
                    "1 person is labelling".into()
                } else {
                    format!("{} people are labelling", users.len())
                }
            }
        };
        let full = if state.failures < 3 && !users.is_empty() {
            users.iter().map(user_detail).collect::<Vec<_>>().join("\n")
        } else {
            label.clone()
        };
        let label_width = ui
            .painter()
            .layout_no_wrap(
                label.clone(),
                egui::TextStyle::Body.resolve(ui.style()),
                theme::TEXT_MUTED,
            )
            .size()
            .x
            + 2.0 * ui.spacing().button_padding.x;
        let text: egui::WidgetText = if shows_names {
            presence_text(ui, &label, true).into()
        } else {
            egui::RichText::new(&label).color(theme::TEXT_MUTED).into()
        };
        let response = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("workspace-presence")),
                |ui| {
                    ui.add_sized(
                        [label_width.min(ui.available_width()), 44.0],
                        egui::Button::new(text).frame(false).truncate(),
                    )
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
        egui::Popup::menu(&response).show(|ui| {
            workspace_status_details(ui, &full);
        });
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
        .max_height((ui.ctx().content_rect().height() * 0.6).max(44.0))
        .show(ui, |ui| {
            ui.add(egui::Label::new(details).wrap());
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ctx: &egui::Context, time: f64, animate: bool) -> std::sync::Arc<egui::Galley> {
        let mut text = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ui| text = Some(presence_text(ui, "@octocat · local_reviewer", animate)),
        );
        text.unwrap()
    }

    #[test]
    fn presence_sweep_changes_only_glyph_color_and_honors_motion_changes() {
        let ctx = egui::Context::default();
        let baseline = sample(&ctx, 0.0, true);
        // Unknown integration preferences keep text static.
        assert_eq!(baseline, sample(&ctx, 1.0, true));
        set_reduced_motion(&ctx, false);
        let animated = sample(&ctx, 1.1, true);
        assert_ne!(baseline, animated);
        assert_eq!(baseline.job, animated.job);
        assert_eq!(baseline.rect, animated.rect);
        assert_eq!(baseline.mesh_bounds, animated.mesh_bounds);
        for (before, after) in baseline.rows.iter().zip(&animated.rows) {
            assert_eq!(before.pos, after.pos);
            assert_eq!(before.glyphs, after.glyphs);
            assert_eq!(before.visuals.mesh.indices, after.visuals.mesh.indices);
            for (a, b) in before
                .visuals
                .mesh
                .vertices
                .iter()
                .zip(&after.visuals.mesh.vertices)
            {
                assert_eq!(a.pos, b.pos);
                assert_eq!(a.uv, b.uv);
                for (base, color) in a.color.to_array().into_iter().zip(b.color.to_array()) {
                    assert!(color >= base && color <= base.saturating_add(18));
                }
            }
        }
        assert_eq!(
            baseline,
            sample(&ctx, 1.2, false),
            "counts and status text stay static"
        );
        assert_eq!(baseline, sample(&ctx, 4.0, true), "idle interval is static");
        set_reduced_motion(&ctx, true);
        assert_eq!(
            baseline,
            sample(&ctx, 9.1, true),
            "preference changes stop the sweep"
        );
    }
}
