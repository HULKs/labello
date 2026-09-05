#[test]
fn session_is_restored_before_datasets_load_and_logout_clears_it() {
    let api = Rc::new(SpyApi::new());
    let mut harness = live_harness(api.clone());
    step_until(&mut harness, 8, |app| !app.datasets.summaries.is_empty());

    assert_eq!(api.counts().me, 1);
    assert_eq!(api.counts().list_datasets, 1);
    assert_eq!(
        harness
            .state()
            .auth
            .account
            .as_ref()
            .map(|account| account.user_id.clone()),
        Some(UserId::from("admin"))
    );
    harness.state_mut().work.drawer = Some(Drawer::Workflow);
    harness.state_mut().work.show_tutorial = true;
    click(&mut harness, "Sign out");
    step_until(&mut harness, 8, |app| app.auth.account.is_none());
    assert_eq!(api.counts().logout, 1);
    assert!(harness.state().datasets.summaries.is_empty());
    assert!(harness.state().work.drawer.is_none());
    assert!(!harness.state().work.show_tutorial);
    assert_eq!(harness.state().setup.section, SetupSection::Login);
    assert!(harness.query_by_label("Sign in with GitHub").is_some());
}

#[test]
fn signed_out_setup_offers_advertised_login_methods_without_raw_credentials() {
    let api = Rc::new(SpyApi::new());
    api.fail_me();
    let mut app = base_live_app(api.clone());
    app.auth.checked = false;
    app.auth.options_checked = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 780.0))
        .with_max_steps(20)
        .build_eframe(|_| app);
    step_until(&mut harness, 8, |app| app.auth.checked);
    assert_eq!(harness.state().setup.section, SetupSection::Login);
    assert!(harness.query_by_label("Datasets").is_none());
    assert!(harness.query_by_label("Sign in with GitHub").is_some());
    assert!(harness.query_by_label("Continue as local admin").is_some());
    assert!(harness.query_by_label("Dev token").is_none());
    assert!(harness.query_by_label("Development user ID").is_none());

    click(&mut harness, "Continue as local admin");
    step_until(&mut harness, 8, |app| app.auth.account.is_some());
    assert_eq!(api.counts().local_admin_login, 1);
}

#[test]
fn auth_options_failure_clears_state_from_the_previous_endpoint() {
    let api = Rc::new(SpyApi::new());
    let metadata = api.metadata();
    let mut app = LabelloApp::default();
    app.runtime.api = Some(api.clone());
    app.auth.account = Some(api.state.borrow().users[0].account.clone());
    app.datasets.summaries = vec![DatasetSummary {
        dataset_id: metadata.dataset_id.clone(),
        name: metadata.name.clone(),
        roles: vec![DatasetRole::DataAdmin],
        total_images: metadata.images.len(),
    }];
    app.datasets.metadata = Some(metadata.clone());
    app.datasets.admin_config = Some(metadata.clone());
    app.datasets.admin_baseline = Some(metadata);
    app.datasets.stats = stats(3);
    app.datasets.last_stats_completion = Some(Instant::now());
    app.datasets.stats_error = Some("old statistics error".to_string());
    app.runtime.notice = Some("Signed in as Previous User".to_string());
    app.view = AppView::Admin;
    app.request_auth_options();
    let request = app.runtime.commands.back().unwrap().request().clone();
    app.runtime
        .tx
        .send(UiMessage::AuthOptionsLoaded {
            request,
            result: Err("server unavailable".to_string().into()),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());

    assert!(app.auth.checked);
    assert!(app.auth.account.is_none());
    assert!(app.datasets.summaries.is_empty());
    assert!(app.datasets.metadata.is_none());
    assert!(app.datasets.admin_config.is_none());
    assert_eq!(app.datasets.stats, DatasetStats::default());
    assert!(app.datasets.last_stats_completion.is_none());
    assert!(app.datasets.stats_error.is_none());
    assert!(app.runtime.notice.is_none());
    assert!(app.work.current.is_none());
    assert_eq!(app.view, AppView::Setup);
}

#[test]
fn github_login_uses_the_application_url_and_same_tab() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api.clone());
    app.request_github_login();
    app.start_next_command();

    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.process_messages(ui.ctx());
    });

    assert_eq!(
        api.last_oauth_return_to().as_deref(),
        Some("https://app.example.test/label?dataset=demo")
    );
    assert_eq!(
        output.platform_output.commands,
        vec![egui::OutputCommand::OpenUrl(egui::OpenUrl::same_tab(
            "https://example.invalid/login",
        ))]
    );
}

