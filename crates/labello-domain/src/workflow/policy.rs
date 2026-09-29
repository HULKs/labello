use crate::*;

impl WorkflowItem {
    pub fn from_review_target(target: &ReviewTarget) -> Self {
        match target {
            ReviewTarget::AnnotationVersion { annotation_id, .. } => Self::Object {
                object: WorkflowObject::Annotation {
                    annotation_id: annotation_id.clone(),
                },
            },
            ReviewTarget::MigrationDisposition {
                object_group_id, ..
            } => Self::Object {
                object: WorkflowObject::Migration {
                    object_group_id: object_group_id.clone(),
                },
            },
            _ => Self::Overview,
        }
    }

    pub fn matches_review_target(&self, target: &ReviewTarget) -> bool {
        *self == Self::from_review_target(target)
    }
}

impl ImageState {
    pub fn workflow_correction_in_scope(
        &self,
        item: &WorkflowItem,
        change: &ReviewCorrectionChange,
    ) -> bool {
        match (item, change) {
            (WorkflowItem::Overview, _) => true,
            (
                WorkflowItem::Object {
                    object:
                        WorkflowObject::Annotation {
                            annotation_id: expected,
                        },
                },
                ReviewCorrectionChange::Edit { annotation_id, .. }
                | ReviewCorrectionChange::Remove { annotation_id, .. },
            ) => expected == annotation_id,
            (
                WorkflowItem::Object {
                    object:
                        WorkflowObject::Migration {
                            object_group_id: expected,
                        },
                },
                ReviewCorrectionChange::MigrationObject {
                    object_group_id, ..
                },
            ) => expected == object_group_id,
            _ => false,
        }
    }

    pub fn workflow_corrected_targets(
        &self,
        task: &TaskId,
        changes: &[ReviewCorrectionChange],
    ) -> Vec<ReviewTarget> {
        changes
            .iter()
            .filter_map(|change| match change {
                ReviewCorrectionChange::Edit { annotation_id, .. } => self
                    .current_annotation(annotation_id)
                    .filter(|a| !a.deleted)
                    .map(|a| ReviewTarget::AnnotationVersion {
                        annotation_id: a.annotation_id.clone(),
                        version: a.version,
                    }),
                ReviewCorrectionChange::MigrationObject {
                    object_group_id, ..
                } => self
                    .migration_dispositions
                    .get(task)
                    .and_then(|items| items.get(object_group_id))
                    .map(|d| ReviewTarget::MigrationDisposition {
                        task_id: task.clone(),
                        object_group_id: object_group_id.clone(),
                        disposition_version: d.disposition_version,
                    }),
                ReviewCorrectionChange::Add { .. } | ReviewCorrectionChange::Remove { .. } => None,
            })
            .collect()
    }

    pub fn workflow_object_draft(
        &self,
        task: &TaskId,
        item: &WorkflowItem,
    ) -> Option<&WorkflowDraft> {
        self.workflow_drafts
            .values()
            .filter(|draft| draft.task_id == *task && draft.item == *item)
            .max_by_key(|draft| draft.sequence)
    }

    pub fn workflow_item_seen(&self, task: &TaskId, item: &WorkflowItem) -> bool {
        self.assignments.iter().any(|assignment| {
            assignment.task_id == *task
                && self.workflow_seen.contains_key(&assignment.assignment_id)
                && self
                    .workflow_assignments
                    .get(&assignment.assignment_id)
                    .is_some_and(|c| c.item == *item)
        })
    }

    pub fn workflow_annotation_confirmed(&self, task: &TaskId, object: &WorkflowObject) -> bool {
        let item = WorkflowItem::Object {
            object: object.clone(),
        };
        let latest = self
            .workflow_confirmations
            .values()
            .filter(|confirmation| {
                confirmation.task_id == *task
                    && confirmation.review.is_none()
                    && self
                        .workflow_assignments
                        .get(&confirmation.assignment_id)
                        .is_some_and(|context| context.item == item)
            })
            .max_by_key(|c| self.workflow_confirmation_sequences.get(&c.assignment_id));
        let Some(confirmation) = latest else {
            return false;
        };
        let sequence = self
            .workflow_confirmation_sequences
            .get(&confirmation.assignment_id)
            .copied()
            .unwrap_or_default();
        if self
            .workflow_object_draft(task, &item)
            .is_some_and(|draft| draft.sequence > sequence)
        {
            return false;
        }
        match &confirmation.annotation {
            Some(confirmed) => self
                .current_annotation(&confirmed.annotation_id)
                .is_some_and(|a| a.version == confirmed.version),
            None => !self
                .active_annotations()
                .any(|a| a.task_id == *task && self.workflow_object_matches_annotation(&item, a)),
        }
    }

