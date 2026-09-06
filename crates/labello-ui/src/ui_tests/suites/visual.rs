#[test]
fn structural_assertions_reject_clipping_overlap_padding_alignment_and_small_targets() {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 44.0));
    assert!(
        std::panic::catch_unwind(|| assert_clipped_region("clipped", rect, rect.shrink(1.0)))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| assert_sibling_spacing(&[("a", rect), ("b", rect)], 4.0))
            .is_err()
    );
    assert!(std::panic::catch_unwind(|| assert_padded_content(rect, rect, 4.0)).is_err());
    assert!(
        std::panic::catch_unwind(|| assert_row_alignment(
            &[rect, rect.translate(egui::vec2(10.0, 50.0))],
            0.5
        ))
        .is_err()
    );
    assert!(std::panic::catch_unwind(|| assert_touch_target(rect.shrink(2.0))).is_err());
    assert_clipped_region("inside", rect.shrink(2.0), rect);
    assert_sibling_spacing(
        &[("a", rect), ("b", rect.translate(egui::vec2(0.0, 48.0)))],
        4.0,
    );
    assert_padded_content(rect, rect.shrink(4.0), 4.0);
    assert_row_alignment(&[rect, rect.translate(egui::vec2(0.0, 48.0))], 0.5);
    assert_touch_target(rect);
}

#[test]
fn shared_display_matrix_preserves_canvas_and_primary_action_boundaries() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../scripts/browser/matrix.json")).unwrap();
    for viewport in matrix["viewports"].as_array().unwrap() {
        let width = viewport["width"].as_f64().unwrap() as f32;
        let height = viewport["height"].as_f64().unwrap() as f32;
        let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
        harness.set_size(egui::vec2(width, height));
        harness.step();
        harness.step();
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
        let canvas = harness.get_by_label("Annotation canvas").rect();
        assert_clipped_region("canvas", canvas, clip);
        let mut regions = vec![("canvas", canvas)];
        for label in ["Submit & next", "More actions"] {
            let node = harness.get_by_role_and_label(egui::accesskit::Role::Button, label);
            let rect = node.rect();
            assert_clipped_region(label, rect, clip);
            assert_touch_target(rect);
            regions.push((label, rect));
        }
        assert_sibling_spacing(&regions, 0.0);
        if harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "Skip")
            .is_none()
        {
            click(&mut harness, "More actions");
            let skip = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Skip X");
            assert_clipped_region("overflow Skip", skip.rect(), clip);
            harness.key_press(egui::Key::Escape);
            harness.step();
        }
    }
}

#[test]
fn compact_shortcut_rows_separate_complete_labels_from_controls_and_footer_rows() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    harness.set_size(egui::vec2(320.0, 568.0));
    harness.state_mut().work.show_settings = true;
    harness.step();
    harness.step();
    let label = harness.get_by_label("Submit and next").rect();
    let record = harness
        .get_by_label_contains("Record shortcut for Submit and next:")
        .rect();
    assert_sibling_spacing(
        &[("complete action label", label), ("record control", record)],
        4.0,
    );
    let restore = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Restore all defaults")
        .rect();
    let cancel = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Cancel")
        .rect();
    assert!(
        cancel.top() - restore.bottom() <= 16.0,
        "footer rows must not reserve unused canvas height"
    );
}

#[test]
fn compact_shortcut_list_reaches_its_last_complete_action_and_footer() {
    let mut harness = loaded_work_harness(Rc::new(SpyApi::new()));
    for height in [568.0, 320.0] {
        harness.set_size(egui::vec2(320.0, height));
        harness.state_mut().work.show_settings = true;
        harness.step();
        harness.step();
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, height));
        let label = "Record shortcut for Reject review object: N";
        assert_scroll_reachable(&mut harness, label, egui::accesskit::Role::Button, clip);
        assert_complete_accessible_value(&harness, label, egui::accesskit::Role::Button, clip);
        assert_scroll_reachable(
            &mut harness,
            "Save changes",
            egui::accesskit::Role::Button,
            clip,
        );
        harness.key_press(egui::Key::Escape);
        harness.step();
        assert!(harness.query_by_label("Keyboard shortcuts").is_none());
    }
}
