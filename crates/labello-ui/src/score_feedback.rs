use eframe::egui;
use labello_domain::{Assignment, AssignmentId, AssignmentKind, FocusWindow, ImageId};

use crate::{
    app::LabelloApp,
    live_protocol::{RequestIdentity, UiCommand, UiRequestError},
    statistics::score,
    theme,
};

const DURATION: f64 = 1.1;

struct ScoreWork {
    assignment_id: AssignmentId,
    image_id: ImageId,
    after_sequence: u64,
}

struct PendingScore {
    request_id: u64,
    image_id: ImageId,
    advanced: bool,
    hundredths: Option<i64>,
    expires_at: f64,
}

enum FeedbackContent {
    Score(i64),
    Focus {
        name: String,
        minutes: u64,
        kind: AssignmentKind,
    },
}

struct FloatingFeedback {
    content: FeedbackContent,
    started_at: f64,
}

impl FloatingFeedback {
    fn duration(&self) -> f64 {
        match self.content {
            FeedbackContent::Score(_) => DURATION,
            FeedbackContent::Focus { .. } => 2.0,
        }
    }
}

#[derive(Default)]
pub(crate) struct ScoreFeedback {
    active: Option<ScoreWork>,
    pending: Option<PendingScore>,
    floating: Option<FloatingFeedback>,
    focus_notice: Option<FloatingFeedback>,
    observed_focus: Option<(AssignmentKind, FocusWindow)>,
}

impl ScoreFeedback {
    #[cfg(feature = "inspector-presets")]
    pub(crate) fn preview(time: f64) -> Self {
        Self {
            floating: Some(FloatingFeedback {
                content: FeedbackContent::Score(2725),
                started_at: time,
            }),
            ..Default::default()
        }
    }

    pub(crate) fn loaded(&mut self, assignment: &Assignment, sequence: u64, time: f64) {
        if self
            .active
            .as_ref()
            .is_none_or(|work| work.assignment_id != assignment.assignment_id)
        {
            self.active = Some(ScoreWork {
                assignment_id: assignment.assignment_id.clone(),
                image_id: assignment.image_id.clone(),
                after_sequence: sequence,
            });
        }
        if let Some(pending) = &mut self.pending {
            if pending.advanced || pending.image_id == assignment.image_id {
                self.pending = None;
            } else {
                pending.advanced = true;
            }
        }
        self.start_if_ready(time);
    }

    fn start_if_ready(&mut self, time: f64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| time >= pending.expires_at)
        {
            self.pending = None;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.advanced && pending.hundredths.is_some())
        {
            let pending = self.pending.take().unwrap();
            if let Some(hundredths) = pending.hundredths.filter(|value| *value > 0) {
                self.floating = Some(FloatingFeedback {
                    content: FeedbackContent::Score(hundredths),
                    started_at: time,
                });
            }
        }
    }
}

impl LabelloApp {
    pub(crate) fn request_image_score(&mut self, ctx: &egui::Context, through_sequence: u64) {
        let Some(work) = self.work.score_feedback.active.take() else {
            return;
        };
        if !self
            .work
            .assignment
            .as_ref()
            .is_some_and(|assignment| assignment.assignment_id == work.assignment_id)
            || through_sequence <= work.after_sequence
        {
            return;
        }
        let dataset_id = self.config.dataset_id.clone();
        let request = self.request_identity(Some(dataset_id.clone()));
        self.work.score_feedback.pending = Some(PendingScore {
            request_id: request.request_id,
            image_id: work.image_id.clone(),
            advanced: false,
            hundredths: None,
            expires_at: ctx.input(|input| input.time) + 10.0,
        });
        self.queue_command(UiCommand::ImageScore {
            request,
            dataset_id,
            image_id: work.image_id,
            after_sequence: work.after_sequence,
            through_sequence,
        });
    }

    pub(crate) fn accept_image_score(
        &mut self,
        ctx: &egui::Context,
        request: RequestIdentity,
        result: Result<labello_client::ImageScore, UiRequestError>,
    ) {
        let feedback = &mut self.work.score_feedback;
        let Some(pending) = &mut feedback.pending else {
            return;
        };
        if pending.request_id != request.request_id {
            return;
        }
        match result {
            Ok(receipt) if receipt.hundredths > 0 => pending.hundredths = Some(receipt.hundredths),
            // Feedback is optional, including when connected to an older server.
            _ => {
                feedback.pending = None;
            }
        }
        feedback.start_if_ready(ctx.input(|input| input.time));
    }

    pub(crate) fn cancel_score_request(&mut self) {
        self.work.score_feedback.pending = None;
    }

