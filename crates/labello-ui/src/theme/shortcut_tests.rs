use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

fn composite(foreground: Color32, background: Color32) -> Color32 {
    let alpha = f32::from(foreground.a()) / 255.0;
    Color32::from_rgb(
        (f32::from(foreground.r()) + f32::from(background.r()) * (1.0 - alpha))
            .round()
            .min(255.0) as u8,
        (f32::from(foreground.g()) + f32::from(background.g()) * (1.0 - alpha))
            .round()
            .min(255.0) as u8,
        (f32::from(foreground.b()) + f32::from(background.b()) * (1.0 - alpha))
            .round()
            .min(255.0) as u8,
    )
}

fn luminance(color: Color32) -> f32 {
    let linear = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn flattened<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
    if let egui::Shape::Vec(shapes) = shape {
        for shape in shapes {
            flattened(shape, out);
        }
    } else {
        out.push(shape);
    }
}

fn painted_text_contrast(harness: &Harness<'_, ()>, text: &str, panel: Color32) -> f32 {
    let mut shapes = Vec::new();
    for shape in &harness.output().shapes {
        flattened(&shape.shape, &mut shapes);
    }
    let (index, rendered) = shapes
        .iter()
        .enumerate()
        .find_map(|(index, shape)| match shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => Some((index, shape)),
            _ => None,
        })
        .expect("actual painted text");
    let center = rendered.visual_bounding_rect().center();
    let mut background = panel;
    for shape in &shapes[..index] {
        if let egui::Shape::Rect(rect) = shape
            && rect.rect.contains(center)
        {
            background = composite(rect.fill, background);
        }
    }
    let glyph = rendered
        .galley
        .rows
        .iter()
        .flat_map(|row| &row.visuals.mesh.vertices)
        .map(|vertex| vertex.color)
        .max_by_key(|color| color.a())
        .expect("painted glyph mesh");
    let color = rendered
        .override_text_color
        .unwrap_or(if glyph == Color32::PLACEHOLDER {
            rendered.fallback_color
        } else {
            glyph
        })
        .gamma_multiply(rendered.opacity_factor);
    contrast(composite(color, background), background)
}

