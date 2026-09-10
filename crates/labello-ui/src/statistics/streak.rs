use eframe::egui;
use labello_domain::{DAILY_LABEL_GOAL, LabelStreak};

use crate::{LabelloApp, theme};

pub(super) fn badge_width(ui: &egui::Ui, streak: LabelStreak) -> f32 {
    let text = egui::WidgetText::from(format!("{} d", streak.days)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    20.0 + ui.spacing().item_spacing.x + text.size().x
}

pub(super) fn badge(ui: &mut egui::Ui, streak: LabelStreak, name: &str) {
    let flame_id = ui.id().with("streak-flame");
    let response = egui::AtomLayout::new((
        egui::Atom::custom(flame_id, egui::vec2(20.0, 22.0)),
        format!("{} d", streak.days),
    ))
    .wrap_mode(egui::TextWrapMode::Extend)
    .show(ui);
    if let Some(rect) = response.rect(flame_id) {
        paint(ui, rect, streak.lit(), 1.0);
    }
    let details = description(streak, name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &details));
    response.response.on_hover_text(details);
}

fn description(streak: LabelStreak, name: &str) -> String {
    format!(
        "{name}: {} day streak · {}/{DAILY_LABEL_GOAL} labels today · Flame {}. Complete {DAILY_LABEL_GOAL} distinct image/task submissions per UTC day in this dataset.",
        streak.days,
        streak.labeled_today,
        if streak.lit() { "lit" } else { "unlit" }
    )
}

fn paint(ui: &egui::Ui, rect: egui::Rect, lit: bool, scale: f32) {
    let color = if lit {
        theme::WARNING
    } else {
        theme::TEXT_MUTED
    };
    let center = rect.center();
    let point = |x: f32, y: f32| center + egui::vec2(x, y) * scale;
    let points = [
        (-6.0, 8.0),
        (-9.0, 2.0),
        (-7.0, -4.0),
        (-4.0, -1.0),
        (0.0, -11.0),
        (6.0, -4.0),
        (9.0, 2.0),
        (6.0, 8.0),
        (0.0, 11.0),
    ]
    .into_iter()
    .map(|(x, y)| point(x, y))
    .collect();
    ui.painter().add(egui::Shape::closed_line(
        points,
        egui::Stroke::new(2.0, color),
    ));
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            point(0.0, -2.0),
            point(4.0, 5.0),
            point(0.0, 9.0),
            point(-4.0, 5.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

impl LabelloApp {
    pub(crate) fn streak_available(&self) -> bool {
        self.auth.checked
            && self.auth.account.is_some()
            && (self
                .datasets
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.dataset_id == self.config.dataset_id)
                || self
                    .datasets
                    .summaries
                    .iter()
                    .any(|dataset| dataset.dataset_id == self.config.dataset_id))
    }

    pub(crate) fn statistics_refresh_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(if self.statistics_visible() { 3 } else { 30 })
    }

    pub(crate) fn streak_indicator(&mut self, ui: &mut egui::Ui) {
        if !self.streak_available() {
            return;
        }
        let today = labello_domain::now().date_naive();
        let streak = self.datasets.stats.contributors.as_ref().map(|people| {
            people
                .get(&self.config.user_id)
                .map(|person| person.label_streak(today))
                .unwrap_or_default()
        });
        let reliable = self.datasets.stats_error.is_none() && streak.is_some();
        let identity = egui::Id::new((
            "daily-flame",
            &self.config.dataset_id,
            &self.config.user_id,
            self.auth_epoch,
            self.workspace_epoch,
            today,
        ));
        let time = ui.input(|input| input.time);
        let reduced = ui.ctx().data(|data| {
            data.get_temp::<bool>(egui::Id::new("reduced-motion"))
                .unwrap_or(true)
        });
        let pulse = ui.ctx().data_mut(|data| {
            let (mut was_lit, mut started) = data
                .get_temp::<(Option<bool>, Option<f64>)>(identity)
                .unwrap_or((None, None));
            if reliable {
                let lit = streak.is_some_and(LabelStreak::lit);
                if lit && was_lit == Some(false) && !reduced {
                    started = Some(time);
                }
                was_lit = Some(lit);
            }
            if reduced || started.is_some_and(|start| time - start >= 0.7) {
                started = None;
            }
            data.insert_temp(identity, (was_lit, started));
            started.map(|start| ((time - start) / 0.7) as f32)
        });
        let details = match streak {
            Some(streak) => format!(
                "{}{}",
                description(streak, "Your streak"),
                if self.datasets.stats_error.is_some() {
                    " Last refresh failed; progress may be stale."
                } else if self.loading.stats {
                    " Refreshing progress…"
                } else {
                    ""
                }
            ),
            None => if self.loading.stats {
                "Loading daily streak…"
            } else {
                "Daily streak unavailable. Select to view statistics or retry."
            }
            .into(),
        };
        let flame_id = ui.id().with("daily-flame-icon");
        let response = egui::Button::new(egui::Atom::custom(flame_id, egui::vec2(24.0, 30.0)))
            .frame(false)
            .min_size(egui::vec2(44.0, 44.0))
            .atom_ui(ui);
        if let Some(rect) = response.rect(flame_id) {
            let scale =
                1.0 + pulse.map_or(0.0, |phase| 0.25 * (phase * std::f32::consts::PI).sin());
            paint(
                ui,
                rect,
                reliable && streak.is_some_and(LabelStreak::lit),
                scale,
            );
        }
        let response = response.response.on_hover_text(&details);
        if self.navigation.statistics.restore_focus == Some(response.id)
            && ui
                .ctx()
                .memory(|memory| memory.allows_interaction(response.layer_id))
        {
            response.request_focus();
            self.navigation.statistics.restore_focus = None;
        }
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &details));
        if response.clicked() {
            self.open_statistics();
            self.navigation.statistics.invoker = Some(response.id);
        }
        if pulse.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}