#[test]
fn dataset_creation_completion_accepts_its_new_dataset_identity() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    let mut metadata = SpyApi::new().metadata();
    metadata.dataset_id = DatasetId::from("new-dataset");
    metadata.name = "New dataset".to_string();
    let request = test_request(&app, 700, Some("new-dataset"));
    app.loading.dataset = true;
    app.runtime.active_requests.insert(request.request_id);
    app.runtime
        .tx
        .send(UiMessage::DatasetCreated {
            request,
            result: Box::new(Ok(metadata)),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());

    assert_eq!(app.config.dataset_id, DatasetId::from("new-dataset"));
    assert_eq!(app.datasets.requested_view, Some(AppView::Admin));
    assert!(app.loading.dataset);
    let load = app.runtime.commands.front().unwrap();
    assert_eq!(
        load.request().dataset_id.as_ref(),
        Some(&DatasetId::from("new-dataset"))
    );
}

#[test]
fn dataset_list_success_only_clears_its_own_error() {
    let mut app = base_live_app(Rc::new(SpyApi::new()));
    app.runtime.error = Some("dataset load failed".to_string());
    app.request_dataset_list();
    let UiCommand::DatasetList { request } = app.runtime.commands.pop_back().unwrap() else {
        panic!("expected dataset list command");
    };
    app.runtime
        .tx
        .send(UiMessage::DatasetList {
            request,
            result: Ok(Vec::new()),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());

    assert_eq!(app.runtime.error.as_deref(), Some("dataset load failed"));

    app.datasets.summaries_error = Some("list failed".to_string());
    app.runtime.error = Some("list failed".to_string());
    app.request_dataset_list();
    assert!(app.datasets.summaries_error.is_none());
    assert!(app.runtime.error.is_none());
    let UiCommand::DatasetList { request } = app.runtime.commands.pop_back().unwrap() else {
        panic!("expected dataset list command");
    };
    app.runtime
        .tx
        .send(UiMessage::DatasetList {
            request,
            result: Ok(Vec::new()),
        })
        .unwrap();

    app.process_messages(&egui::Context::default());

    assert!(app.runtime.error.is_none());
}

#[test]
fn setup_recommendation_keeps_explicit_dataset_actions() {
    let api = Rc::new(SpyApi::new());
    let mut harness = live_harness(api);
    step_until(&mut harness, 8, |app| !app.datasets.summaries.is_empty());

    let recommended = harness
        .get_by_label("Recommended dataset Demo Dataset")
        .rect();
    let all_datasets = harness.get_by_label("All datasets").rect();
    let dataset = harness.get_by_label("Dataset card Demo Dataset").rect();
    assert!(recommended.bottom() < all_datasets.top());
    assert!(all_datasets.bottom() < dataset.top());
    assert_eq!(
        harness
            .query_all_by_role_and_label(
                egui::accesskit::Role::Button,
                "Annotate Demo Dataset",
            )
            .count(),
        1
    );
    assert_eq!(
        harness
            .query_all_by_role_and_label(
                egui::accesskit::Role::Button,
                "Continue with Demo Dataset",
            )
            .count(),
        1
    );
    click(&mut harness, "Annotate Demo Dataset");
    step_until(&mut harness, 12, |app| app.work.current.is_some());
    assert_eq!(harness.state().view, AppView::Annotate);
}

#[test]
fn setup_does_not_recommend_a_dataset_without_an_available_destination() {
    let api = Rc::new(SpyApi::new());
    api.set_summary_roles(Vec::new());
    let mut harness = live_harness(api);
    step_until(&mut harness, 8, |app| !app.datasets.summaries.is_empty());

    assert!(harness.query_by_label("Recommended").is_none());
    assert!(
        harness
            .query_by_label("Continue with Demo Dataset")
            .is_none()
    );
    assert!(
        harness
            .query_by_label("Dataset card Demo Dataset")
            .is_some()
    );
}

#[test]
fn adjudicator_only_dataset_recommends_statistics() {
    let api = Rc::new(SpyApi::new());
    api.set_summary_roles(vec![DatasetRole::Adjudicator]);
    let mut harness = live_harness(api);
    step_until(&mut harness, 8, |app| !app.datasets.summaries.is_empty());

    assert!(
        harness
            .query_by_label("View statistics for this dataset.")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Adjudicate Demo Dataset")
            .is_none()
    );
    click(&mut harness, "Continue with Demo Dataset");
    step_until(&mut harness, 12, |app| app.view == AppView::Stats);
}

#[test]
fn api_url_focus_loss_does_not_reconnect_and_enter_commits() {
    let mut app = LabelloApp {
        view: AppView::Setup,
        ..Default::default()
    };
    app.setup.section = SetupSection::AdvancedConnection;
    let original_url = app.config.api_base_url.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 780.0))
        .build_eframe(move |_| app);
    let input = harness
        .query_all_by_role_and_label(egui::accesskit::Role::TextInput, "API URL")
        .next()
        .expect("API URL field")
        .rect()
        .center();
    click_at(&mut harness, input);
    harness.state_mut().setup.api_base_url_draft = "not a URL".to_string();
    harness.step();
    let auth_epoch = harness.state().auth_epoch;

    let connection = harness.get_by_label("Advanced connection").rect().center();
    click_at(&mut harness, connection);
    assert_eq!(harness.state().config.api_base_url, original_url);
    assert_eq!(harness.state().auth_epoch, auth_epoch);
    assert!(harness.query_by_label("Reconnect").is_some());

    let input = harness
        .query_all_by_role_and_label(egui::accesskit::Role::TextInput, "API URL")
        .next()
        .unwrap()
        .rect()
        .center();
    click_at(&mut harness, input);
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert_eq!(harness.state().config.api_base_url, "not a URL");
    assert!(harness.state().auth_epoch > auth_epoch);
    harness.state_mut().setup.section = SetupSection::Datasets;
    harness.step();
    assert_eq!(harness.state().setup.section, SetupSection::Login);
    assert!(harness.query_by_label("Reconnect").is_none());
    click(&mut harness, "Advanced connection");
    assert!(harness.query_by_label("Reconnect").is_some());
    assert!(
        harness
            .query_by_label("Checking dataset access...")
            .is_none()
    );
}