    pub fn workflow_pending_objects(&self, task: &TaskId) -> Vec<WorkflowObject> {
        self.workflow_preparations
            .get(task)
            .into_iter()
            .flat_map(|p| &p.objects)
            .filter(|object| match object {
                WorkflowObject::Migration { object_group_id } => {
                    (!self.workflow_migration_object_complete(task, object_group_id)
                        || self
                            .workflow_object_draft(
                                task,
                                &WorkflowItem::Object {
                                    object: (*object).clone(),
                                },
                            )
                            .is_some_and(|draft| {
                                self.workflow_confirmations
                                    .values()
                                    .filter(|c| {
                                        c.task_id == *task
                                            && self
                                                .workflow_assignments
                                                .get(&c.assignment_id)
                                                .is_some_and(|context| context.item == draft.item)
                                    })
                                    .filter_map(|c| {
                                        self.workflow_confirmation_sequences.get(&c.assignment_id)
                                    })
                                    .max()
                                    .is_none_or(|sequence| *sequence < draft.sequence)
                            }))
                        && !self
                            .skipped_migration_groups(task)
                            .contains(object_group_id)
                }
                _ => !self.workflow_annotation_confirmed(task, object),
            })
            .cloned()
            .collect()
    }

    pub fn workflow_migration_object_complete(&self, task: &TaskId, group: &ObjectGroupId) -> bool {
        !self
            .migration_dependencies
            .get(task)
            .is_some_and(|markers| markers.contains_key(group))
            && self
                .migration_dispositions
                .get(task)
                .and_then(|items| items.get(group))
                .is_some_and(|d| !matches!(d.status, MigrationDispositionStatus::Pending))
    }

    pub fn workflow_object_matches_annotation(
        &self,
        item: &WorkflowItem,
        annotation: &AnnotationVersion,
    ) -> bool {
        match item {
            WorkflowItem::Overview => true,
            WorkflowItem::Object {
                object: WorkflowObject::Annotation { annotation_id },
            } => annotation.annotation_id == *annotation_id,
            WorkflowItem::Object {
                object: WorkflowObject::Migration { object_group_id },
            } => annotation.object_group_id.as_ref() == Some(object_group_id),
            WorkflowItem::Object {
                object: WorkflowObject::Prelabel { suggestion_id },
            } => {
                matches!(&annotation.origin, AnnotationOrigin::Prelabel { prelabel } if prelabel.provenance.suggestion_id == *suggestion_id)
            }
        }
    }

    pub fn workflow_independent_reviewer(
        &self,
        item: &WorkflowItem,
        target: &ReviewTarget,
        user: &UserId,
    ) -> bool {
        if *item == WorkflowItem::Overview {
            return !self.workflow_contributors.contains(user);
        }
        match target {
            ReviewTarget::AnnotationVersion { annotation_id, .. } => {
                self.current_annotation(annotation_id).is_some_and(|a| {
                    // A partial contributor is eligible; confirmation owns final authorship.
                    self.assignments
                        .iter()
                        .rev()
                        .find(|assignment| {
                            self.workflow_confirmations
                                .get(&assignment.assignment_id)
                                .is_some_and(|c| {
                                    c.review.is_none()
                                        && c.annotation.as_ref().is_some_and(|confirmed| {
                                            confirmed.annotation_id == *annotation_id
                                                && confirmed.version == a.version
                                        })
                                })
                        })
                        .map_or(&a.author_user_id, |assignment| &assignment.assigned_to)
                        != user
                })
            }
            ReviewTarget::MigrationDisposition {
                task_id,
                object_group_id,
                ..
            } => self
                .migration_authors
                .get(task_id)
                .and_then(|items| items.get(object_group_id))
                .is_some_and(|author| author != user),
            _ => false,
        }
    }

    pub fn workflow_pending_reviews(
        &self,
        task: &TaskDefinition,
    ) -> DomainResult<Vec<ReviewTarget>> {
        Ok(self
            .review_object_targets(task)?
            .into_iter()
            .filter(|target| {
                !self.workflow_confirmations.values().any(|confirmation| {
                    confirmation.task_id == task.task_id
                        && self
                            .workflow_confirmation_sequences
                            .get(&confirmation.assignment_id)
                            .copied()
                            .unwrap_or_default()
                            > self
                                .workflow_review_barriers
                                .get(&task.task_id)
                                .copied()
                                .unwrap_or_default()
                        && confirmation.reviewed_targets.contains(target)
                        && confirmation
                            .review
                            .as_ref()
                            .is_some_and(|r| !self.superseded_review_ids.contains(&r.review_id))
                }) && !self.reviews.iter().any(|review| {
                    review.target == *target
                        && review.decision == ReviewDecision::Approved
                        && !self.superseded_review_ids.contains(&review.review_id)
                        && self
                            .workflow_review_sequences
                            .get(&review.review_id)
                            .copied()
                            .unwrap_or_default()
                            > self
                                .workflow_review_barriers
                                .get(&task.task_id)
                                .copied()
                                .unwrap_or_default()
                })
            })
            .collect())
    }
}