#[test]
fn shortcut_text_meets_contrast_on_composited_button_backgrounds() {
    let mut failures = Vec::new();
    for background in [APP_BG, PANEL, SURFACE, SURFACE_ELEVATED] {
        for kind in ["ordinary", "selected", "primary", "quiet", "danger"] {
            for state in [
                "idle",
                "hover",
                "pressed",
                "focus",
                "disabled",
                "parent-disabled",
            ] {
                let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::NOTHING));
                let observed_rect = rect.clone();
                let mut harness = Harness::builder()
                    .with_size(Vec2::new(600., 160.))
                    .build_ui(move |ui| {
                        if !apply_fallback(ui.ctx()) {
                            return;
                        }
                        Frame::new()
                            .fill(background)
                            .inner_margin(16)
                            .show(ui, |ui| {
                                if state == "parent-disabled" {
                                    ui.disable();
                                }
                                let weak = ui.visuals().weak_text_color;
                                let disabled_alpha = ui.visuals().disabled_alpha;
                                let opacity = ui.opacity();
                                let button =
                                    Button::new("Action").shortcut_text(if kind == "ordinary" {
                                        button_shortcut("Ctrl+Alt+Shift+End")
                                    } else {
                                        RichText::new("Ctrl+Alt+Shift+End")
                                    });
                                let enabled = state != "disabled";
                                let response = match kind {
                                    "primary" => primary_button(ui, enabled, button),
                                    "quiet" => quiet_button(ui, enabled, button),
                                    "danger" => danger_button(ui, enabled, button),
                                    "selected" => super::button(ui, enabled, button.selected(true)),
                                    _ => ui.add_enabled(enabled, button),
                                };
                                assert_eq!(ui.visuals().weak_text_color, weak);
                                assert_eq!(ui.visuals().disabled_alpha, disabled_alpha);
                                assert_eq!(ui.opacity(), opacity);
                                observed_rect.set(response.rect);
                                if state == "focus" {
                                    response.request_focus();
                                }
                                ui.label(RichText::new("Supporting text").weak());
                            });
                    });
                harness.run();
                if state == "hover" || state == "pressed" {
                    harness.hover_at(rect.get().center());
                    harness.run();
                }
                if state == "pressed" {
                    harness.drag_at(rect.get().center());
                    harness.run();
                }
                let ratio = painted_text_contrast(&harness, "Ctrl+Alt+Shift+End", background);
                if ratio < 4.5 {
                    failures.push(format!("{kind}/{state}/{background:?}: {ratio:.2}"));
                }
                let main = painted_text_contrast(&harness, "Action", background);
                if main < 4.5 {
                    failures.push(format!(
                        "main label {kind}/{state}/{background:?}: {main:.2}"
                    ));
                }
                if state == "focus" {
                    let mut shapes = Vec::new();
                    for shape in &harness.output().shapes {
                        flattened(&shape.shape, &mut shapes);
                    }
                    assert!(shapes.iter().any(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.stroke.color == FOCUS_RING && rect.stroke.width >= 1.5 && contrast(composite(rect.stroke.color,background),background) >= 3.0)));
                }
                if state == "disabled" || state == "parent-disabled" {
                    assert!(
                        harness
                            .get_by_role(egui::accesskit::Role::Button)
                            .accesskit_node()
                            .is_disabled()
                    );
                }
                // Shortcut rendering must keep exactly one action.
                assert_eq!(
                    harness
                        .query_all_by_role(egui::accesskit::Role::Button)
                        .count(),
                    1
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn rebound_shortcut_keeps_selection_action_name_and_complete_description() {
    let mut occluded = true;
    let shortcut = "Ctrl+Alt+Shift+ArrowDown";
    let harness = Harness::builder()
        .with_size(Vec2::new(600., 240.))
        .build_ui(move |ui| {
            if !apply_fallback(ui.ctx()) {
                return;
            }
            crate::panels::keypoint_placement_mode(ui, "nose", &mut occluded, shortcut);
        });
    let button =
        harness.get_by_role_and_label(egui::accesskit::Role::Button, "Place nose as occluded");
    assert!(
        button
            .accesskit_node()
            .description()
            .unwrap()
            .contains(shortcut)
    );
    assert_eq!(
        harness
            .query_all_by_role(egui::accesskit::Role::Button)
            .count(),
        2
    );
}

#[test]
fn measured_workspace_shortcuts_keep_contrast_inline_and_in_wrapped_menus() {
    use crate::panels::{WorkspaceAction, WorkspaceCommand, workspace_secondary_actions};
    for menu in [false, true] {
        for enabled in [false, true] {
            let shortcut = "Ctrl+Alt+Shift+ArrowDown";
            let label = "Previous assignment";
            let mut harness = Harness::builder()
                .with_size(Vec2::new(if menu { 320.0 } else { 1100.0 }, 400.0))
                .build_ui(move |ui| {
                    if !apply_fallback(ui.ctx()) {
                        return;
                    }
                    Frame::new().fill(PANEL).inner_margin(16).show(ui, |ui| {
                        ui.set_width(if menu { 120.0 } else { 1000.0 });
                        ui.horizontal_wrapped(|ui| {
                            let action = WorkspaceAction {
                                command: WorkspaceCommand::User(
                                    labello_domain::UserAction::PreviousImage,
                                ),
                                label: label.into(),
                                shortcut: shortcut.into(),
                                enabled,
                                help: "Return to the previous assignment.",
                            };
                            assert!(
                                workspace_secondary_actions(ui, &[action], "More actions")
                                    .is_none()
                            );
                        });
                    });
                });
            harness.run_steps(4);
            if menu {
                harness.get_by_label("More actions").click();
                harness.run_steps(4);
            }
            let action = harness.get_by_label_contains(label);
            assert_eq!(action.accesskit_node().is_disabled(), !enabled);
            assert_eq!(harness.query_all_by_label_contains(label).count(), 1);
            let ratio = painted_text_contrast(&harness, shortcut, PANEL);
            assert!(ratio >= 4.5, "menu={menu}, enabled={enabled}: {ratio:.2}");
            let main = painted_text_contrast(&harness, label, PANEL);
            assert!(
                main >= 4.5,
                "main menu={menu}, enabled={enabled}: {main:.2}"
            );
        }
    }
}
