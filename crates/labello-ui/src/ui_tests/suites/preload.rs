#[test]
fn preload_uses_admin_target_and_blocks_image_requests_when_admission_is_denied() {
    let api = Rc::new(SpyApi::new());
    api.state.borrow_mut().block_prefetch = true;
    api.state.borrow_mut().metadata.preload_queue_size = 100;
    let mut harness = loaded_work_harness(api.clone());
    harness.run_steps(12);
    assert_eq!(harness.state().work.queue.queue_size(), 100);
    assert!(harness.state().work.queue.is_empty());
    assert!(api.state.borrow().prefetch_requests > 0);
    assert_eq!(api.counts().get_encoded_image_preview, 1);
    assert!(!harness.state().loading.image);
}

#[test]
fn preload_resizing_releases_surplus_and_preserves_current_work_for_both_kinds() {
    for review in [false, true] {
        let api = Rc::new(SpyApi::new());
        let mut harness = if review {
            loaded_review_harness(api.clone())
        } else {
            loaded_work_harness(api.clone())
        };
        step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
        if !review {
            click(&mut harness, "Accept");
            assert!(!harness.state().work.annotations.is_empty());
            harness.state_mut().work.last_edit_at = None;
        }
        let current = harness.state().work.assignment.clone().unwrap();
        let annotations = harness.state().work.annotations.clone();
        let first = harness.state().work.queue.prepared_image_ids()[0].clone();
        api.state.borrow_mut().metadata.preload_queue_size = 1;
        harness.state_mut().request_assignment_availability();
        step_until(&mut harness, 12, |app| app.work.queue.queue_size() == 1);
        harness.run_steps(5);
        assert_eq!(harness.state().work.queue.prepared_image_ids(), vec![first]);
        assert_eq!(
            harness
                .state()
                .work
                .assignment
                .as_ref()
                .unwrap()
                .assignment_id,
            current.assignment_id
        );
        assert_eq!(harness.state().work.annotations, annotations);
        if !review {
            assert_eq!(harness.state().work.save_status, SaveStatus::Dirty);
        }
        assert_eq!(api.counts().release_assignment, 1);
        api.state.borrow_mut().metadata.preload_queue_size = 2;
        harness.state_mut().request_assignment_availability();
        step_until(&mut harness, 16, |app| app.work.queue.len() == 2);
    }
}

#[test]
fn preload_reconciliation_does_not_discard_items_newer_than_the_checked_snapshot() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    api.state.borrow_mut().block_prefetch = true;
    let checked = harness.state().work.queue.prepared_assignment_ids()[0].clone();
    let newer = harness.state().work.queue.prepared_image_ids()[1].clone();
    let current = harness.state().work.assignment.clone().unwrap();
    let app = harness.state_mut();
    let request = test_request(app, 991_600, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(request.request_id);
    app.work.availability.refresh_after_load = false;
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            request,
            checked_assignments: vec![checked],
            result: Ok(labello_client::AssignmentAvailability {
                kind: AssignmentKind::Annotation,
                tasks: BTreeMap::from([(current.task_id.clone(), false)]),
                related: Vec::new(),
                queue: Some(labello_client::AssignmentQueueStatus {
                    size: 2,
                    eligible_assignments: Some(Vec::new()),
                }),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.queue.prepared_image_ids(), vec![newer]);
    assert_eq!(
        app.work.assignment.as_ref().unwrap().assignment_id,
        current.assignment_id
    );
}

#[test]
fn preload_expiry_uses_elapsed_time_and_never_promotes_an_expired_entry() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let app = harness.state_mut();
    let mut expired = app.work.queue.pop_prepared().unwrap();
    let valid = app.work.queue.pop_prepared().unwrap();
    let valid_id = valid.assignment.assignment_id.clone();
    expired.prepared_until = Some(Instant::now() - Duration::from_secs(1));
    app.work.queue.push_prepared(expired);
    app.work.queue.push_prepared(valid);
    assert!(app.promote_prepared_assignment(&egui::Context::default(), None));
    assert_eq!(
        app.work.assignment.as_ref().unwrap().assignment_id,
        valid_id
    );
}

