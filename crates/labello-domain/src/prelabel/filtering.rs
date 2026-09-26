use super::*;

/// Filter one image's complete hint set against the current annotation draft.
/// Existing boxes always win. Only identical workflow/class pairs compete.
pub fn filter_prelabels(
    hints: &[PrelabelSuggestion],
    annotations: &[AnnotationVersion],
    processing: &OutputProcessing,
) -> Vec<PrelabelSuggestion> {
    if processing.validate().is_err() {
        return Vec::new();
    }
    let mut candidates: Vec<_> = hints
        .iter()
        .filter(|hint| {
            hint.confidence.is_finite()
                && (0.0..=1.0).contains(&hint.confidence)
                && hint.passes(processing)
                && hint.geometry.validate().is_ok()
        })
        .cloned()
        .collect();
    candidates.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.suggestion_id.cmp(&b.suggestion_id))
            .then_with(|| a.config_id.cmp(&b.config_id))
    });
    let mut kept: Vec<PrelabelSuggestion> = Vec::new();
    for hint in candidates {
        if kept
            .iter()
            .any(|old| old.suggestion_id == hint.suggestion_id)
        {
            continue;
        }
        if let AnnotationGeometry::BoundingBox(bounds) = hint.geometry {
            let matches = |task: &TaskId, class: &ClassId, geometry: &AnnotationGeometry| {
                task == &hint.task_id
                    && class == &hint.class_id
                    && matches!(geometry, AnnotationGeometry::BoundingBox(other)
                        if bounds.iou(*other) > processing.iou_threshold())
            };
            if annotations
                .iter()
                .any(|a| !a.deleted && matches(&a.task_id, &a.class_id, &a.geometry))
                || kept
                    .iter()
                    .any(|h| matches(&h.task_id, &h.class_id, &h.geometry))
            {
                continue;
            }
        }
        kept.push(hint);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnnotationId, AnnotationType, BoundingBox, UserId, now};

    fn hint(id: &str, confidence: f32, x: f32, width: f32) -> PrelabelSuggestion {
        PrelabelSuggestion {
            suggestion_id: id.into(),
            config_id: "model".into(),
            task_id: "boxes".into(),
            class_id: "person".into(),
            confidence,
            geometry: AnnotationGeometry::BoundingBox(BoundingBox {
                x,
                y: 0.0,
                width,
                height: 0.5,
            }),
            evidence: None,
        }
    }
    fn processing() -> OutputProcessing {
        OutputProcessing {
            confidence_threshold: 0.2,
            suppress_overlaps_iou: None,
        }
    }

    #[test]
    fn current_boxes_win_and_other_workflows_classes_and_deleted_boxes_do_not_suppress() {
        let h = hint("hint", 0.99, 0.0, 0.5);
        let mut annotation = AnnotationVersion::native(
            AnnotationId::from("human"),
            h.task_id.clone(),
            h.class_id.clone(),
            AnnotationType::BoundingBox,
            h.geometry.clone(),
            UserId::from("u"),
            now(),
        );
        assert!(
            filter_prelabels(
                std::slice::from_ref(&h),
                &[annotation.clone()],
                &processing()
            )
            .is_empty()
        );
        annotation.class_id = "vehicle".into();
        assert_eq!(
            filter_prelabels(
                std::slice::from_ref(&h),
                &[annotation.clone()],
                &processing()
            )
            .len(),
            1
        );
        annotation.class_id = h.class_id.clone();
        annotation.task_id = "other".into();
        assert_eq!(
            filter_prelabels(
                std::slice::from_ref(&h),
                &[annotation.clone()],
                &processing()
            )
            .len(),
            1
        );
        annotation.task_id = h.task_id.clone();
        annotation.deleted = true;
        assert_eq!(
            filter_prelabels(&[h], &[annotation], &processing()).len(),
            1
        );
    }

    #[test]
    fn nms_merges_sources_with_stable_confidence_ties_and_strict_threshold() {
        let a = hint("a", 0.9, 0.0, 0.5);
        let mut b = hint("b", 0.9, 0.0, 0.5);
        b.config_id = "other".into();
        assert_eq!(
            filter_prelabels(&[b.clone(), a.clone()], &[], &processing()),
            vec![a.clone()]
        );
        assert_eq!(
            filter_prelabels(&[a.clone(), b], &[], &processing()),
            vec![a.clone()]
        );
        // A contained box with half the area has IoU exactly 0.5 and remains.
        let half = hint("half", 0.8, 0.0, 0.25);
        assert_eq!(filter_prelabels(&[a, half], &[], &processing()).len(), 2);
        assert_eq!(processing().iou_threshold(), 0.5);
    }

    #[test]
    fn invalid_geometry_confidence_and_thresholds_never_produce_hints() {
        let mut invalid = hint("bad", f32::NAN, 0.0, 0.5);
        assert!(filter_prelabels(&[invalid.clone()], &[], &processing()).is_empty());
        invalid.confidence = 0.9;
        invalid.geometry = AnnotationGeometry::BoundingBox(crate::BoundingBox {
            x: 2.0,
            y: 0.0,
            width: 0.1,
            height: 0.1,
        });
        assert!(filter_prelabels(&[invalid], &[], &processing()).is_empty());
        let mut p = processing();
        p.suppress_overlaps_iou = Some(f32::NAN);
        assert!(filter_prelabels(&[hint("h", 0.9, 0.0, 0.5)], &[], &p).is_empty());
    }
}
