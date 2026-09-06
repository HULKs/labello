#[allow(
    clippy::too_many_arguments,
    reason = "painting is a leaf operation over explicit immutable render inputs"
)]
fn paint_canvas(
    ui: &Ui,
    viewport: Rect,
    image_rect: Rect,
    texture: Option<&egui::TextureHandle>,
    annotations: &[AnnotationVersion],
    selected_annotation: Option<&AnnotationId>,
    editable: bool,
    edit_preview: Option<(&AnnotationId, BoundingBox)>,
    draft_box: Option<BoundingBox>,
    selected_keypoint: Option<usize>,
    keypoint_preview: Option<(&AnnotationId, usize, NormalizedPoint)>,
    skeleton_edges: &[(String, String)],
    prelabels: &[PrelabelSuggestion],
    annotation_color: Color32,
    annotation_styles: &std::collections::BTreeMap<AnnotationId, CanvasAnnotationStyle>,
    zoom: f32,
) {
    let painter = ui.painter_at(viewport);
    painter.rect_filled(
        viewport,
        CornerRadius::same(VIEWPORT_CORNER_RADIUS),
        theme::INPUT_BG,
    );
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            image_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            viewport.center(),
            egui::Align2::CENTER_CENTER,
            "Image preview unavailable",
            egui::FontId::new(theme::BODY_SIZE, egui::FontFamily::Proportional),
            theme::TEXT_MUTED,
        );
        ui.interact(
            viewport,
            ui.id().with("image_preview_unavailable"),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, "Image preview unavailable"));
    }

    for suggestion in prelabels {
        let color = theme::PRELABEL;
        match &suggestion.geometry {
            AnnotationGeometry::BoundingBox(bbox) => {
                let rect = bbox_to_screen_rect(image_rect, *bbox);
                paint_dashed_segment(&painter, rect.left_top(), rect.right_top(), color);
                paint_dashed_segment(&painter, rect.right_top(), rect.right_bottom(), color);
                paint_dashed_segment(&painter, rect.right_bottom(), rect.left_bottom(), color);
                paint_dashed_segment(&painter, rect.left_bottom(), rect.left_top(), color);
            }
            AnnotationGeometry::Skeleton(skeleton) => {
                for keypoint in &skeleton.keypoints {
                    if keypoint.state != KeypointState::Absent && let Some(point) = keypoint.point {
                        paint_keypoint(
                            &painter,
                            normalized_to_screen(image_rect, pos2(point.x, point.y)),
                            &keypoint.state,
                            color,
                            6.0,
                            true,
                        );
                    }
                }
            }
        }
    }

    for annotation in annotations.iter().filter(|annotation| !annotation.deleted) {
        let selected = selected_annotation == Some(&annotation.annotation_id);
        let style = annotation_styles
            .get(&annotation.annotation_id)
            .copied()
            .unwrap_or_else(|| CanvasAnnotationStyle::solid(annotation_color));
        match &annotation.geometry {
            AnnotationGeometry::BoundingBox(bbox) => {
                let bbox = edit_preview
                    .filter(|(id, _)| *id == &annotation.annotation_id)
                    .map_or(*bbox, |(_, preview)| preview);
                if selected {
                    paint_selected_box(&painter, image_rect, bbox, editable, style.color);
                } else if style.dashed_box {
                    paint_context_box(&painter, image_rect, bbox, style.color, zoom);
                } else {
                    paint_existing_box(&painter, image_rect, bbox, style.color);
                }
            }
            AnnotationGeometry::Skeleton(skeleton) => {
                let color = style.color;
                for (from, to) in skeleton_edges {
                    let from = skeleton_keypoint_point(
                        &annotation.annotation_id,
                        skeleton,
                        from,
                        keypoint_preview,
                    );
                    let to = skeleton_keypoint_point(
                        &annotation.annotation_id,
                        skeleton,
                        to,
                        keypoint_preview,
                    );
                    if let (Some(from), Some(to)) = (from, to) {
                        paint_outlined_segment(
                            &painter,
                            [
                                normalized_to_screen(image_rect, pos2(from.x, from.y)),
                                normalized_to_screen(image_rect, pos2(to.x, to.y)),
                            ],
                            color,
                            if selected { 2.0 } else { 1.5 },
                        );
                    }
                }
                for (keypoint_index, keypoint) in skeleton.keypoints.iter().enumerate() {
                    if keypoint.state != KeypointState::Absent && let Some(point) = keypoint.point {
                        let point = previewed_keypoint_point(
                            &annotation.annotation_id,
                            keypoint_index,
                            point,
                            keypoint_preview,
                        );
                        let center = normalized_to_screen(image_rect, pos2(point.x, point.y));
                        paint_keypoint(&painter, center, &keypoint.state, color, if selected { 5.0 } else { 4.0 }, false);
                        if selected && selected_keypoint == Some(keypoint_index) {
                            for stroke in overlay_strokes(theme::FOCUS_RING, 2.0) {
                                painter.circle_stroke(center, 14.0, stroke);
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(bbox) = draft_box {
        paint_draft_box(&painter, image_rect, bbox);
    }
    painter.add(rounded_corner_mask(viewport, ui.visuals().panel_fill));
    painter.rect_stroke(
        viewport,
        CornerRadius::same(VIEWPORT_CORNER_RADIUS),
        Stroke::new(1.0, theme::BORDER_STRONG),
        StrokeKind::Inside,
    );
}

fn skeleton_keypoint_point(
    annotation_id: &AnnotationId,
    skeleton: &labello_domain::SkeletonGeometry,
    keypoint_name: &str,
    keypoint_preview: Option<(&AnnotationId, usize, NormalizedPoint)>,
) -> Option<NormalizedPoint> {
    skeleton
        .keypoints
        .iter()
        .enumerate()
        .find(|(_, keypoint)| keypoint.name == keypoint_name)
        .filter(|(_, keypoint)| keypoint.state != KeypointState::Absent)
        .and_then(|(keypoint_index, keypoint)| {
            keypoint.point.map(|point| {
                previewed_keypoint_point(
                    annotation_id,
                    keypoint_index,
                    point,
                    keypoint_preview,
                )
            })
        })
}

fn previewed_keypoint_point(
    annotation_id: &AnnotationId,
    keypoint_index: usize,
    point: NormalizedPoint,
    keypoint_preview: Option<(&AnnotationId, usize, NormalizedPoint)>,
) -> NormalizedPoint {
    keypoint_preview
        .filter(|(id, index, _)| *id == annotation_id && *index == keypoint_index)
        .map_or(point, |(_, _, preview)| preview)
}

fn rounded_corner_mask(viewport: Rect, color: Color32) -> Mesh {
    let radius = f32::from(VIEWPORT_CORNER_RADIUS)
        .min(viewport.width() * 0.5)
        .min(viewport.height() * 0.5);
    let mut mesh = Mesh::default();
    if radius <= 0.0 {
        return mesh;
    }

    let corners = [
        (
            viewport.left_top(),
            viewport.left_top() + vec2(radius, radius),
            PI,
            PI * 1.5,
        ),
        (
            viewport.right_top(),
            viewport.right_top() + vec2(-radius, radius),
            PI * 1.5,
            PI * 2.0,
        ),
        (
            viewport.right_bottom(),
            viewport.right_bottom() - vec2(radius, radius),
            0.0,
            PI * 0.5,
        ),
        (
            viewport.left_bottom(),
            viewport.left_bottom() + vec2(radius, -radius),
            PI * 0.5,
            PI,
        ),
    ];
    for (outer, center, start, end) in corners {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(outer, color);
        for step in 0..=CORNER_MASK_SEGMENTS {
            let angle = start + (end - start) * step as f32 / CORNER_MASK_SEGMENTS as f32;
            mesh.colored_vertex(center + vec2(angle.cos(), angle.sin()) * radius, color);
        }
        for step in 0..CORNER_MASK_SEGMENTS {
            mesh.add_triangle(base, base + step + 1, base + step + 2);
        }
    }
    mesh
}

// A single one-point halo separates the class color from image content without
// turning small markers and thin edges into concentric black/white bands.
fn overlay_outline(color: Color32) -> Color32 {
    let linear = egui::Rgba::from(color);
    let luminance = 0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b();
    if luminance > 0.179 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

fn overlay_strokes(color: Color32, width: f32) -> [Stroke; 2] {
    [Stroke::new(width + 2.0, overlay_outline(color)), Stroke::new(width, color)]
}

fn paint_outlined_segment(painter: &egui::Painter, points: [Pos2; 2], color: Color32, width: f32) {
    for stroke in overlay_strokes(color, width) {
        painter.line_segment(points, stroke);
    }
}

fn paint_outlined_rect(painter: &egui::Painter, rect: Rect, radius: u8, color: Color32, width: f32) {
    for stroke in overlay_strokes(color, width) {
        painter.rect_stroke(rect, CornerRadius::same(radius), stroke, StrokeKind::Middle);
    }
}

fn paint_keypoint(painter: &egui::Painter, center: Pos2, state: &KeypointState, color: Color32, radius: f32, suggestion: bool) {
    match state {
        KeypointState::Visible if !suggestion => {
            painter.circle_filled(center, radius + 1.0, overlay_outline(color));
            painter.circle_filled(center, radius, color);
        }
        KeypointState::Visible => {
            for stroke in overlay_strokes(color, 2.0) {
                painter.circle_stroke(center, radius, stroke);
            }
        }
        KeypointState::Hidden => {
            let radius = radius + 2.0;
            let points = [center + vec2(0.0, -radius), center + vec2(radius, 0.0), center + vec2(0.0, radius), center + vec2(-radius, 0.0)];
            for stroke in overlay_strokes(color, 2.0) {
                painter.add(egui::Shape::closed_line(points.to_vec(), stroke));
            }
        }
        KeypointState::Absent => {}
    }
}

fn paint_existing_box(
    painter: &egui::Painter,
    image_rect: Rect,
    bbox: BoundingBox,
    color: Color32,
) {
    let rect = bbox_to_screen_rect(image_rect, bbox);
    painter.rect_filled(
        rect,
        CornerRadius::same(4),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 14),
    );
    paint_outlined_rect(painter, rect, 4, color, 1.5);
}

fn paint_context_box(
    painter: &egui::Painter,
    image_rect: Rect,
    bbox: BoundingBox,
    color: Color32,
    zoom: f32,
) {
    let rect = bbox_to_screen_rect(image_rect, bbox);
    painter.rect_filled(
        rect,
        CornerRadius::same(4),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 7),
    );
    paint_dashed_segment_with_gap(painter, rect.left_top(), rect.right_top(), color, context_box_dash_gap(zoom));
    paint_dashed_segment_with_gap(painter, rect.right_top(), rect.right_bottom(), color, context_box_dash_gap(zoom));
    paint_dashed_segment_with_gap(painter, rect.right_bottom(), rect.left_bottom(), color, context_box_dash_gap(zoom));
    paint_dashed_segment_with_gap(painter, rect.left_bottom(), rect.left_top(), color, context_box_dash_gap(zoom));
}

fn paint_selected_box(
    painter: &egui::Painter,
    image_rect: Rect,
    bbox: BoundingBox,
    editable: bool,
    color: Color32,
) {
    let rect = bbox_to_screen_rect(image_rect, bbox);
    painter.rect_filled(
        rect,
        CornerRadius::same(5),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28),
    );
    paint_outlined_rect(painter, rect, 5, color, 2.0);

    if editable {
        for (_, center) in resize_handles(rect) {
            let handle = Rect::from_center_size(center, Vec2::splat(HANDLE_SIZE));
            painter.rect_filled(handle, CornerRadius::same(2), Color32::WHITE);
            paint_outlined_rect(painter, handle, 2, theme::SELECTION, 1.5);
        }
    }
}

fn paint_draft_box(painter: &egui::Painter, image_rect: Rect, bbox: BoundingBox) {
    let rect = bbox_to_screen_rect(image_rect, bbox);
    let color = theme::DRAFT;
    painter.rect_filled(
        rect,
        CornerRadius::same(3),
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
    );
    paint_dashed_segment(painter, rect.left_top(), rect.right_top(), color);
    paint_dashed_segment(painter, rect.right_top(), rect.right_bottom(), color);
    paint_dashed_segment(painter, rect.right_bottom(), rect.left_bottom(), color);
    paint_dashed_segment(painter, rect.left_bottom(), rect.left_top(), color);
}

// Screen-space gaps grow toward Fit so background guides stay quiet when
// several objects are visible. Dash length and stroke width stay readable.
fn context_box_dash_gap(zoom: f32) -> f32 {
    10.0 + 14.0 / finite_or(zoom, MIN_ZOOM).clamp(MIN_ZOOM, MAX_ZOOM)
}

fn paint_dashed_segment(painter: &egui::Painter, start: Pos2, end: Pos2, color: Color32) {
    paint_dashed_segment_with_gap(painter, start, end, color, 10.0);
}

fn paint_dashed_segment_with_gap(painter: &egui::Painter, start: Pos2, end: Pos2, color: Color32, gap: f32) {
    if !start.x.is_finite() || !start.y.is_finite() || !end.x.is_finite() || !end.y.is_finite() {
        return;
    }
    let vector = end - start;
    let length = vector.length();
    if !length.is_finite() || length <= f32::EPSILON {
        return;
    }
    let direction = vector / length;
    let mut offset = 0.0;
    for _ in 0..MAX_DASH_SEGMENTS {
        if offset >= length {
            break;
        }
        let dash_end = (offset + 8.0).min(length);
        if !dash_end.is_finite() || dash_end <= offset {
            break;
        }
        paint_outlined_segment(
            painter,
            [start + direction * offset, start + direction * dash_end],
            color,
            2.0,
        );
        let next = offset + 8.0 + gap;
        if !next.is_finite() || next <= offset {
            break;
        }
        offset = next;
    }
}
