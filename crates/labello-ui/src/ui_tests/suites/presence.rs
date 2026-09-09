fn presence_sample() -> labello_client::ServerPresence {
    labello_client::ServerPresence { users: vec![labello_client::PresentUser {
        user_id: UserId::from("alexandria_long_username"),
        github_login: None,
        datasets: vec![labello_client::PresenceDataset { dataset_id: DatasetId::from("other"), name: "Another dataset".into() }],
    }] }
}

#[test]
fn presence_retries_recover_without_changing_work_and_coalesce_refreshes() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let assignment = app.work.assignment.clone();
    let epoch = app.workspace_epoch;
    app.runtime.presence = Default::default();
    app.runtime.commands.clear();
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.request_presence();
    assert_eq!(app.runtime.commands.len(), 1);
    app.accept_presence(request, Ok(presence_sample()));
    assert_eq!(app.connection_status().0, theme::SUCCESS);
    for failures in 1..=3 {
        app.request_presence();
        let request = app.runtime.commands.back().unwrap().request().clone();
        app.accept_presence(request, Err("Network unavailable".to_string().into()));
        assert_eq!(app.runtime.presence.failures, failures);
        assert_eq!(app.connection_status().0, if failures < 3 { theme::WARNING } else { theme::DANGER });
        assert!(app.connection_status().1.contains("Network unavailable"));
    }
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.accept_presence(request, Ok(labello_client::ServerPresence { users: vec![] }));
    assert_eq!(app.connection_status().0, theme::SUCCESS);
    assert_eq!(app.runtime.presence.failures, 0);
    assert_eq!(app.work.assignment, assignment);
    assert_eq!(app.workspace_epoch, epoch);
    app.work.save_status = SaveStatus::Dirty;
    assert_eq!(app.connection_status().0, theme::WARNING);
    app.runtime.error = Some("Save rejected".into());
    assert_eq!(app.connection_status().0, theme::DANGER);
    assert!(app.connection_status().1.contains("Save rejected"));
}

#[test]
fn presence_discards_old_identity_and_epoch_responses() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    app.runtime.presence = Default::default();
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.begin_workspace_epoch();
    app.runtime.tx.send(UiMessage::PresenceLoaded { request, result: Ok(presence_sample()) }).unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.runtime.presence.value.is_none());
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.config.api_base_url = "http://other.example.test".into();
    app.accept_presence(request, Ok(presence_sample()));
    assert!(app.runtime.presence.value.is_none());
}

#[test]
fn presence_header_overflows_and_exposes_details_without_a_footer() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().runtime.presence.value = Some(presence_sample());
    for (width,height) in [(320.,320.), (320.,568.), (390.,844.), (600.,800.), (1288.,820.), (1440.,1000.)] {
        harness.set_size(egui::vec2(width,height));
        harness.step();
        let presence = harness.get_by_label_contains("Labelling presence: alexandria_long_username: Another dataset");
        assert!(presence.rect().bottom() <= 56.0);
        let dot = harness.get_by_label_contains("Connection status:");
        assert_eq!(dot.rect().height(),44.0);
        assert!(presence.rect().right() <= dot.rect().left());
        assert!(harness.query_by_label_contains("Annotation tasks submitted today").is_none());
        assert!(harness.get_by_label("Annotation canvas").rect().height() >= 44.0);
        assert_visible_controls_clamped(&harness,width,height);
    }
    let label = "Labelling presence: alexandria_long_username: Another dataset";
    click_accesskit_button(&mut harness,label);
    assert!(harness.query_all_by_label_contains("Another dataset").count() >= 2);

    harness.state_mut().runtime.presence.value = Some(labello_client::ServerPresence { users: vec![] });
    harness.set_size(egui::vec2(390.0, 844.0));
    harness.step();
    assert!(
        harness
            .query_by_role_and_label(
                egui::accesskit::Role::Button,
                "Labelling presence: No active labellers",
            )
            .is_some()
    );
    assert!(harness.query_by_label_contains("Labelling alone").is_none());
}

#[test]
fn presence_and_dot_are_right_aligned_before_navigation_without_topbar_username() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().runtime.presence.value = Some(presence_sample());

    for (width, height) in [
        (320.0, 568.0),
        (390.0, 844.0),
        (600.0, 800.0),
        (800.0, 800.0),
        (1440.0, 1000.0),
    ] {
        harness.set_size(egui::vec2(width, height));
        harness.step();
        let presence = harness.get_by_label_contains("Labelling presence:").rect();
        let dot = harness.get_by_label_contains("Connection status:").rect();
        let navigation = harness
            .query_by_label("Open navigation")
            .or_else(|| {
                harness
                    .query_all_by_role_and_label(egui::accesskit::Role::Button, "Sign out")
                    .next()
            })
            .expect("work header must expose a right-side navigation control")
            .rect();

        assert!(presence.left() >= -0.5 && presence.right() <= width + 0.5);
        assert!(dot.left() >= -0.5 && dot.right() <= width + 0.5);
        assert!(navigation.left() >= -0.5 && navigation.right() <= width + 0.5);
        assert!(presence.right() <= dot.left() + 0.5);
        assert!(dot.right() <= navigation.left() + 0.5);
        assert!(
            harness.query_by_label("Admin User").is_none(),
            "the signed-in username must not be rendered in the work top bar at {width}x{height}",
        );
    }
}

