use std::rc::Rc;

use wasm_bindgen::{JsCast, closure::Closure};

pub(super) fn install(app: &mut labello_ui::LabelloApp, ctx: &eframe::egui::Context) {
    app.set_web_build_metadata(
        option_env!("LABELLO_RELEASE_TAG"),
        option_env!("LABELLO_SOURCE_COMMIT"),
    );
    app.set_build_clipboard_writer(Rc::new(|text| {
        let promise = web_sys::window().and_then(|window| {
            js_sys::Reflect::get(window.navigator().as_ref(), &"clipboard".into())
                .ok()
                .filter(|value| !value.is_null() && !value.is_undefined())
                .map(|value| {
                    value
                        .unchecked_into::<web_sys::Clipboard>()
                        .write_text(&text)
                })
        });
        Box::pin(async move {
            let promise = promise.ok_or(())?;
            wasm_bindgen_futures::JsFuture::from(promise)
                .await
                .map(|_| ())
                .map_err(|_| ())
        })
    }));

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let notify = app.build_refresh_notifier(ctx.clone());
    for (target, event) in [
        (window.unchecked_into::<web_sys::EventTarget>(), "focus"),
        (
            document.clone().unchecked_into::<web_sys::EventTarget>(),
            "visibilitychange",
        ),
    ] {
        let document = document.clone();
        let notify = notify.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if document.visibility_state() == web_sys::VisibilityState::Visible {
                notify();
            }
        });
        if target
            .add_event_listener_with_callback(event, listener.as_ref().unchecked_ref())
            .is_ok()
        {
            // The one app and its listeners share the page lifetime.
            listener.forget();
        }
    }
}