#[test]
fn admin_preload_setting_is_persisted_and_accessible() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_admin_harness(api.clone());
    harness.state_mut().admin.section = AdminSection::Automation;
    harness.step();
    assert!(harness.query_by_label("Image preloading").is_some());
    assert!(harness.query_by_label("Upcoming images").is_some());
    harness
        .state_mut()
        .datasets
        .admin_config
        .as_mut()
        .unwrap()
        .preload_queue_size = 100;
    assert!(harness.state_mut().request_admin_save());
    step_until(&mut harness, 12, |app| !app.loading.admin);
    assert_eq!(api.metadata().preload_queue_size, 100);
    assert_eq!(
        harness
            .state()
            .datasets
            .admin_baseline
            .as_ref()
            .unwrap()
            .preload_queue_size,
        100
    );
}

#[test]
fn preload_large_cleanup_bounds_release_concurrency() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let app = harness.state_mut();
    let template = app.work.queue.pop_prepared().unwrap();
    app.work.queue.drain_prepared_assignments();
    app.work.queue.set_queue_size(100);
    for index in 0..100 {
        let mut loaded = template.clone();
        loaded.assignment.assignment_id =
            labello_domain::AssignmentId::from(format!("cleanup_{index}"));
        loaded.assignment.image_id = labello_domain::ImageId::from(format!("cleanup_{index}"));
        app.work.queue.push_prepared(loaded);
    }
    app.resize_preload_queue(1);
    let releases_before = api.counts().release_assignment;
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.queue.len(), 1);
    assert_eq!(api.counts().release_assignment - releases_before, 8);
    assert!(app.runtime.reservation_cleanup.has_pending_releases());
}

#[test]
fn preload_reconciliation_preserves_owned_work_when_no_new_claim_is_available() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_review_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let app = harness.state_mut();
    let checked = app.work.queue.prepared_assignment_ids();
    let request = test_request(app, 991_601, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(request.request_id);
    app.work.availability.refresh_after_load = false;
    app.runtime
        .tx
        .send(UiMessage::AssignmentAvailabilityLoaded {
            request,
            checked_assignments: checked.clone(),
            result: Ok(labello_client::AssignmentAvailability {
                kind: AssignmentKind::Review,
                tasks: BTreeMap::from([(
                    app.work.assignment.as_ref().unwrap().task_id.clone(),
                    false,
                )]),
                related: Vec::new(),
                queue: Some(labello_client::AssignmentQueueStatus {
                    size: 2,
                    eligible_assignments: Some(checked.clone()),
                }),
            }),
        })
        .unwrap();
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.queue.prepared_assignment_ids(), checked);
}

#[test]
fn preload_result_arriving_after_shrink_releases_its_reservation() {
    let api = Rc::new(SpyApi::new());
    let mut harness = loaded_work_harness(api.clone());
    step_until(&mut harness, 12, |app| app.work.queue.len() == 2);
    let app = harness.state_mut();
    let late = app.work.queue.pop_prepared().unwrap();
    app.resize_preload_queue(1);
    let operation_id = 991_602;
    let request = test_request(app, operation_id, Some(app.config.dataset_id.as_str()));
    app.runtime.active_requests.insert(operation_id);
    app.work.active_prefetch_id = Some(operation_id);
    app.work.queue.set_loading(true);
    app.runtime
        .tx
        .send(UiMessage::PrefetchLoaded {
            request,
            operation_id,
            assignment: Some(late.assignment.clone()),
            result: Box::new(Ok(Some(late))),
        })
        .unwrap();
    let releases_before = api.counts().release_assignment;
    app.process_messages(&egui::Context::default());
    assert_eq!(app.work.queue.len(), 1);
    assert_eq!(api.counts().release_assignment - releases_before, 1);
}
