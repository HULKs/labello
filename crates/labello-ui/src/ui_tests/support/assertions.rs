pub(super) fn assert_control_inside(
    harness: &Harness<'static, LabelloApp>,
    label: &str,
    role: egui::accesskit::Role,
    width: f32,
    height: f32,
) {
    let node = harness
        .query_all_by_role_and_label(role, label)
        .next()
        .or_else(|| {
            harness
                .query_all_by_label_contains(label)
                .find(|node| node.accesskit_node().role() == role)
        })
        .unwrap_or_else(|| panic!("No {role:?} found containing {label:?}"));
    let rect = node.rect();
    assert!(
        rect.left() >= -0.5
            && rect.top() >= -0.5
            && rect.right() <= width + 0.5
            && rect.bottom() <= height + 0.5,
        "{label:?} is outside {width}x{height}: {rect:?}",
    );
    if role == egui::accesskit::Role::Button {
        assert!(
            rect.height() >= 43.0,
            "{label:?} touch target is shorter than 44px: {rect:?}",
        );
    }
}
pub(super) fn assert_label_inside(
    harness: &Harness<'static, LabelloApp>,
    label: &str,
    width: f32,
    height: f32,
) {
    let rect = harness.get_by_label(label).rect();
    assert!(
        rect.left() >= -0.5
            && rect.top() >= -0.5
            && rect.right() <= width + 0.5
            && rect.bottom() <= height + 0.5,
        "{label:?} is outside {width}x{height}: {rect:?}",
    );
}

pub(super) fn assert_canvas_geometry(
    harness: &Harness<'static, LabelloApp>,
    width: f32,
    height: f32,
) {
    let canvas = harness.get_by_label("Annotation canvas").rect();
    let dataset = harness.get_by_label_contains("Dataset ").rect();
    assert!(
        canvas.top() >= dataset.bottom(),
        "canvas overlaps the top shell"
    );
    assert!(
        canvas.left() >= -0.5
            && canvas.top() >= -0.5
            && canvas.right() <= width + 0.5
            && canvas.bottom() <= height + 0.5,
        "canvas is outside {width}x{height}: {canvas:?}",
    );
    let minimum = if width < 600.0 { 200.0 } else { 360.0 };
    assert!(
        canvas.height() >= minimum,
        "canvas is not useful at {width}x{height}: {canvas:?}",
    );
}

pub(super) fn assert_visible_controls_clamped(
    harness: &Harness<'static, LabelloApp>,
    width: f32,
    height: f32,
) {
    for role in [
        egui::accesskit::Role::Button,
        egui::accesskit::Role::CheckBox,
        egui::accesskit::Role::ComboBox,
        egui::accesskit::Role::TextInput,
    ] {
        for node in harness.query_all_by_role(role) {
            let rect = node.rect();
            // Scroll areas retain accessibility nodes just outside their clip rect.
            // Check horizontal containment for controls that are fully visible vertically.
            if rect.top() < 0.0 || rect.bottom() > height {
                continue;
            }
            assert!(
                rect.left() >= -0.5
                    && rect.right() <= width + 0.5
                    && rect.left().is_finite()
                    && rect.right().is_finite(),
                "visible {role:?} is outside {width}x{height}: {rect:?}\n{node:?}",
            );
        }
    }
}

pub(super) fn assert_clipped_region(label: &str, rect: egui::Rect, clip: egui::Rect) {
    assert!(
        rect.is_finite() && clip.contains_rect(rect),
        "{label} exceeds its actual clip region"
    );
}

pub(super) fn assert_sibling_spacing(regions: &[(&str, egui::Rect)], minimum: f32) {
    for (index, (left_name, left)) in regions.iter().enumerate() {
        for (right_name, right) in &regions[index + 1..] {
            let horizontal = (right.left() - left.right()).max(left.left() - right.right());
            let vertical = (right.top() - left.bottom()).max(left.top() - right.bottom());
            assert!(
                horizontal >= minimum || vertical >= minimum,
                "{left_name} and {right_name} overlap or lack required spacing"
            );
        }
    }
}

pub(super) fn assert_padded_content(container: egui::Rect, content: egui::Rect, padding: f32) {
    assert!(
        container.shrink(padding).contains_rect(content),
        "content lacks deliberate padding"
    );
}

pub(super) fn assert_row_alignment(rows: &[egui::Rect], tolerance: f32) {
    if let Some(first) = rows.first() {
        assert!(
            rows.iter()
                .all(|row| (row.left() - first.left()).abs() <= tolerance
                    && (row.right() - first.right()).abs() <= tolerance),
            "repeated rows are misaligned"
        );
    }
}

pub(super) fn assert_touch_target(rect: egui::Rect) {
    assert!(
        rect.width() >= 43.0 && rect.height() >= 43.0,
        "interactive target is smaller than 44 points"
    );
}

pub(super) fn assert_scroll_reachable(
    harness: &mut Harness<'static, LabelloApp>,
    label: &str,
    role: egui::accesskit::Role,
    clip: egui::Rect,
) {
    harness.get_by_role_and_label(role, label).scroll_to_me();
    for _ in 0..12 {
        harness.step();
    }
    let node = harness.get_by_role_and_label(role, label);
    assert_clipped_region(label, node.rect(), clip);
    if role == egui::accesskit::Role::Button {
        assert_touch_target(node.rect());
    }
}

pub(super) fn assert_complete_accessible_value(
    harness: &Harness<'static, LabelloApp>,
    value: &str,
    role: egui::accesskit::Role,
    clip: egui::Rect,
) {
    // Exact matching proves the complete value survived visual wrapping or
    // truncation. A shortened display string cannot satisfy this lookup.
    let node = harness.get_by_role_and_label(role, value);
    assert_clipped_region(value, node.rect(), clip);
}
