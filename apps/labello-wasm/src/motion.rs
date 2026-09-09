use wasm_bindgen::{JsCast, closure::Closure};

pub fn install(ctx: &eframe::egui::Context) {
    let Some(query) = web_sys::window()
        .and_then(|window| window.match_media("(prefers-reduced-motion: reduce)").ok())
        .flatten()
    else {
        return;
    };
    let context = ctx.clone();
    let current_query = query.clone();
    let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        labello_ui::set_reduced_motion(&context, current_query.matches());
    });
    // Keep the static default if preference changes cannot be observed.
    if query
        .add_event_listener_with_callback("change", listener.as_ref().unchecked_ref())
        .is_ok()
    {
        labello_ui::set_reduced_motion(ctx, query.matches());
        listener.forget();
    }
}
