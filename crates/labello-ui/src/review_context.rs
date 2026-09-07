use labello_client::MigrationReviewTarget;
use labello_domain::{
    AnnotationId, AnnotationType, AssignmentKind, AssignmentStatus, MigrationDispositionStatus,
    ReviewDecision, ReviewTarget,
};

use crate::app::{AppView, LabelloApp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReviewContext {
    pub workflow_name: String,
    pub class_name: String,
    pub annotation_type: AnnotationType,
    pub phase: ReviewContextPhase,
    pub decision: Option<ReviewDecision>,
    pub revision_mode: bool,
    pub staged_decision: Option<ReviewDecision>,
    pub correction: Option<CorrectionContext>,
    pub preview_unavailable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReviewContextPhase {
    Object {
        number: usize,
        total: usize,
        kind: &'static str,
        annotation_version: Option<u32>,
        disposition_version: Option<u32>,
    },
    FullImage {
        migration: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CorrectionContext {
    pub base_version: u32,
    pub unsaved_input: bool,
}

impl ReviewContext {
    pub fn type_label(&self) -> &'static str {
        match self.annotation_type {
            AnnotationType::BoundingBox => "Bounding boxes",
            AnnotationType::Skeleton => "Skeletons",
        }
    }

    pub fn phase_label(&self) -> String {
        match self.phase {
            ReviewContextPhase::Object { number, total, .. } => {
                format!("Object {number} of {total}")
            }
            ReviewContextPhase::FullImage { .. } => "Final check / Full image".to_string(),
        }
    }

    pub fn details(&self) -> Vec<String> {
        let mut lines = vec![
            format!("Workflow: {}", self.workflow_name),
            format!("Class: {} · Type: {}", self.class_name, self.type_label()),
            format!("Review position: {}", self.phase_label()),
        ];
        match self.phase {
            ReviewContextPhase::Object {
                kind,
                annotation_version,
                disposition_version,
                ..
            } => {
                if kind != "Annotation review" {
                    lines.push(format!("Target: {kind}"));
                }
                if let Some(version) = annotation_version {
                    lines.push(if let Some(correction) = &self.correction {
                        format!("Base persisted version {}", correction.base_version)
                    } else {
                        format!("Persisted version {version}")
                    });
                }
                if let Some(version) = disposition_version {
                    lines.push(format!("Disposition version {version}"));
                }
            }
            ReviewContextPhase::FullImage { migration: true } => {
                lines.push("Migration full-image confirmation".to_string())
            }
            ReviewContextPhase::FullImage { migration: false } => {}
        }
        if let Some(correction) = &self.correction {
            lines.push("Mode: Correction mode".to_string());
            lines.push(
                if correction.unsaved_input {
                    "Unsaved correction input"
                } else {
                    "No correction changes yet"
                }
                .to_string(),
            );
        }
        if self.revision_mode {
            lines.push("Decision revision mode".to_string());
        }
        let decision = match self.decision {
            Some(ReviewDecision::Approved) => "Approved",
            Some(ReviewDecision::Rejected) => "Rejected",
            None => "Not reviewed",
        };
        lines.push(format!(
            "{} decision: {decision}",
            if self.revision_mode {
                "Effective"
            } else {
                "Current"
            }
        ));
        if let Some(staged) = &self.staged_decision {
            lines.push(format!(
                "Staged decision: {} (not committed)",
                match staged {
                    ReviewDecision::Approved => "Approved",
                    ReviewDecision::Rejected => "Rejected",
                }
            ));
        }
        lines
    }

    pub fn accessible_summary(&self) -> String {
        self.details().join(". ")
    }
}

impl LabelloApp {
    pub(crate) fn review_context(&self) -> Option<ReviewContext> {
        if self.view != AppView::Review
            || self.loading.image
            || self.loading.dataset
            || self.loading.session
            || self.loading.logout
        {
            return None;
        }
        let assignment = self.work.assignment.as_ref()?;
        let state = self.work.current_state.as_ref()?;
        let task = self.selected_task()?;
        if assignment.kind != AssignmentKind::Review
            || assignment.status != AssignmentStatus::Active
            || assignment.assigned_to != self.config.user_id
            || assignment.image_id != state.image_id
            || assignment.task_id != task.task_id
            || self
                .work
                .current
                .as_ref()
                .is_some_and(|current| current.image.image_id != state.image_id)
        {
            return None;
        }
        if state
            .assignments
            .iter()
            .find(|known| known.assignment_id == assignment.assignment_id)
            .is_some_and(|known| {
                known.status != AssignmentStatus::Active
                    || known.kind != AssignmentKind::Review
                    || known.assigned_to != assignment.assigned_to
                    || known.task_id != assignment.task_id
            })
        {
            return None;
        }
        let class = self
            .work
            .classes
            .iter()
            .find(|class| Some(&class.class_id) == self.selected_class_id())?;
        let canonical_targets = state.review_object_targets(task).ok()?;
        let (target, phase, annotation_id) = if self.manual_migration_active() {
            self.migration_review_context_target(&canonical_targets)?
        } else {
            let objects = self
                .work
                .annotations
                .iter()
                .filter(|annotation| {
                    !annotation.deleted && self.annotation_matches_selected_workflow(annotation)
                })
                .collect::<Vec<_>>();
            let displayed_targets = objects
                .iter()
                .map(|annotation| ReviewTarget::AnnotationVersion {
                    annotation_id: annotation.annotation_id.clone(),
                    version: annotation.version,
                })
                .collect::<Vec<_>>();
            if displayed_targets != canonical_targets {
                return None;
            }
            if let Some(annotation) = self.current_review_annotation() {
                let persisted = state.current_annotation(&annotation.annotation_id)?;
                if persisted.deleted
                    || persisted.version != annotation.version
                    || persisted.task_id != task.task_id
                    || persisted.class_id != class.class_id
                    || persisted.annotation_type != task.annotation_type
                {
                    return None;
                }
                let number = objects
                    .iter()
                    .position(|object| object.annotation_id == persisted.annotation_id)?
                    + 1;
                (
                    ReviewTarget::AnnotationVersion {
                        annotation_id: persisted.annotation_id.clone(),
                        version: persisted.version,
                    },
                    ReviewContextPhase::Object {
                        number,
                        total: objects.len(),
                        kind: "Annotation review",
                        annotation_version: Some(persisted.version),
                        disposition_version: None,
                    },
                    Some(persisted.annotation_id.clone()),
                )
            } else if self.work.review_index == objects.len() {
                (
                    ReviewTarget::Task {
                        task_id: task.task_id.clone(),
                    },
                    ReviewContextPhase::FullImage { migration: false },
                    None,
                )
            } else {
                return None;
            }
        };
        if let Some(annotation_id) = &annotation_id {
            let persisted = state.current_annotation(annotation_id)?;
            if persisted.deleted
                || persisted.task_id != task.task_id
                || persisted.class_id != class.class_id
                || persisted.annotation_type != task.annotation_type
            {
                return None;
            }
        }
        let correction = if let Some(draft) = &self.work.correction_draft {
            let persisted = state.current_annotation(annotation_id.as_ref()?)?;
            if draft.annotation_id != persisted.annotation_id
                || draft.expected_version != persisted.version
            {
                return None;
            }
            Some(CorrectionContext {
                base_version: persisted.version,
                unsaved_input: draft.geometry_changed() || !draft.reason.trim().is_empty(),
            })
        } else {
            None
        };
        let decision = state
            .effective_review_for_target(&task.task_id, &target, &self.config.user_id)
            .map(|review| review.decision.clone());
        Some(ReviewContext {
            workflow_name: task.name.clone(),
            class_name: class.name.clone(),
            annotation_type: task.annotation_type.clone(),
            phase,
            decision,
            revision_mode: self.review_revision_active(),
            staged_decision: self
                .staged_review_decision(&target)
                .map(|review| review.decision.clone()),
            correction,
            preview_unavailable: self.work.current_texture.is_none(),
        })
    }

    fn migration_review_context_target(
        &self,
        canonical_targets: &[ReviewTarget],
    ) -> Option<(ReviewTarget, ReviewContextPhase, Option<AnnotationId>)> {
        let state = self.work.current_state.as_ref()?;
        let (task_id, target) = self.current_migration_review_target()?;
        let total = canonical_targets.len();
        let number = self.work.migration.review_index.checked_add(1)?;
        match target {
            MigrationReviewTarget::Disposition {
                object_group_id,
                disposition_version,
            } => {
                let disposition = state
                    .migration_dispositions
                    .get(&task_id)?
                    .get(&object_group_id)?;
                if disposition.disposition_version != disposition_version {
                    return None;
                }
                let (target, kind, version, annotation_id) = match &disposition.status {
                    MigrationDispositionStatus::Pending => return None,
                    MigrationDispositionStatus::Annotated {
                        skeleton_annotation_id,
                        skeleton_version,
                    } => {
                        let annotation = state.current_annotation(skeleton_annotation_id)?;
                        if annotation.deleted
                            || annotation.version != *skeleton_version
                            || annotation.task_id != task_id
                        {
                            return None;
                        }
                        (
                            ReviewTarget::AnnotationVersion {
                                annotation_id: skeleton_annotation_id.clone(),
                                version: *skeleton_version,
                            },
                            "Migration: Annotated skeleton",
                            Some(*skeleton_version),
                            Some(skeleton_annotation_id.clone()),
                        )
                    }
                    MigrationDispositionStatus::Excluded { .. } => (
                        ReviewTarget::MigrationDisposition {
                            task_id,
                            object_group_id,
                            disposition_version,
                        },
                        "Migration: Excluded object",
                        None,
                        None,
                    ),
                };
                if canonical_targets.get(number - 1) != Some(&target) {
                    return None;
                }
                Some((
                    target,
                    ReviewContextPhase::Object {
                        number,
                        total,
                        kind,
                        annotation_version: version,
                        disposition_version: Some(disposition_version),
                    },
                    annotation_id,
                ))
            }
            MigrationReviewTarget::Discovered {
                annotation_id,
                version,
            } => {
                let exact = ReviewTarget::AnnotationVersion {
                    annotation_id: annotation_id.clone(),
                    version,
                };
                if canonical_targets.get(number - 1) != Some(&exact) {
                    return None;
                }
                Some((
                    ReviewTarget::AnnotationVersion {
                        annotation_id: annotation_id.clone(),
                        version,
                    },
                    ReviewContextPhase::Object {
                        number,
                        total,
                        kind: "Migration: Discovered skeleton",
                        annotation_version: Some(version),
                        disposition_version: None,
                    },
                    Some(annotation_id),
                ))
            }
            MigrationReviewTarget::Confirmation { confirmation_hash } => {
                if self.work.migration.review_index != total {
                    return None;
                }
                let confirmation = state.migration_confirmations.get(&task_id)?;
                if confirmation.confirmation_hash != confirmation_hash {
                    return None;
                }
                Some((
                    ReviewTarget::MigrationConfirmation {
                        task_id,
                        confirmation_hash,
                    },
                    ReviewContextPhase::FullImage { migration: true },
                    None,
                ))
            }
        }
    }
}
