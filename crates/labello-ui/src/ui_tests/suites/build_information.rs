fn release_build(tag: &str, digit: char) -> labello_client::BuildIdentity {
    labello_client::BuildIdentity::from_metadata(Some(tag), Some(&digit.to_string().repeat(40)))
}

fn mismatch_app() -> LabelloApp {
    let mut app = LabelloApp::default();
    app.builds.web = release_build("v1.2.3", 'a');
    app.builds.server = Some(release_build("v1.2.4", 'b'));
    app.builds.checked = true;
    app
}

#[test]
fn build_information_refresh_coalesces_and_rejects_obsolete_endpoint_responses() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.builds.web = release_build("v1.2.3", 'a');
    app.builds.server = Some(release_build("v1.2.4", 'b'));
    assert!(app.builds_differ());
    app.request_build_information();
    assert!(app.builds.loading);
    assert!(!app.builds_differ());
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.request_build_information();
    assert_eq!(app.runtime.commands.len(), 1);
    app.runtime
        .tx
        .send(UiMessage::BuildInformationLoaded {
            request: request.clone(),
            result: Err("unavailable".to_string().into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.builds.loading);
    assert!(app.builds.server.is_none());
    assert!(app.build_information_text().contains("Server: unavailable"));
    app.runtime.commands.clear();
    app.request_build_information();
    let old = app.runtime.commands.back().unwrap().request().clone();
    app.config.api_base_url = "http://other.example.test".into();
    app.rebuild_http_api();
    app.request_build_information();
    let current = app.runtime.commands.back().unwrap().request().clone();
    app.runtime
        .tx
        .send(UiMessage::BuildInformationLoaded {
            request: old,
            result: Ok(release_build("v1.2.4", 'b')),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.builds.server.is_none());
    assert!(app.builds.loading);
    assert_eq!(app.builds.pending_request_id, Some(current.request_id));
    app.runtime
        .tx
        .send(UiMessage::BuildInformationLoaded {
            request: current,
            result: Ok(app.builds.web.clone()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.builds.server, Some(app.builds.web.clone()));
    assert!(!app.builds_differ());
}

#[test]
fn build_information_browser_a_survives_server_b_and_visible_focus_refresh() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.set_web_build_metadata(Some("v1.2.3"), Some(&"a".repeat(40)));
    let web_a = app.builds.web.clone();
    app.builds.server = Some(web_a.clone());
    let notify = app.build_refresh_notifier(egui::Context::default());
    notify();
    notify();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.runtime.commands.len(), 1);
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.runtime
        .tx
        .send(UiMessage::BuildInformationLoaded {
            request,
            result: Ok(release_build("v1.2.4", 'b')),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.builds.web, web_a);
    assert!(app.builds_differ());
}

#[test]
fn build_information_copy_waits_for_success_and_exposes_failure_and_manual_text() {
    for succeeded in [true, false] {
        let mut app = mismatch_app();
        app.view = AppView::Setup;
        app.setup.section = SetupSection::About;
        let captured = Rc::new(RefCell::new(String::new()));
        let copied = captured.clone();
        app.set_build_clipboard_writer(Rc::new(move |text| {
            *copied.borrow_mut() = text;
            Box::pin(async move { if succeeded { Ok(()) } else { Err(()) } })
        }));
        app.copy_build_information();
        assert!(app.builds.copying);
        assert!(app.builds.copy_feedback.is_none());
        app.process_messages(&egui::Context::default());
        assert!(!app.builds.copying);
        assert!(captured.borrow().contains(&"a".repeat(40)));
        assert!(captured.borrow().contains(&"b".repeat(40)));
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 800.0))
            .build_eframe(|_| app);
        harness.run();
        assert!(
            harness
                .query_by_label("Build information for manual copying")
                .is_some()
        );
        let label = if succeeded {
            "Build information copied."
        } else {
            "Copy failed. Select the build information below and copy it manually."
        };
        let feedback = harness.get_by_label(label);
        assert_eq!(
            feedback.accesskit_node().live(),
            egui::accesskit::Live::Polite
        );
    }
}

#[test]
fn build_information_warning_is_lower_right_and_navigation_preserves_work_until_confirmed() {
    for (width, height) in
        viewport_sizes()
            .into_iter()
            .chain([(390.0, 844.0), (1440.0, 1000.0), (320.0, 320.0)])
    {
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.set_size(egui::vec2(width, height));
        harness.state_mut().builds.web = release_build("v1.2.3", 'a');
        harness.state_mut().builds.server = Some(release_build("v1.2.4", 'b'));
        harness.state_mut().builds.checked = true;
        harness.state_mut().work.save_status = SaveStatus::Dirty;
        let assignment = harness.state().work.assignment.clone();
        assert!(assignment.is_some());
        let epoch = harness.state().workspace_epoch;
        harness.run();
        let warning = harness.get_by_role_and_label(
            egui::accesskit::Role::Button,
            "Web app and server builds differ; open About",
        );
        let bounds = warning.rect();
        assert!(
            bounds.min.x >= 0.0 && bounds.max.x <= width + 1.0,
            "{width} {bounds:?}"
        );
        assert!(
            bounds.min.y >= 0.0 && bounds.max.y <= height + 1.0,
            "{height} {bounds:?}"
        );
        assert!(bounds.max.x >= width - 2.0 && bounds.max.y >= height - 2.0);
        assert_eq!(harness.state().workspace_epoch, epoch);
        warning.click();
        harness.run();
        assert_eq!(harness.state().view, AppView::Annotate);
        assert_eq!(
            harness.state().work.pending_transition,
            Some(crate::app::PendingTransition::About)
        );
        harness.state_mut().cancel_pending_transition();
        harness.run();
        assert_eq!(harness.state().work.assignment, assignment);
        assert_eq!(harness.state().work.save_status, SaveStatus::Dirty);
        assert_eq!(harness.state().workspace_epoch, epoch);
    }
}

#[test]
fn build_information_about_is_available_before_and_after_authentication() {
    for signed_in in [false, true] {
        let mut app = mismatch_app();
        app.view = AppView::Setup;
        app.work.assignment = None;
        app.auth.account = signed_in.then(|| SpyApi::new().state.borrow().users[0].account.clone());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 1000.0))
            .build_eframe(|_| app);
        harness.run();
        click_accesskit_button(&mut harness, "About");
        harness.run();
        assert_eq!(harness.state().setup.section, SetupSection::About);
        assert!(harness.query_by_label("About Labello").is_some());
        assert!(
            harness
                .query_by_label_contains(
                    "Web app: v1.2.3; source commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_some()
        );
        assert!(harness.query_by_label("Copy build information").is_some());
    }
}

