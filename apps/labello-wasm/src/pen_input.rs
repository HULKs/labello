//! Browser pen input before eframe's compatibility mouse/touch listeners.
//!
//! eframe 0.35 reads pointer down/up but only mouse/touch movement. Own the
//! complete pen stream here so a browser need not emit compatibility movement,
//! and a stylus reported through both PointerEvent and TouchEvent is handled once.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2};
use wasm_bindgen::{JsCast, JsValue};

#[derive(Default)]
struct PenInput {
    active: Option<i32>,
    last_pen: bool,
    mouse_down: bool,
    pen_seen: bool,
    touches: BTreeMap<i32, Pos2>,
    touch_pointer: Option<i32>,
    position: Pos2,
    events: Vec<Event>,
}

impl PenInput {
    fn moved(&mut self, position: Pos2) {
        self.position = position;
        if let Some(Event::PointerMoved(previous)) = self.events.last_mut() {
            *previous = position;
        } else {
            self.events.push(Event::PointerMoved(position));
        }
    }

    fn button(&mut self, pressed: bool, modifiers: Modifiers) {
        self.events.push(Event::PointerButton {
            pos: self.position,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
    }

    fn cancel(&mut self) {
        if self.active.take().is_some() {
            self.button(false, Modifiers::NONE);
            // The shared canvas sees an unavailable pointer and discards the
            // preview before it can interpret the release as a committed edit.
            self.events.push(Event::PointerGone);
        }
    }
}

/// Target-specific input stays in the browser bootstrap; all rendering and
/// annotation policy still belongs to `LabelloApp` and its shared canvas.
pub struct BrowserApp {
    app: labello_ui::LabelloApp,
    input: Rc<RefCell<PenInput>>,
}

impl BrowserApp {
    pub fn new(
        app: labello_ui::LabelloApp,
        canvas: web_sys::HtmlCanvasElement,
        ctx: egui::Context,
        runner: &eframe::WebRunner,
    ) -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("missing window"))?;
        let document = window
            .document()
            .ok_or_else(|| JsValue::from_str("missing document"))?;
        let input = Rc::new(RefCell::new(PenInput::default()));
        for name in [
            "pointerdown",
            "pointermove",
            "pointerup",
            "pointercancel",
            "lostpointercapture",
            "mousedown",
            "mousemove",
            "mouseup",
            "mouseleave",
            "touchstart",
            "touchmove",
            "touchend",
            "touchcancel",
            "blur",
        ] {
            let target: web_sys::EventTarget = if name == "blur" {
                window.clone().into()
            } else {
                document.clone().into()
            };
            let input = input.clone();
            let canvas = canvas.clone();
            let ctx = ctx.clone();
            let options = web_sys::AddEventListenerOptions::new();
            options.set_capture(true);
            options.set_passive(false);
            runner.add_event_listener_ex(
                &target,
                name,
                &options,
                move |event: web_sys::Event, app_runner, _| {
                    handle_event(&mut input.borrow_mut(), &event, &canvas, &ctx);
                    let pen_button = matches!(event.type_().as_str(), "pointerdown" | "pointerup")
                        && event
                            .dyn_ref::<web_sys::PointerEvent>()
                            .is_some_and(|pointer| pointer.pointer_type() == "pen");
                    let touch_button = matches!(event.type_().as_str(), "touchstart" | "touchend")
                        && input
                            .borrow()
                            .events
                            .iter()
                            .any(|event| matches!(event, Event::PointerButton { .. }));
                    if pen_button || touch_button {
                        if touch_button {
                            let _ = canvas.focus();
                        }
                        // Preserve eframe's synchronous user-activation path for
                        // browser actions such as opening a picker or clipboard.
                        app_runner.logic();
                        if touch_button && event.type_() == "touchend" {
                            // Commit the release before clearing hover, matching
                            // eframe's touchend order. Gone in the same frame is
                            // an annotation cancellation, not a normal release.
                            input.borrow_mut().events.push(Event::PointerGone);
                            ctx.request_repaint();
                        }
                    }
                },
            )?;
        }
        Ok(Self { app, input })
    }
}

impl eframe::App for BrowserApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.input.borrow_mut().events);
        self.app.raw_input_hook(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
    }
}

fn consume(event: &web_sys::Event) {
    event.prevent_default();
    event.stop_immediate_propagation();
}

fn is_stylus_touch(contact: &web_sys::Touch) -> bool {
    js_sys::Reflect::get(contact, &JsValue::from_str("touchType"))
        .ok()
        .and_then(|value| value.as_string())
        .as_deref()
        == Some("stylus")
}