#[test]
fn signed_out_login_keeps_endpoint_configuration_in_advanced_view() {
    let api = Rc::new(SpyApi::new());
    api.fail_me();
    let mut app = base_live_app(api);
    app.auth.checked = false;
    app.auth.options_checked = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 780.0))
        .with_max_steps(20)
        .build_eframe(|_| app);
    step_until(&mut harness, 8, |app| app.auth.checked);
    assert!(harness.query_by_label("Sign in with GitHub").is_some());
    assert!(harness.query_by_label("API URL").is_none());
    assert!(harness.query_by_label("About").is_some());
    click(&mut harness, "Advanced connection");
    assert!(harness.query_by_label("API URL").is_some());
}

#[test]
fn expired_session_blocks_commands_and_retains_draft_for_the_same_account() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    let app = harness.state_mut();
    app.work.save_status = SaveStatus::Dirty;
    let draft = app.work.annotations.clone();
    app.import.open = true;
    app.import.destination_name = "Retained import draft".to_string();
    app.import.capabilities_loading = true;
    app.import.source_picker.loading = true;
    app.import.source_picker.pending_request_id = Some(123);
    app.import.yolo_inspection_loading = true;
    let import_epoch = app.import_epoch;
    let request = test_request(app, 9100, None);
    app.runtime.active_requests.insert(request.request_id);
    api.fail_me();
    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request,
            result: Err(ClientError::Api {
                status: 401,
                message: "authentication required".to_string(),
            }
            .into()),
        })
        .unwrap();
    let context = egui::Context::default();
    app.process_messages(&context);
    assert!(app.loading.session);
    assert!(!app.auth.checked);
    assert_eq!(app.view, AppView::Setup);
    assert!(app.import_epoch > import_epoch);
    assert!(!app.import.capabilities_loading);
    assert!(!app.import.source_picker.loading);
    assert!(app.import.source_picker.pending_request_id.is_none());
    assert!(!app.import.yolo_inspection_loading);
    assert_eq!(app.import.destination_name, "Retained import draft");
    app.start_next_command();
    app.process_messages(&context);
    assert!(app.auth.account.is_none());
    assert!(app.auth.session_error.is_none());
    assert_eq!(app.work.annotations, draft);
    app.request_auth_options();
    let request = app.runtime.commands.front().unwrap().request().clone();
    app.runtime.commands.clear();
    app.runtime
        .tx
        .send(UiMessage::AuthOptionsLoaded {
            request,
            result: Err("temporary options failure".to_string().into()),
        })
        .unwrap();
    app.process_messages(&context);
    assert!(app.auth.recovery.is_some());
    assert_eq!(app.work.annotations, draft);
    app.request_create_dataset();
    assert!(app.runtime.commands.is_empty());
    app.request_local_admin_login();
    app.start_next_command();
    app.process_messages(&context);
    assert_eq!(
        app.auth.account.as_ref().unwrap().user_id,
        UserId::from("admin")
    );
    assert_eq!(app.view, AppView::Annotate);
    assert_eq!(app.work.annotations, draft);
}