#[test]
fn build_information_warning_supports_keyboard_focus_and_enter_in_short_layout() {
    let mut app = mismatch_app();
    app.view = AppView::Setup;
    app.work.assignment = None;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.run();
    let label = "Web app and server builds differ; open About";
    for _ in 0..40 {
        harness.key_press(egui::Key::Tab);
        harness.run();
        if harness.get_by_label(label).is_focused() {
            break;
        }
    }
    let warning = harness.get_by_label(label);
    assert!(warning.is_focused());
    assert!(warning.rect().bottom() <= 320.0);
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().setup.section, SetupSection::About);
}

#[test]
fn build_information_about_preserves_staged_admin_changes() {
    let mut harness = loaded_admin_harness(Rc::new(SpyApi::new()));
    harness
        .state_mut()
        .datasets
        .admin_config
        .as_mut()
        .unwrap()
        .name = "Unsaved name".into();
    let epoch = harness.state().workspace_epoch;
    harness.state_mut().open_about();
    assert_eq!(harness.state().view, AppView::Admin);
    assert_eq!(harness.state().workspace_epoch, epoch);
    assert_eq!(
        harness.state().datasets.admin_config.as_ref().unwrap().name,
        "Unsaved name"
    );
    assert!(
        harness
            .state()
            .runtime
            .error
            .as_ref()
            .unwrap()
            .contains("Save or discard")
    );
}

