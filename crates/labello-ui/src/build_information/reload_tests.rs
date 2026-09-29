use super::*;
use std::cell::Cell;

fn app() -> (LabelloApp, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let mut app = LabelloApp::default();
    app.work.save_status = crate::app::SaveStatus::Saved;
    app.builds.web = BuildIdentity::from_metadata(Some("v1"), Some(&"a".repeat(40)));
    app.builds.server = Some(BuildIdentity::from_metadata(
        Some("v2"),
        Some(&"b".repeat(40)),
    ));
    let preparations = Rc::new(Cell::new(0));
    let navigations = Rc::new(Cell::new(0));
    let p = preparations.clone();
    let n = navigations.clone();
    app.set_build_reload_adapter(BuildReloadAdapter {
        prepare: Rc::new(move |_, _| {
            p.set(p.get() + 1);
            Box::pin(async { Ok(()) })
        }),
        navigate: Rc::new(move || {
            n.set(n.get() + 1);
            Ok(())
        }),
    });
    (app, preparations, navigations)
}

#[test]
fn reload_coalesces_and_rechecks_edits_after_asset_preparation() {
    let (mut app, preparations, navigations) = app();
    let ctx = egui::Context::default();
    app.advance_build_reload(&ctx);
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 1);
    assert_eq!(navigations.get(), 0);
    app.process_messages(&ctx);
    app.work.save_status = crate::app::SaveStatus::Dirty;
    app.advance_build_reload(&ctx);
    assert_eq!(
        navigations.get(),
        0,
        "unsaved edits cannot be lost during asset fetch"
    );
    app.work.save_status = crate::app::SaveStatus::Saved;
    app.advance_build_reload(&ctx);
    app.advance_build_reload(&ctx);
    assert_eq!(navigations.get(), 1);
}

#[test]
fn reload_requires_current_complete_mismatch_and_rejects_obsolete_preparation() {
    let ctx = egui::Context::default();
    for server in [
        None,
        Some(BuildIdentity::default()),
        Some(BuildIdentity::from_metadata(
            Some("v1"),
            Some(&"a".repeat(40)),
        )),
    ] {
        let (mut app, preparations, _) = app();
        app.builds.server = server;
        app.advance_build_reload(&ctx);
        assert_eq!(preparations.get(), 0);
    }
    let (mut app, _, navigations) = app();
    app.advance_build_reload(&ctx);
    app.rebuild_http_api();
    app.process_messages(&ctx);
    app.advance_build_reload(&ctx);
    assert_eq!(navigations.get(), 0);
    assert!(matches!(app.builds.reload.phase, ReloadPhase::Idle));
}

#[test]
fn reload_waits_for_operations_and_nonrecoverable_input() {
    let (mut app, preparations, _) = app();
    let ctx = egui::Context::default();
    app.work.migration.draft_dirty = true;
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 0);
    app.work.migration.draft_dirty = false;
    app.runtime.active_requests.insert(42);
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 0);
    app.runtime.active_requests.clear();
    app.import.open = true;
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 0);
    app.import.open = false;
    app.view = AppView::Setup;
    app.setup.section = SetupSection::Create;
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 0);
    app.setup.section = SetupSection::About;
    app.advance_build_reload(&ctx);
    assert_eq!(preparations.get(), 1);
}

#[test]
fn reload_failure_stays_actionable_without_automatic_retry() {
    let (mut app, _, _) = app();
    let attempts = Rc::new(Cell::new(0));
    let calls = attempts.clone();
    app.set_build_reload_adapter(BuildReloadAdapter {
        prepare: Rc::new(move |_, manual| {
            assert!(!manual);
            calls.set(calls.get() + 1);
            Box::pin(async { Err("Update failed. Retry when deployment is complete.".into()) })
        }),
        navigate: Rc::new(|| panic!("failed preparation cannot navigate")),
    });
    let ctx = egui::Context::default();
    app.advance_build_reload(&ctx);
    app.process_messages(&ctx);
    app.advance_build_reload(&ctx);
    app.advance_build_reload(&ctx);
    assert_eq!(attempts.get(), 1);
    assert!(matches!(app.builds.reload.phase, ReloadPhase::Failed(_)));
}

#[test]
fn manual_retry_rechecks_identity_before_bypassing_attempt_guard() {
    let (mut app, _, _) = app();
    let manual_attempt = Rc::new(Cell::new(false));
    let manual = manual_attempt.clone();
    app.set_build_reload_adapter(BuildReloadAdapter {
        prepare: Rc::new(move |_, explicit| {
            manual.set(explicit);
            Box::pin(async { Err("still unavailable".into()) })
        }),
        navigate: Rc::new(|| panic!("not prepared")),
    });
    let server = app.builds.server.clone();
    app.retry_build_reload();
    assert_eq!(app.builds.server, server);
    app.builds.loading = true;
    app.advance_build_reload(&egui::Context::default());
    assert!(!manual_attempt.get());
    app.builds.loading = false;
    app.builds.server = server;
    app.advance_build_reload(&egui::Context::default());
    assert!(manual_attempt.get());
}

#[test]
fn navigation_replaces_editable_controls_until_document_unloads() {
    use egui_kittest::{Harness, kittest::Queryable};
    let (mut app, _, navigations) = app();
    let ctx = egui::Context::default();
    app.advance_build_reload(&ctx);
    app.process_messages(&ctx);
    app.advance_build_reload(&ctx);
    assert_eq!(navigations.get(), 1);
    let mut harness = Harness::builder().build_eframe(|_| app);
    harness.run();
    assert!(harness.query_by_label("Updating Labello...").is_some());
    assert!(harness.query_by_label("Annotate").is_none());
}
