use egui::{Context, Id, Rect};

const DURATION: f64 = 0.45;

/// Presentation only: never changes the stored zoom, pan, or annotation geometry.
#[derive(Clone, Debug, Default)]
pub(super) struct ScanTransition {
    phase: Option<Id>,
    entered: bool,
    started: Option<f64>,
    progress: Option<f32>,
}

impl ScanTransition {
    pub(super) fn set_phase(&mut self, phase: Option<Id>) {
        if self.phase != phase {
            self.entered = phase.is_some();
            self.phase = phase;
            self.cancel();
        }
    }

    pub(super) fn cancel(&mut self) {
        self.started = None;
        self.progress = None;
    }

    pub(super) fn update(&mut self, ctx: &Context) {
        let reduced = ctx.data(|data| {
            data.get_temp::<bool>(Id::new("reduced-motion"))
                .unwrap_or(true)
        });
        let (time, interrupted) = ctx.input(|input| {
            (
                input.time,
                input.pointer.any_down()
                    || input.smooth_scroll_delta != egui::Vec2::ZERO
                    || input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key { pressed: true, .. }
                                | egui::Event::MouseWheel { .. }
                                | egui::Event::Zoom(_)
                                | egui::Event::Touch { .. }
                        )
                    }),
            )
        });
        let entered = std::mem::take(&mut self.entered);
        if reduced {
            self.cancel();
            return;
        }
        if entered {
            self.started = Some(time);
        } else if interrupted {
            self.cancel();
        }
        self.progress = self.started.map(|start| ((time - start) / DURATION) as f32);
        if self.progress.is_some_and(|progress| progress >= 1.0) {
            self.cancel();
        }
        if self.progress.is_some() {
            ctx.request_repaint();
        }
    }

    pub(super) fn image_rect(&self, rect: Rect) -> Rect {
        let scale = self.progress.map_or(1.0, |progress| {
            // One pronounced contraction, one small settling contraction. Stay
            // inside the fitted image bounds so edge objects remain visible.
            1.0 - 0.14 * (1.0 - progress).powi(2) * (progress * std::f32::consts::TAU).sin().abs()
        });
        Rect::from_center_size(rect.center(), rect.size() * scale)
    }

    pub(super) fn emphasis(&self) -> f32 {
        self.progress.map_or(0.0, |progress| 1.0 - progress)
    }
}