#[cfg(feature = "inspector-presets")]
#[test]
fn build_information_warning_keeps_review_and_migration_controls_reachable() {
    use crate::inspector_presets::{self, InspectorPreset};
    for preset in [InspectorPreset::Review, InspectorPreset::MigrationObject] {
        for (width, height) in [
            (320.0, 568.0),
            (390.0, 844.0),
            (600.0, 800.0),
            (1288.0, 820.0),
            (1440.0, 1000.0),
            (320.0, 320.0),
        ] {
            let mut app = inspector_presets::build(preset, &egui::Context::default());
            app.builds.web = release_build("v1.2.3", 'a');
            app.builds.server = Some(release_build("v1.2.4", 'b'));
            app.builds.checked = true;
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, height))
                .build_eframe(|_| app);
            harness.run();
            assert_control_inside(
                &harness,
                "Web app and server builds differ; open About",
                egui::accesskit::Role::Button,
                width,
                height,
            );
            assert_visible_controls_clamped(&harness, width, height);
            let assignment = harness.state().work.assignment.clone();
            let epoch = harness.state().workspace_epoch;
            click_accesskit_button(&mut harness, "Web app and server builds differ; open About");
            assert_eq!(
                harness.state().work.pending_transition,
                Some(crate::app::PendingTransition::About)
            );
            harness.state_mut().cancel_pending_transition();
            assert_eq!(harness.state().work.assignment, assignment);
            assert_eq!(harness.state().workspace_epoch, epoch);
        }
    }
}

#[test]
fn build_information_about_copy_focus_scrolls_into_short_view() {
    let mut app = mismatch_app();
    app.view = AppView::Setup;
    app.setup.section = SetupSection::About;
    app.auth.account = Some(SpyApi::new().state.borrow().users[0].account.clone());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.run();
    for _ in 0..40 {
        harness.key_press(egui::Key::Tab);
        harness.run();
        if harness.get_by_label("Copy build information").is_focused() {
            break;
        }
    }
    let copy = harness.get_by_label("Copy build information");
    assert!(copy.is_focused());
    assert!(copy.rect().bottom() <= 276.0);
    assert!(copy.rect().top() >= 56.0);
}

#[test]
fn build_information_long_values_wrap_and_keep_complete_accessible_identity() {
    let mut app = mismatch_app();
    app.view = AppView::Setup;
    app.setup.section = SetupSection::About;
    let tag = "v".repeat(64);
    let commit = "a".repeat(64);
    app.set_web_build_metadata(Some(&tag), Some(&commit));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 844.0))
        .build_eframe(|_| app);
    harness.run();
    let label = format!("Web app: {tag}; source commit: {commit}");
    let row = harness.get_by_label(&label);
    assert!(row.rect().left() >= 0.0 && row.rect().right() <= 320.0);
    assert!(row.rect().height() > 20.0, "long values should wrap");
    assert!(harness.state().build_information_text().contains(&commit));
}

#[test]
fn build_information_stays_coalesced_across_session_and_workspace_epochs() {
    for dispatched in [false, true] {
        let mut app = base_live_app(Rc::new(SpyApi::new()));
        app.request_build_information();
        let request = app.runtime.commands.back().unwrap().request().clone();
        if dispatched {
            // Hold the transport response while normal startup/session ownership changes.
            app.runtime.commands.pop_front();
        }
        app.request_session();
        app.begin_workspace_epoch();
        app.request_build_information();
        assert!(app.builds.loading);
        let build_requests = app
            .runtime
            .commands
            .iter()
            .filter_map(|command| match command {
                UiCommand::BuildInformation { request } => Some(request.request_id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            build_requests,
            if dispatched {
                vec![]
            } else {
                vec![request.request_id]
            }
        );
        app.runtime
            .tx
            .send(UiMessage::BuildInformationLoaded {
                request: request.clone(),
                result: Ok(release_build("v1.2.3", 'a')),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.builds.server, Some(release_build("v1.2.3", 'a')));
        assert!(!app.builds.loading);
        // A duplicate completion cannot replace the accepted response.
        app.runtime
            .tx
            .send(UiMessage::BuildInformationLoaded {
                request,
                result: Ok(release_build("v9.0.0", 'b')),
            })
            .unwrap();
        app.process_messages(&egui::Context::default());
        assert_eq!(app.builds.server, Some(release_build("v1.2.3", 'a')));
    }
}

#[test]
fn build_information_dispatch_failure_ends_the_endpoint_request_after_auth_changes() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.request_build_information();
    let request = app.runtime.commands.pop_front().unwrap().request().clone();
    app.begin_auth_epoch();
    app.runtime
        .tx
        .send(UiMessage::RequestFailed {
            request,
            error: "dispatch unavailable".into(),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(!app.builds.loading);
    assert!(app.builds.server.is_none());
    app.request_build_information();
    assert!(app.builds.loading);
    assert_eq!(app.runtime.commands.len(), 1);
}
