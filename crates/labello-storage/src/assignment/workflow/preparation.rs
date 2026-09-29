use super::*;
use labello_domain::{PrelabelSuggestion, WorkflowPreparationStatus};

impl DatasetRepository {
    pub async fn prepare_workflows_without_inference(&self) -> StorageResult<()> {
        let metadata = self.load_dataset().await?;
        for task in metadata.tasks.iter().filter(|t| t.enabled) {
            let config_digest = labello_domain::workflow_prelabel_digest(&metadata, task);
            for image in metadata.images.keys() {
                let state = self.load_image_state(image).await?;
                if state
                    .workflow_preparations
                    .get(&task.task_id)
                    .is_some_and(|p| p.config_digest == config_digest)
                {
                    continue;
                }
                self.prepare_workflow_predictions(
                    image,
                    &task.task_id,
                    config_digest.clone(),
                    None,
                    if config_digest.is_some() {
                        WorkflowPreparationStatus::Pending
                    } else {
                        WorkflowPreparationStatus::Ready
                    },
                    vec![],
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Publication shares queue admission with display, so a claim alone never
    /// freezes predictions and a display can never race replacement.
    pub(crate) async fn prepare_workflow_predictions(
        &self,
        image: &ImageId,
        task_id: &TaskId,
        config_digest: Option<String>,
        generation_key: Option<String>,
        mut status: WorkflowPreparationStatus,
        mut suggestions: Vec<PrelabelSuggestion>,
    ) -> StorageResult<bool> {
        self.ensure_artifact_migration().await?;
        let _config = self.review_config_lock.read().await;
        let _admission = self.assignment_claim_lock.lock().await;
        let metadata = self.load_dataset_config().await?;
        let task = metadata
            .task(task_id)
            .ok_or_else(|| StorageError::AssignmentConflict("workflow no longer exists".into()))?;
        if config_digest != labello_domain::workflow_prelabel_digest(&metadata, task) {
            return Err(StorageError::AssignmentConflict(
                "workflow model changed during preparation".into(),
            ));
        }
        let lock = self.image_lock(image);
        let _image = lock.lock().await;
        let state = self.load_image_state(image).await?;
        if !task.enabled
            || !state.assignment_eligible(task_id)
            || state.workflow_item_seen(task_id, &WorkflowItem::Overview)
        {
            return Ok(false);
        }
        // Legacy work already presented without item contexts is protected too.
        if state.assignments.iter().any(|a| {
            a.task_id == *task_id && !state.workflow_assignments.contains_key(&a.assignment_id)
        }) {
            status = WorkflowPreparationStatus::Ready;
            suggestions.clear();
        }
        let previous = state.workflow_preparations.get(task_id);
        let mut preparation = previous
            .cloned()
            .unwrap_or_else(|| initial_preparation(&state, task));
        let protected = preparation
            .prelabels
            .iter()
            .filter(|p| {
                state.workflow_item_seen(
                    task_id,
                    &WorkflowItem::Object {
                        object: WorkflowObject::Prelabel {
                            suggestion_id: p.suggestion_id.clone(),
                        },
                    },
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        preparation
            .objects
            .retain(|object| !matches!(object, WorkflowObject::Prelabel { .. }));
        preparation.prelabels = protected;
        for suggestion in suggestions {
            if preparation.prelabels.iter().any(|old| {
                old.suggestion_id == suggestion.suggestion_id
                    || overlapping(&old.geometry, &suggestion.geometry)
            }) {
                continue;
            }
            // Existing objects, including accepted predictions and migration guides,
            // should not become duplicate prediction work after a model change.
            if state
                .visible_annotations()
                .any(|a| a.task_id == *task_id && overlapping(&a.geometry, &suggestion.geometry))
            {
                continue;
            }
            preparation.prelabels.push(suggestion);
        }
        preparation
            .objects
            .extend(
                preparation
                    .prelabels
                    .iter()
                    .map(|p| WorkflowObject::Prelabel {
                        suggestion_id: p.suggestion_id.clone(),
                    }),
            );
        preparation.config_digest = config_digest;
        preparation.generation_key = generation_key;
        preparation.status = status;
        if previous == Some(&preparation) {
            return Ok(true);
        }
        let now = labello_domain::now();
        let mut payloads = state
            .assignments
            .iter()
            .filter(|a| {
                a.task_id == *task_id
                    && a.status == AssignmentStatus::Active
                    && state.workflow_assignments.contains_key(&a.assignment_id)
                    && !state.workflow_seen.contains_key(&a.assignment_id)
            })
            .map(|a| {
                let mut cancelled = a.clone();
                cancelled.status = AssignmentStatus::Cancelled;
                cancelled.updated_at = now;
                EventPayload::AssignmentUpdated {
                    assignment: cancelled,
                }
            })
            .collect::<Vec<_>>();
        payloads.push(workflow_payload(WorkflowEvent::Prepared {
            task_id: task_id.clone(),
            preparation,
        }));
        self.append_payloads_unlocked(
            image,
            &Actor {
                user_id: UserId::from("prelabel_service"),
                role: DatasetRole::DataAdmin,
            },
            payloads,
        )
        .await?;
        Ok(true)
    }
}

fn overlapping(first: &AnnotationGeometry, second: &AnnotationGeometry) -> bool {
    fn bounds(geometry: &AnnotationGeometry) -> Option<labello_domain::BoundingBox> {
        match geometry {
            AnnotationGeometry::BoundingBox(bbox) => Some(*bbox),
            AnnotationGeometry::Skeleton(skeleton) => {
                let points = skeleton
                    .keypoints
                    .iter()
                    .filter_map(|k| k.point)
                    .collect::<Vec<_>>();
                let first = points.first()?;
                let (mut x, mut y, mut right, mut bottom) = (first.x, first.y, first.x, first.y);
                for point in points {
                    x = x.min(point.x);
                    y = y.min(point.y);
                    right = right.max(point.x);
                    bottom = bottom.max(point.y);
                }
                // Single-keypoint objects still have a small spatial footprint.
                Some(labello_domain::BoundingBox {
                    x,
                    y,
                    width: (right - x).max(0.01),
                    height: (bottom - y).max(0.01),
                })
            }
        }
    }
    let (Some(a), Some(b)) = (bounds(first), bounds(second)) else {
        return false;
    };
    let intersection = ((a.x + a.width).min(b.x + b.width) - a.x.max(b.x)).max(0.0)
        * ((a.y + a.height).min(b.y + b.height) - a.y.max(b.y)).max(0.0);
    intersection > 0.5 * (a.width * a.height + b.width * b.height - intersection)
}