    pub(crate) fn current_scoring_focus(&self) -> Option<&FocusWindow> {
        match self.view {
            crate::app::AppView::Annotate => self.datasets.stats.scoring_focus.as_ref(),
            crate::app::AppView::Review => self.datasets.stats.review_scoring_focus.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn focused_workflow_blocked(&self) -> bool {
        self.current_scoring_focus()
            .and_then(|focus| focus.task_id.as_ref())
            .is_some_and(|task| {
                self.workflow_availability(task) == Some(false)
                    && self.work.availability.reasons.get(task)
                        == Some(&labello_domain::WorkflowUnavailableReason::BalanceLimit)
            })
    }

    pub(crate) fn refresh_blocked_focus(&mut self) {
        if self.focused_workflow_blocked() {
            self.work.score_feedback.focus_notice = None;
            self.request_stats();
        }
    }

    pub(crate) fn observe_scoring_focus(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.assignment_kind() else {
            return;
        };
        let focus = self
            .current_scoring_focus()
            .filter(|focus| focus.contains(labello_domain::now()))
            .cloned();
        let identity = focus.clone().map(|focus| (kind.clone(), focus));
        if self.work.score_feedback.observed_focus == identity {
            return;
        }
        self.work.score_feedback.observed_focus = identity;
        self.work.score_feedback.focus_notice = focus.and_then(|focus| {
            let id = focus.task_id?;
            let task = self.work.tasks.iter().find(|task| task.task_id == id)?;
            Some(FloatingFeedback {
                content: FeedbackContent::Focus {
                    name: task.name.clone(),
                    kind,
                    minutes: ((focus.ends_at - labello_domain::now())
                        .num_milliseconds()
                        .max(0) as u64)
                        .div_ceil(60_000),
                },
                started_at: ctx.input(|input| input.time),
            })
        });
    }

    pub(crate) fn score_feedback(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        if self.datasets.stats_error.is_some()
            || self.focused_workflow_blocked()
            || self
                .current_scoring_focus()
                .is_none_or(|focus| !focus.contains(labello_domain::now()))
        {
            self.work.score_feedback.focus_notice = None;
        }
        let feedback = &mut self.work.score_feedback;
        feedback.start_if_ready(now);
        for notice in [&mut feedback.floating, &mut feedback.focus_notice] {
            if notice
                .as_ref()
                .is_some_and(|notice| now - notice.started_at >= notice.duration())
            {
                *notice = None;
            }
        }
        if !self.work_view()
            || self.statistics_visible()
            || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let feedback = &self.work.score_feedback;
        if let Some(notice) = &feedback.floating {
            paint_feedback(ctx, notice, now, 42.0);
        }
        if let Some(notice) = &feedback.focus_notice {
            paint_feedback(ctx, notice, now, -42.0);
        }
    }
}

fn paint_feedback(ctx: &egui::Context, notice: &FloatingFeedback, now: f64, offset: f32) {
    let duration = notice.duration();
    let elapsed = now - notice.started_at;
    let reduced = ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("reduced-motion"))
            .unwrap_or(true)
    });
    let progress = (elapsed / duration).clamp(0.0, 1.0) as f32;
    let pop = if reduced {
        0.0
    } else {
        (1.0 - progress / 0.25).max(0.0)
    };
    let scale = 1.0 + 0.18 * (pop * std::f32::consts::PI).sin();
    let opacity = if reduced {
        1.0
    } else {
        ((1.0 - progress) / 0.4).min(1.0)
    };
    let screen = ctx.content_rect();
    let drift = if reduced { 0.0 } else { 34.0 * progress };
    let anchor = egui::pos2(screen.right() - 16.0, screen.center().y + offset - drift);
    let (id, text, caption, accessible, accent) = match &notice.content {
        FeedbackContent::Score(hundredths) => (
            "score-gain-feedback",
            format!("+{}", score::compact(*hundredths)),
            "SCORE".to_owned(),
            format!("Score gained: +{} points", score::exact(*hundredths)),
            theme::ACCENT,
        ),
        FeedbackContent::Focus {
            name,
            minutes,
            kind,
        } => {
            let stage = if *kind == AssignmentKind::Review {
                "Review"
            } else {
                "Annotation"
            };
            (
                "focus-change-feedback",
                "×1.5".to_owned(),
                format!("Focus · {name}"),
                format!("{stage} focus changed: {name}, ×1.5 points, {minutes} min left"),
                theme::WARNING,
            )
        }
    };
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_CENTER, anchor - screen.right_center())
        .interactable(false)
        .show(ctx, |ui| {
            let color = accent.gamma_multiply(opacity);
            let galley =
                ui.painter()
                    .layout_no_wrap(text, egui::FontId::monospace(28.0 * scale), color);
            let width = if matches!(notice.content, FeedbackContent::Focus { .. }) {
                180.0_f32.min(screen.width() * 0.5 - 16.0)
            } else {
                (galley.size().x + 24.0).min(screen.width() - 24.0)
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, 62.0), egui::Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &accessible)
            });
            let painter = ui.painter_at(rect);
            painter.rect_filled(
                rect,
                theme::CONTROL_RADIUS,
                theme::PANEL.gamma_multiply(opacity),
            );
            painter.rect_filled(
                egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height())),
                theme::CONTROL_RADIUS,
                color,
            );
            let shape = egui::epaint::TextShape::new(
                rect.center() - galley.size() * 0.5 - egui::vec2(0.0, 5.0),
                galley,
                color,
            )
            .with_angle_and_anchor(-0.06 * pop, egui::Align2::CENTER_CENTER);
            painter.add(shape);
            let mut caption = egui::text::LayoutJob::simple_singleline(
                caption,
                egui::FontId::monospace(10.0),
                theme::TEXT_MUTED.gamma_multiply(opacity),
            );
            caption.wrap.max_width = (rect.width() - 16.0).max(1.0);
            caption.wrap.max_rows = 1;
            caption.wrap.overflow_character = Some('…');
            let caption = ui.fonts_mut(|fonts| fonts.layout_job(caption));
            painter.galley(
                rect.center_bottom()
                    - egui::vec2(caption.size().x / 2.0, 10.0 + caption.size().y / 2.0),
                caption,
                theme::TEXT_MUTED,
            );
        });
    ctx.request_repaint_after(if reduced {
        std::time::Duration::from_secs_f64((duration - elapsed).max(0.0))
    } else {
        std::time::Duration::from_millis(16)
    });
}
