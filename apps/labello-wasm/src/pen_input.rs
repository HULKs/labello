//! Browser pen input before eframe's compatibility mouse/touch listeners.
//!
//! eframe 0.35 reads pointer down/up but only mouse/touch movement. Own the
//! complete pen stream here so a browser need not emit compatibility movement,
//! and a stylus reported through both PointerEvent and TouchEvent is handled once.

use std::{cell::RefCell, rc::Rc};

use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2};
use wasm_bindgen::{JsCast, JsValue};

#[derive(Default)]
struct PenInput {
    active: Option<i32>,
    last_pen: bool,
    mouse_down: bool,
    touch_count: u32,
    suppress_touches: bool,
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
                    if matches!(event.type_().as_str(), "pointerdown" | "pointerup")
                        && event
                            .dyn_ref::<web_sys::PointerEvent>()
                            .is_some_and(|pointer| pointer.pointer_type() == "pen")
                    {
                        // Preserve eframe's synchronous user-activation path for
                        // browser actions such as opening a picker or clipboard.
                        app_runner.logic();
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
        input.touch_count = 0;
        input.suppress_touches = false;
        ctx.request_repaint();
    } else if let Some(pointer) = event.dyn_ref::<web_sys::PointerEvent>() {
        if pointer.pointer_type() != "pen" {
            if input.active.is_some()
                || (input.suppress_touches && pointer.pointer_type() == "touch")
            {
                consume(event);
            } else if pointer.pointer_type() == "mouse" {
                input.last_pen = false;
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
                    && input.touch_count == 0
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
                    || (input.active.is_none()
                        && pointer.buttons() == 0
                        && input.touch_count == 0
                        && !input.mouse_down) =>
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
        if !on_canvas && input.touch_count == 0 && !input.suppress_touches {
            return;
        }
        let stylus = (0..touch.changed_touches().length()).any(|index| {
            touch
                .changed_touches()
                .item(index)
                .is_some_and(|contact| is_stylus_touch(&contact))
        });
        // Stylus compatibility touches must not block their own pointerdown,
        // including when touchstart arrives before the PointerEvent.
        input.touch_count = (0..touch.touches().length())
            .filter_map(|index| touch.touches().item(index))
            .filter(|contact| !is_stylus_touch(contact))
            .count() as u32;
        // A contact ignored during a pen drag stays ignored until lift. It must
        // not enter eframe halfway through its touch sequence after the pen lifts.
        if stylus || input.active.is_some() || input.suppress_touches {
            consume(event);
            input.suppress_touches = input.touch_count != 0;
        }
    } else if event.dyn_ref::<web_sys::MouseEvent>().is_some()
        && (input.last_pen || input.active.is_some())
    {
        consume(event);
    }
}