#[test]
fn role_denial_rechecks_session_without_signing_out_or_losing_work() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    let assignment = app.work.assignment.clone();
    let request = test_request(app, 9101, None);
    app.runtime.active_requests.insert(request.request_id);
    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request,
            result: Err(ClientError::Api {
                status: 401,
                message: "dataset access denied".to_string(),
            }
            .into()),
        })
        .unwrap();
    let context = egui::Context::default();
    app.process_messages(&context);
    app.start_next_command();
    app.process_messages(&context);
    assert!(app.auth.account.is_some());
    assert_eq!(app.view, AppView::Annotate);
    assert_eq!(app.work.assignment, assignment);
}

#[test]
fn expired_session_login_to_another_account_clears_previous_work() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    let app = harness.state_mut();
    let request = test_request(app, 9102, None);
    app.runtime.active_requests.insert(request.request_id);
    api.fail_me();
    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request,
            result: Err(ClientError::Api {
                status: 401,
                message: "authentication required".to_string(),
            }
            .into()),
        })
        .unwrap();
    let context = egui::Context::default();
    app.process_messages(&context);
    app.start_next_command();
    app.process_messages(&context);
    api.state.borrow_mut().users[0].account.user_id = UserId::from("different_account");
    app.request_local_admin_login();
    app.start_next_command();
    app.process_messages(&context);
    assert_eq!(
        app.auth.account.as_ref().unwrap().user_id,
        UserId::from("different_account")
    );
    assert!(app.work.assignment.is_none());
    assert!(app.work.annotations.is_empty());
    assert!(app.datasets.metadata.is_none());
}

#[test]
fn endpoint_change_immediately_invalidates_account_dataset_and_auth_options() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    let old_epoch = app.auth_epoch;
    app.config.api_base_url = "http://127.0.0.1:19999/".to_string();
    app.rebuild_http_api();
    assert!(app.auth_epoch > old_epoch);
    assert!(app.auth.account.is_none());
    assert!(!app.auth.options_checked);
    assert!(!app.auth.checked);
    assert!(!app.auth.options.github_oauth);
    assert!(!app.auth.options.local_admin_login);
    assert!(app.datasets.metadata.is_none());
    assert!(app.datasets.summaries.is_empty());
    assert!(app.work.assignment.is_none());
    assert!(app.runtime.commands.is_empty());
}

