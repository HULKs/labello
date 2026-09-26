//! Pointer identity supplied by platform adapters, separate from finger gestures.
use egui::{
    Context, Id, LayerId, Pos2, Rect,
    scroll_area::{DragScroll, ScrollSource},
};

pub fn set_pen_pointer(ctx: &Context, pen: bool) {
    let click_duration = ctx.options(|options| options.input_options.max_click_duration);
    let duration = ctx.data_mut(|data| {
        data.insert_temp(Id::new("pen-pointer"), pen);
        let saved = Id::new("pen-saved-click-duration");
        if pen {
            if data.get_temp::<f64>(saved).is_none() {
                data.insert_temp(saved, click_duration);
            }
            Some(f64::INFINITY)
        } else {
            let duration = data.get_temp::<f64>(saved);
            data.remove::<f64>(saved);
            duration
        }
    });
    if let Some(duration) = duration {
        // egui treats any held primary pointer as a touch long-press while
        // fingers are down, which steals a stationary pen's drag ownership.
        ctx.options_mut(|options| options.input_options.max_click_duration = duration);
    }
}

pub fn pen_pointer(ctx: &Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(Id::new("pen-pointer"))
            .unwrap_or(false)
    })
}

pub(crate) fn set_canvas_rect(ctx: &Context, rect: Rect, layer: LayerId) {
    ctx.data_mut(|data| data.insert_temp(Id::new("input-canvas"), (rect, layer)));
}

pub fn on_canvas(ctx: &Context, position: Pos2) -> bool {
    ctx.data(|data| data.get_temp::<(Rect, LayerId)>(Id::new("input-canvas")))
        .is_some_and(|(rect, layer)| {
            rect.contains(position) && ctx.layer_id_at(position) == Some(layer)
        })
        && ctx.memory(|memory| memory.top_modal_layer().is_none())
}

pub(crate) fn scroll_source(ctx: &Context) -> ScrollSource {
    ScrollSource {
        drag: if pen_pointer(ctx) {
            DragScroll::Never
        } else {
            DragScroll::OnTouch
        },
        ..ScrollSource::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, TouchDeviceId, TouchId, TouchPhase, pos2, vec2};
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn held_pen_keeps_drag_ownership_while_fingers_are_down() {
        let mut harness = Harness::builder()
            .with_size(vec2(300.0, 200.0))
            .build_ui_state(
                |ui, held: &mut bool| {
                    *held = ui
                        .add(egui::Button::new("Canvas").sense(egui::Sense::click_and_drag()))
                        .is_pointer_button_down_on();
                },
                false,
            );
        let original_duration = harness
            .ctx
            .options(|options| options.input_options.max_click_duration);
        set_pen_pointer(&harness.ctx, true);
        let position = harness.get_by_label("Canvas").rect().center();
        harness.event(Event::PointerMoved(position));
        harness.event(Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        harness.event(Event::Touch {
            device_id: TouchDeviceId(1),
            id: TouchId(1),
            phase: TouchPhase::Start,
            pos: pos2(250.0, 150.0),
            force: None,
        });
        harness.step();
        harness.input_mut().time =
            Some(harness.ctx.input(|input| input.time) + original_duration + 1.0);
        harness.step();
        harness.step();
        assert!(
            *harness.state(),
            "finger contact must not turn the held pen into a touch long-press"
        );
        set_pen_pointer(&harness.ctx, false);
        assert_eq!(
            harness
                .ctx
                .options(|options| options.input_options.max_click_duration),
            original_duration
        );
    }

    #[test]
    fn pen_button_drag_does_not_scroll_after_touch_detection() {
        let mut harness = Harness::builder()
            .with_size(vec2(300.0, 200.0))
            .build_ui_state(
                |ui, offset: &mut f32| {
                    *offset = egui::ScrollArea::vertical()
                        .scroll_source(scroll_source(ui.ctx()))
                        .show(ui, |ui| {
                            for index in 0..20 {
                                let _ = ui.button(format!("Action {index}"));
                            }
                        })
                        .state
                        .offset
                        .y;
                },
                0.0,
            );
        for phase in [TouchPhase::Start, TouchPhase::End] {
            harness.event(Event::Touch {
                device_id: TouchDeviceId(1),
                id: TouchId(1),
                phase,
                pos: pos2(280.0, 180.0),
                force: None,
            });
            harness.step();
        }
        assert!(harness.ctx.input(|input| input.has_touch_screen()));
        set_pen_pointer(&harness.ctx, true);
        let start = harness.get_by_label("Action 4").rect().center();
        harness.event(Event::PointerMoved(start));
        harness.event(Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        harness.step();
        harness.event(Event::PointerMoved(start - vec2(0.0, 60.0)));
        harness.step();
        assert_eq!(*harness.state(), 0.0);
        harness.event(Event::PointerButton {
            pos: start - vec2(0.0, 60.0),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        harness.step();
        harness.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -100.0),
            modifiers: Modifiers::NONE,
            phase: TouchPhase::Move,
        });
        harness.run_steps(5);
        assert!(*harness.state() > 0.0);
    }
}