fn handle_event(
    input: &mut PenInput,
    event: &web_sys::Event,
    canvas: &web_sys::HtmlCanvasElement,
    ctx: &egui::Context,
) {
    let on_canvas = event
        .target()
        .is_some_and(|target| target.dyn_ref::<web_sys::HtmlCanvasElement>() == Some(canvas));
    if event.type_() == "blur" {
        input.cancel();
        input.mouse_down = false;
        if input.touch_pointer.take().is_some() {
            input.button(false, Modifiers::NONE);
            input.events.push(Event::PointerGone);
        }
        for (id, pos) in std::mem::take(&mut input.touches) {
            input
                .events
                .push(finger_event(id, pos, egui::TouchPhase::Cancel));
        }
        ctx.request_repaint();
    } else if let Some(pointer) = event.dyn_ref::<web_sys::PointerEvent>() {
        if pointer.pointer_type() != "pen" {
            if input.active.is_some() || pointer.pointer_type() == "touch" {
                consume(event);
            } else if pointer.pointer_type() == "mouse" {
                input.last_pen = false;
                labello_ui::pointer_input::set_pen_pointer(ctx, false);
                if event.type_() == "pointerdown" && on_canvas {
                    input.mouse_down = true;
                } else if event.type_() == "pointerup" || event.type_() == "pointercancel" {
                    input.mouse_down = false;
                }
            }
            return;
        }
        if !on_canvas && input.active != Some(pointer.pointer_id()) {
            return;
        }
        consume(event);
        input.last_pen = true;
        input.pen_seen = true;
        labello_ui::pointer_input::set_pen_pointer(ctx, true);
        if input.touch_pointer.take().is_some() {
            input.button(false, Modifiers::NONE);
            input.events.push(Event::PointerGone);
        }
        let rect = canvas.get_bounding_client_rect();
        let position = egui::pos2(
            (pointer.client_x() as f32 - rect.left() as f32) / ctx.zoom_factor(),
            (pointer.client_y() as f32 - rect.top() as f32) / ctx.zoom_factor(),
        );
        let modifiers = Modifiers {
            alt: pointer.alt_key(),
            ctrl: pointer.ctrl_key(),
            shift: pointer.shift_key(),
            mac_cmd: pointer.meta_key(),
            command: pointer.ctrl_key() || pointer.meta_key(),
        };
        match event.type_().as_str() {
            "pointerdown"
                if input.active.is_none()
                    && !input.mouse_down
                    && pointer.is_primary()
                    && pointer.button() == 0
                    && pointer.buttons() == 1 =>
            {
                input.active = Some(pointer.pointer_id());
                input.moved(position);
                input.button(true, modifiers);
                let _ = canvas.focus();
                let _ = canvas.set_pointer_capture(pointer.pointer_id());
            }
            "pointermove"
                if input.active == Some(pointer.pointer_id()) && pointer.buttons() != 1 =>
            {
                input.cancel();
            }
            "pointermove"
                if input.active == Some(pointer.pointer_id())
                    || (input.active.is_none() && pointer.buttons() == 0 && !input.mouse_down) =>
            {
                input.moved(position);
            }
            "pointerup" if input.active == Some(pointer.pointer_id()) => {
                input.moved(position);
                input.button(false, modifiers);
                input.active = None;
            }
            "pointercancel" | "lostpointercapture"
                if input.active == Some(pointer.pointer_id()) =>
            {
                input.cancel();
            }
            _ => {}
        }
        ctx.request_repaint();
    } else if let Some(touch) = event.dyn_ref::<web_sys::TouchEvent>() {
        if !on_canvas && input.touches.is_empty() {
            return;
        }
        consume(event);
        let phase = match event.type_().as_str() {
            "touchstart" => egui::TouchPhase::Start,
            "touchmove" => egui::TouchPhase::Move,
            "touchend" => egui::TouchPhase::End,
            _ => egui::TouchPhase::Cancel,
        };
        let rect = canvas.get_bounding_client_rect();
        for index in 0..touch.changed_touches().length() {
            let Some(contact) = touch.changed_touches().item(index) else {
                continue;
            };
            if is_stylus_touch(&contact) {
                continue;
            }
            let id = contact.identifier();
            let position = egui::pos2(
                (contact.client_x() as f32 - rect.left() as f32) / ctx.zoom_factor(),
                (contact.client_y() as f32 - rect.top() as f32) / ctx.zoom_factor(),
            );
            input.events.push(finger_event(id, position, phase));
            match phase {
                egui::TouchPhase::Start => {
                    if input.touches.is_empty() && input.active.is_none() && !input.mouse_down {
                        // egui needs a pointer position to initialize a pinch, even
                        // when fingers do not own the annotation pointer. A cancelled
                        // pen may have cleared it. Never move an active pen's pointer.
                        input.moved(position);
                        if !input.pen_seen || !labello_ui::pointer_input::on_canvas(ctx, position) {
                            input.touch_pointer = Some(id);
                            input.last_pen = false;
                            labello_ui::pointer_input::set_pen_pointer(ctx, false);
                            input.button(true, Modifiers::NONE);
                        }
                    }
                    input.touches.insert(id, position);
                }
                egui::TouchPhase::Move => {
                    input.touches.insert(id, position);
                    if input.touch_pointer == Some(id) {
                        input.moved(position);
                    }
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                    input.touches.remove(&id);
                    if input.touch_pointer == Some(id) {
                        input.moved(position);
                        input.button(false, Modifiers::NONE);
                        if phase == egui::TouchPhase::Cancel {
                            input.events.push(Event::PointerGone);
                        }
                        input.touch_pointer = None;
                    }
                }
            }
        }
        ctx.request_repaint();
    } else if event.dyn_ref::<web_sys::MouseEvent>().is_some()
        && (input.last_pen || input.active.is_some())
    {
        consume(event);
    }
}

fn finger_event(id: i32, pos: Pos2, phase: egui::TouchPhase) -> Event {
    Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(id as u64),
        phase,
        pos,
        force: None,
    }
}