#[test]
fn login_discovery_and_failure_states_never_flash_sign_in_methods() {
    for size in [
        egui::vec2(320.0, 320.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(1440.0, 1000.0),
    ] {
        for state in 0..5 {
            let api = Rc::new(SpyApi::new());
            let mut app = base_live_app(api);
            app.auth.checked = true;
            app.auth.options_checked = true;
            app.auth.account = None;
            app.auth.options = AuthOptions {
                github_oauth: true,
                local_admin_login: true,
            };
            let expected = match state {
                0 => {
                    app.auth.options_checked = false;
                    app.loading.session = true;
                    "Loading sign-in options..."
                }
                1 => {
                    app.auth.checked = false;
                    app.loading.session = true;
                    "Checking your session..."
                }
                2 => {
                    app.auth.options_error = Some("Sign-in options could not be loaded. This deliberately long message must wrap within the login page.".to_string());
                    "Could not load sign-in options."
                }
                3 => {
                    app.auth.session_error = Some("Session service unavailable".to_string());
                    "Could not check your session."
                }
                _ => {
                    app.auth.options = AuthOptions {
                        github_oauth: false,
                        local_admin_login: false,
                    };
                    "No interactive sign-in method is enabled on this server."
                }
            };
            let mut harness = Harness::builder().with_size(size).build_eframe(|_| app);
            harness.step();
            assert!(
                harness.query_by_label(expected).is_some(),
                "missing explicit login state"
            );
            assert!(harness.query_by_label("Sign in with GitHub").is_none());
            assert!(harness.query_by_label("Continue as local admin").is_none());
            assert!(harness.query_by_label("API URL").is_none());
            assert!(harness.query_by_label("About").is_some());
        }
    }
}

#[test]
fn login_options_failure_retries_and_about_survives_session_discovery() {
    let api = Rc::new(SpyApi::new());
    api.fail_me();
    let mut app = base_live_app(api.clone());
    app.auth.checked = true;
    app.auth.options_checked = true;
    app.auth.options_error = Some("temporary failure".to_string());
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 780.0))
        .build_eframe(|_| app);
    click(&mut harness, "Retry sign-in options");
    step_until(&mut harness, 8, |app| app.auth.checked);
    assert!(harness.state().auth.options_error.is_none());
    assert!(harness.query_by_label("Sign in with GitHub").is_some());
    click(&mut harness, "About");
    harness.state_mut().request_session();
    harness.state_mut().start_next_command();
    harness.step();
    assert!(harness.query_by_label("About Labello").is_some());
}

#[test]
fn stale_unauthorized_response_cannot_start_session_recovery() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api);
    let app = harness.state_mut();
    let mut request = test_request(app, 9103, None);
    request.auth_epoch = app.auth_epoch.wrapping_sub(1);
    app.runtime.active_requests.insert(request.request_id);
    app.runtime
        .tx
        .send(UiMessage::StatsLoaded {
            request,
            result: Err(ClientError::Api {
                status: 401,
                message: "authentication required".to_string(),
            }
            .into()),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert!(app.auth.recovery.is_none());
    assert!(app.auth.checked);
    assert!(!app.loading.session);
    assert_eq!(app.view, AppView::Annotate);
}

#[test]
fn short_login_scrolls_the_keyboard_focused_action_into_view() {
    let api = Rc::new(SpyApi::new());
    let mut app = base_live_app(api);
    app.auth.checked = true;
    app.auth.options_checked = true;
    app.auth.account = None;
    app.auth.options = AuthOptions {
        github_oauth: false,
        local_admin_login: true,
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 320.0))
        .build_eframe(|_| app);
    harness.step();
    assert!(
        harness
            .get_by_label("Continue as local admin")
            .rect()
            .bottom()
            > 320.0
    );
    for _ in 0..4 {
        harness.key_press(egui::Key::Tab);
        harness.step();
    }
    harness.run_steps(8);
    let button = harness.get_by_label("Continue as local admin");
    assert!(button.is_focused());
    assert!(
        button.rect().bottom() <= 320.0,
        "focused login action remains offscreen"
    );
    assert!(button.rect().top() >= 56.0);
}