#[test]
fn compact_navigation_trigger_keeps_focus_across_header_resizes() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.set_size(egui::vec2(320.0, 568.0));
    harness.step();
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Open navigation")
        .focus();
    harness.step();

    for (width, height) in [(390.0, 844.0), (600.0, 800.0), (320.0, 320.0)] {
        harness.set_size(egui::vec2(width, height));
        harness.step();
        assert!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, "Open navigation")
                .is_focused(),
            "navigation trigger lost focus at {width}x{height}",
        );
    }
}

#[test]
fn presence_and_connection_details_keep_keyboard_focus_across_header_layouts() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.state_mut().runtime.presence.value = Some(presence_sample());
    let assignment = harness.state().work.assignment.clone();
    harness.step();
    for prefix in ["Labelling presence:", "Connection status:"] {
        harness.get_by_label_contains(prefix).focus();
        harness.step();
        for (width, height) in [(320.,320.), (1440.,1000.), (390.,844.)] {
            harness.set_size(egui::vec2(width,height));
            harness.step();
            assert!(harness.get_by_label_contains(prefix).is_focused(), "{prefix} at {width}");
        }
        harness.key_press(egui::Key::Enter);
        harness.step();
        let detail = if prefix.starts_with("Labelling") { "Another dataset" } else { "Connected" };
        assert!(harness.query_all_by_label_contains(detail).any(|node| node.accesskit_node().role() == egui::accesskit::Role::Label));
        harness.key_press(egui::Key::Escape);
        harness.step();
    }
    assert_eq!(harness.state().work.assignment, assignment);
}

#[test]
fn presence_includes_self_deduplicates_by_id_and_uses_handles_in_details() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    let app = harness.state_mut();
    let mut sample = presence_sample();
    sample.users[0].user_id = app.config.user_id.clone();
    sample.users[0].github_login = Some("octocat".into());
    sample.users.push(sample.users[0].clone());
    app.runtime.presence = Default::default();
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.accept_presence(request, Ok(sample));
    assert_eq!(app.runtime.presence.value.as_ref().unwrap().users.len(), 1);
    harness.step();
    let label = "Labelling presence: @octocat: Another dataset";
    assert!(harness.query_by_label(label).is_some());
    assert!(harness.query_by_label_contains("No active labellers").is_none());
    assert!(harness.query_by_label_contains("You are alone").is_none());
    click_accesskit_button(&mut harness, label);
    assert!(harness.query_by_role_and_label(egui::accesskit::Role::Label,
        "@octocat: Another dataset").is_some());
    harness.key_press(egui::Key::Escape);
    // Matching presentation names must not collapse different identities.
    let app = harness.state_mut();
    let sample = app.runtime.presence.value.as_mut().unwrap();
    let mut other = sample.users[0].clone();
    other.user_id = UserId::from("different_id");
    sample.users.push(other);
    app.request_presence();
    let request = app.runtime.commands.back().unwrap().request().clone();
    let sample = app.runtime.presence.value.clone().unwrap();
    app.accept_presence(request, Ok(sample));
    assert_eq!(app.runtime.presence.value.as_ref().unwrap().users.len(), 2);
}

#[test]
fn presence_counts_and_animation_keep_header_geometry_and_accessible_names_stable() {
    fn text_in(shape: &egui::Shape, expected: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text == expected,
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| text_in(shape, expected)),
            _ => false,
        }
    }
    let mut app = LabelloApp::default();
    let mut sample = presence_sample();
    sample.users[0].github_login = Some("a-very-long-github-handle-for-testing".into());
    app.runtime.presence.value = Some(sample);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(190.0, 60.0))
        .build_ui_state(|ui, app: &mut LabelloApp| app.presence_summary(ui), app);
    harness.step();
    assert!(harness.output().shapes.iter().any(|s| text_in(&s.shape, "1 person is labelling")));
    let users = &mut harness.state_mut().runtime.presence.value.as_mut().unwrap().users;
    let mut other = users[0].clone();
    other.user_id = "another_user".into();
    users.push(other);
    harness.step();
    assert!(harness.output().shapes.iter().any(|s| text_in(&s.shape, "2 people are labelling")));
    harness.set_size(egui::vec2(1200.0, 60.0));
    harness.step();
    let rect = harness.get_by_label_contains("Labelling presence:").rect();
    let label = harness.get_by_label_contains("Labelling presence:").accesskit_node().label().unwrap().to_owned();
    crate::set_reduced_motion(&harness.ctx, false);
    for time in [0.1, 0.5, 1.0, 1.5, 3.0, 8.1, 9.0] {
        harness.input_mut().time = Some(time);
        harness.step();
        assert_eq!(harness.get_by_label(&label).rect(), rect);
    }
    crate::set_reduced_motion(&harness.ctx, true);
    harness.step();
    assert_eq!(harness.get_by_label(&label).rect(), rect);
}
