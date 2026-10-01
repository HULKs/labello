fn paint_workflow_boost(
    ui: &egui::Ui,
    response: &egui::Response,
    window: Option<i64>,
) {
    let identity = response.id.with("workflow-boost");
    let Some(window) = window else {
        ui.ctx()
            .data_mut(|data| data.remove::<(i64, bool, Option<f64>)>(identity));
        return;
    };
    if !ui.is_rect_visible(response.rect) {
        return;
    }
    let time = ui.input(|input| input.time);
    let engaged = response.hovered() || response.has_focus();
    let reduced = ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::new("reduced-motion"))
            .unwrap_or(true)
    });
    let phase = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<(i64, bool, Option<f64>)>(identity);
        let mut started = previous.and_then(|(_, _, start)| start);
        if previous.is_none_or(|(old, was_engaged, _)| old != window || engaged && !was_engaged) {
            started = Some(time);
        }
        if reduced || !response.enabled() || started.is_some_and(|start| time - start >= 1.4) {
            started = None;
        }
        data.insert_temp(identity, (window, engaged, started));
        started.map(|start| ((time - start) / 1.4) as f32)
    });
    let strength = if response.enabled() { 1.0 } else { 0.4 };
    let color = theme::WARNING.gamma_multiply(strength);
    // Keep the native outer border available for keyboard focus and selection.
    let rect = response.rect.shrink(2.5);
    let button_radius = theme::SURFACE_RADIUS;
    let radius = (f32::from(button_radius) - 2.5).min(rect.height() / 2.0);
    let painter = ui
        .painter()
        .with_clip_rect(ui.clip_rect().intersect(response.rect.expand(5.0)));
    painter.add(
        egui::epaint::RectShape::stroke(
            response.rect.shrink(1.0),
            button_radius,
            egui::Stroke::new(4.0, color.gamma_multiply(0.5)),
            egui::StrokeKind::Middle,
        )
        .with_blur_width(8.0),
    );
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );
    if let Some(phase) = phase {
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), phase);
        let intensity = (phase * std::f32::consts::PI).sin();
        for step in -12..=12 {
            let px = x + step as f32 * 1.5;
            if px < rect.left() + radius || px > rect.right() - radius {
                continue;
            }
            let alpha = (1.0 - (step as f32 / 13.0).abs()) * intensity;
            painter.line_segment(
                [
                    egui::pos2(px, rect.top() + 1.0),
                    egui::pos2(px, rect.top() + 3.0),
                ],
                egui::Stroke::new(2.0, theme::TEXT.gamma_multiply(alpha)),
            );
            painter.line_segment(
                [
                    egui::pos2(px, rect.bottom() - 3.0),
                    egui::pos2(px, rect.bottom() - 1.0),
                ],
                egui::Stroke::new(2.0, color.gamma_multiply(alpha)),
            );
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}
