use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Unfinished edits remain proposals until the item is explicitly confirmed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowEdits {
    pub changes: Vec<ReviewCorrectionChange>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEditDraft {
    pub task_id: TaskId,
    pub item: WorkflowItem,
    pub kind: AssignmentKind,
    pub edits: WorkflowEdits,
    pub sequence: u64,
    pub previous_sequence: u64,
}

impl ImageState {
    pub fn workflow_edit_draft(
        &self,
        task: &TaskId,
        item: &WorkflowItem,
        kind: &AssignmentKind,
    ) -> Option<&WorkflowEditDraft> {
        self.workflow_edit_drafts
            .values()
            .filter(|d| &d.task_id == task && &d.item == item && &d.kind == kind)
            .max_by_key(|d| d.sequence)
    }

    pub fn pending_workflow_edits(
        &self,
        task: &TaskId,
        item: &WorkflowItem,
        kind: &AssignmentKind,
    ) -> Option<&WorkflowEditDraft> {
        self.workflow_edit_draft(task, item, kind).filter(|draft| {
            !self.workflow_confirmations.values().any(|confirmation| {
                &confirmation.task_id == task
                    && self
                        .workflow_confirmation_sequences
                        .get(&confirmation.assignment_id)
                        .is_some_and(|sequence| *sequence > draft.sequence)
                    && self
                        .assignments
                        .iter()
                        .any(|a| a.assignment_id == confirmation.assignment_id && &a.kind == kind)
                    && self
                        .workflow_assignments
                        .get(&confirmation.assignment_id)
                        .is_some_and(|c| &c.item == item)
            })
        })
    }

    pub fn validate_workflow_edits(
        &self,
        task: &TaskId,
        item: &WorkflowItem,
        edits: &WorkflowEdits,
    ) -> DomainResult<()> {
        let invalid = || {
            DomainError::InvalidWorkflow(
                "partial edits are stale or outside the leased item".into(),
            )
        };
        if edits.changes.len() > 10_000
            || edits
                .reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 2_000)
        {
            return Err(invalid());
        }
        let mut identities = std::collections::BTreeSet::new();
        for change in &edits.changes {
            if !self.workflow_correction_in_scope(item, change) {
                return Err(invalid());
            }
            let (identity, geometry) = match change {
                ReviewCorrectionChange::Add {
                    annotation_id,
                    class_id,
                    geometry,
                } => {
                    annotation_id
                        .validate_path_segment()
                        .map_err(|_| invalid())?;
                    class_id.validate_path_segment().map_err(|_| invalid())?;
                    if self.current_annotation(annotation_id).is_some() {
                        return Err(invalid());
                    }
                    (
                        format!("annotation:{annotation_id}"),
                        Some(geometry.clone()),
                    )
                }
                ReviewCorrectionChange::Edit {
                    annotation_id,
                    expected_version,
                    ..
                }
                | ReviewCorrectionChange::Remove {
                    annotation_id,
                    expected_version,
                } => {
                    if !self.current_annotation(annotation_id).is_some_and(|a| {
                        &a.task_id == task && a.version == *expected_version && !a.deleted
                    }) {
                        return Err(invalid());
                    }
                    let geometry = if let ReviewCorrectionChange::Edit { geometry, .. } = change {
                        Some(geometry.clone())
                    } else {
                        None
                    };
                    (format!("annotation:{annotation_id}"), geometry)
                }
                ReviewCorrectionChange::MigrationObject {
                    object_group_id,
                    expected_disposition_version,
                    replacement,
                } => {
                    if !self
                        .migration_dispositions
                        .get(task)
                        .and_then(|items| items.get(object_group_id))
                        .is_some_and(|d| d.disposition_version == *expected_disposition_version)
                    {
                        return Err(invalid());
                    }
                    let geometry = match replacement {
                        MigrationReviewCorrection::Skeleton { skeleton } => {
                            Some(AnnotationGeometry::Skeleton(skeleton.clone()))
                        }
                        MigrationReviewCorrection::Exclude { note, .. } => {
                            if note.as_ref().is_some_and(|note| note.len() > 2_000) {
                                return Err(invalid());
                            }
                            None
                        }
                    };
                    (format!("migration:{object_group_id}"), geometry)
                }
            };
            if !identities.insert(identity) {
                return Err(invalid());
            }
            if let Some(geometry) = geometry {
                geometry.validate()?;
            }
        }
        Ok(())
    }
}

pub fn validate_workflow_partial_geometry(
    task: &TaskDefinition,
    geometry: &AnnotationGeometry,
) -> DomainResult<()> {
    geometry.validate()?;
    let invalid =
        || DomainError::InvalidWorkflow("partial geometry does not match the workflow".into());
    match (geometry, &task.annotation_type) {
        (AnnotationGeometry::BoundingBox(_), AnnotationType::BoundingBox) => Ok(()),
        (AnnotationGeometry::Skeleton(skeleton), AnnotationType::Skeleton) => {
            let spec = task.skeleton.as_ref().ok_or_else(invalid)?;
            let mut names = std::collections::BTreeSet::new();
            if skeleton.keypoints.len() > spec.keypoints.len()
                || skeleton.keypoints.iter().any(|keypoint| {
                    !names.insert(&keypoint.name)
                        || !spec.keypoints.iter().any(|k| k.name == keypoint.name)
                        || (keypoint.state == KeypointState::Hidden && !spec.allow_hidden)
                        || (keypoint.state == KeypointState::Absent && !spec.allow_absent)
                })
            {
                return Err(invalid());
            }
            Ok(())
        }
        _ => Err(invalid()),
    }
}
