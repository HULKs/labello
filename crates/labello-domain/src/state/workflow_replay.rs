use crate::*;

fn invalid(reason: &str) -> DomainError {
    DomainError::InvalidWorkflow(reason.into())
}

impl ImageState {
    pub(super) fn apply_workflow_correction(
        &mut self,
        assignment: &Assignment,
        submission: &ReviewCorrectionSubmission,
        review: &ReviewRecord,
        task_state: &TaskState,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        submission.validate()?;
        let (old, context) =
            self.validate_workflow_actor(&assignment.task_id, &assignment.assignment_id, event)?;
        if old.kind != AssignmentKind::Review
            || assignment.assigned_to != old.assigned_to
            || assignment.image_id != old.image_id
            || assignment.kind != old.kind
            || assignment.status != AssignmentStatus::Completed
            || assignment.created_at != old.created_at
            || assignment.expires_at != old.expires_at
            || assignment.updated_at != event.timestamp
            || !self.workflow_seen.contains_key(&assignment.assignment_id)
            || self
                .review_correction_submissions
                .contains_key(&assignment.assignment_id)
            || self
                .review_correction_submissions
                .values()
                .any(|s| s.correction_id == submission.correction_id)
            || self.review_round(&assignment.task_id) != Some(&submission.round)
            || review.decision != ReviewDecision::Rejected
            || review.reviewer_user_id != event.actor_user_id
            || context.review_target.as_ref() != Some(&review.target)
            || review.timestamp != event.timestamp
            || review.comment != submission.reason
            || self.reviews.iter().any(|r| r.review_id == review.review_id)
            || task_state.task_id != assignment.task_id
            || task_state.status != TaskStatus::Submitted
            || task_state.outcome.is_some()
            || task_state.assigned_to.is_some()
            || task_state.completed_by.is_some()
            || task_state.completed_at.is_some()
            || task_state.updated_at != event.timestamp
            || submission
                .changes
                .iter()
                .any(|change| !self.workflow_correction_in_scope(&context.item, change))
            || !submission.matches_applied_changes(
                self,
                &assignment.task_id,
                &event.actor_user_id,
                event.timestamp,
            )
        {
            return Err(invalid("invalid queue correction boundary"));
        }
        let receipt = WorkflowConfirmation {
            assignment_id: assignment.assignment_id.clone(),
            task_id: assignment.task_id.clone(),
            annotation: None,
            reviewed_targets: self
                .workflow_corrected_targets(&assignment.task_id, &submission.changes),
            review: Some(review.clone()),
        };
        let mut next = self.clone();
        next.apply_review_record(review);
        next.apply_task_state(task_state)?;
        *next
            .assignments
            .iter_mut()
            .find(|a| a.assignment_id == assignment.assignment_id)
            .expect("validated assignment") = assignment.clone();
        next.review_correction_submissions
            .insert(assignment.assignment_id.clone(), submission.clone());
        next.workflow_confirmations
            .insert(assignment.assignment_id.clone(), receipt);
        next.workflow_confirmation_sequences
            .insert(assignment.assignment_id.clone(), event.event_sequence);
        *self = next;
        Ok(())
    }

    pub(crate) fn observe_workflow_contribution(&mut self, event: &EventLogEntry) {
        match &event.payload {
            EventPayload::ReviewRecorded { review }
            | EventPayload::ReviewCorrectionSubmitted { review, .. }
            | EventPayload::ReviewerCorrectionRecorded { review, .. } => {
                self.workflow_review_sequences
                    .insert(review.review_id.clone(), event.event_sequence);
            }
            EventPayload::ReviewRevisionCommitted { replacement, .. } => {
                for review in &replacement.reviews {
                    self.workflow_review_sequences
                        .insert(review.review_id.clone(), event.event_sequence);
                }
            }
            _ => {}
        }
        let contributed = match &event.payload {
            EventPayload::AnnotationVersionCreated { annotation, .. } => !matches!(
                annotation.revision_source,
                RevisionSource::Import { .. } | RevisionSource::MigrationSkeleton { .. }
            ),
            EventPayload::MigrationDispositionChanged {
                task_id,
                object_group_id,
                ..
            }
            | EventPayload::MigrationDispositionReopened {
                task_id,
                object_group_id,
                ..
            } => {
                self.migration_authors
                    .entry(task_id.clone())
                    .or_default()
                    .insert(object_group_id.clone(), event.actor_user_id.clone());
                true
            }
            EventPayload::Workflow { event } => {
                matches!(
                    event.as_ref(),
                    WorkflowEvent::ItemConfirmed { .. }
                        | WorkflowEvent::DraftSaved { .. }
                        | WorkflowEvent::EditsSaved { .. }
                )
            }
            EventPayload::TaskStateChanged { task_state } => task_state.completed_by.is_some(),
            EventPayload::AnnotationDeleted { .. }
            | EventPayload::ReviewRecorded { .. }
            | EventPayload::ReviewRevisionCommitted { .. }
            | EventPayload::ReviewCorrectionSubmitted { .. }
            | EventPayload::ReviewerCorrectionRecorded { .. }
            | EventPayload::MigrationFullImageConfirmed { .. }
            | EventPayload::MigrationPassItemRecorded { .. } => true,
            _ => false,
        };
        if contributed {
            self.workflow_contributors
                .insert(event.actor_user_id.clone());
        }
    }

    pub(crate) fn apply_workflow_event(
        &mut self,
        workflow: &WorkflowEvent,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        workflow
            .task_id()
            .validate_path_segment()
            .map_err(|_| invalid("invalid workflow ID"))?;
        match workflow {
            WorkflowEvent::EditsSaved {
                task_id,
                assignment_id,
                edits,
                expected_sequence,
            } => {
                let (assignment, context) =
                    self.validate_workflow_actor(task_id, assignment_id, event)?;
                if !self.workflow_seen.contains_key(assignment_id)
                    || (assignment.kind == AssignmentKind::Annotation
                        && context.item != WorkflowItem::Overview)
                    || self
                        .workflow_edit_draft(task_id, &context.item, &assignment.kind)
                        .map_or(0, |d| d.sequence)
                        != *expected_sequence
                {
                    return Err(invalid("partial edits are stale or not displayed work"));
                }
                self.validate_workflow_edits(task_id, &context.item, edits)?;
                let draft = WorkflowEditDraft {
                    task_id: task_id.clone(),
                    item: context.item.clone(),
                    kind: assignment.kind.clone(),
                    edits: edits.clone(),
                    sequence: event.event_sequence,
                    previous_sequence: *expected_sequence,
                };
                self.workflow_edit_drafts
                    .insert(assignment_id.clone(), draft);
            }
            WorkflowEvent::DraftSaved {
                task_id,
                assignment_id,
                geometry,
                expected_sequence,
            } => {
                let (assignment, context) =
                    self.validate_workflow_actor(task_id, assignment_id, event)?;
                if assignment.kind != AssignmentKind::Annotation
                    || context.item == WorkflowItem::Overview
                    || !self.workflow_seen.contains_key(assignment_id)
                    || self
                        .workflow_object_draft(task_id, &context.item)
                        .map_or(0, |draft| draft.sequence)
                        != *expected_sequence
                {
                    return Err(invalid(
                        "partial object draft is stale or not displayed annotation work",
                    ));
                }
                geometry.validate()?;
                let draft = crate::WorkflowDraft {
                    task_id: task_id.clone(),
                    item: context.item.clone(),
                    geometry: geometry.clone(),
                    sequence: event.event_sequence,
                    previous_sequence: *expected_sequence,
                };
                self.workflow_drafts.insert(assignment_id.clone(), draft);
            }
            WorkflowEvent::Prepared {
                task_id,
                preparation,
            } => {
                self.validate_workflow_preparation(task_id, preparation)?;
                self.workflow_preparations
                    .insert(task_id.clone(), preparation.clone());
            }
            WorkflowEvent::AssignmentOpened {
                assignment,
                context,
            } => {
                if assignment.image_id != self.image_id
                    || assignment.assigned_to != event.actor_user_id
                    || assignment.status != AssignmentStatus::Active
                    || assignment
                        .expires_at
                        .is_none_or(|expires| expires <= event.timestamp)
                    || self
                        .assignments
                        .iter()
                        .any(|a| a.assignment_id == assignment.assignment_id)
                    || assignment.kind == AssignmentKind::LegacyAdjudication
                    || (assignment.kind == AssignmentKind::Annotation
                        && (context.review_exception || context.review_target.is_some()))
                    || (assignment.kind == AssignmentKind::Review
                        && context.review_target.is_none())
                {
                    return Err(invalid("invalid item assignment"));
                }
                let role = if assignment.kind == AssignmentKind::Annotation {
                    DatasetRole::Annotator
                } else {
                    DatasetRole::Reviewer
                };
                if event.actor_role != role {
                    return Err(invalid("assignment role does not match its work"));
                }
                if let Some(source_id) = &context.source_assignment_id {
                    let source = self
                        .assignments
                        .iter()
                        .find(|a| a.assignment_id == *source_id);
                    let source_context = self.workflow_assignments.get(source_id);
                    if source.is_none_or(|a| {
                        a.assigned_to != assignment.assigned_to
                            || a.task_id != assignment.task_id
                            || a.kind != assignment.kind
                    }) || source_context
                        .is_none_or(|c| c.item != context.item || c.source_assignment_id.is_some())
                        || !self.workflow_seen.contains_key(source_id)
                    {
                        return Err(invalid(
                            "history source is not this user's displayed queue item",
                        ));
                    }
                }
                if assignment.kind == AssignmentKind::Annotation
                    && let WorkflowItem::Object { object } = &context.item
                    && !self
                        .workflow_preparations
                        .get(&assignment.task_id)
                        .is_some_and(|prepared| prepared.objects.contains(object))
                {
                    return Err(invalid("object is not in the prepared workflow"));
                }
                if let Some(target) = &context.review_target
                    && (self.review_target_task(target) != Some(&assignment.task_id)
                        || !context.item.matches_review_target(target)
                        || (!context.review_exception
                            && !self.workflow_independent_reviewer(
                                &context.item,
                                target,
                                &event.actor_user_id,
                            )))
                {
                    return Err(invalid(
                        "review target or independence does not match the item",
                    ));
                }
                if self.assignments.iter().any(|other| {
                    other.task_id == assignment.task_id
                        && other.status == AssignmentStatus::Active
                        && other
                            .expires_at
                            .is_some_and(|expiry| expiry > event.timestamp)
                        && self
                            .workflow_assignments
                            .get(&other.assignment_id)
                            .is_none_or(|existing| {
                                other.kind != assignment.kind
                                    || existing.item == context.item
                                    || existing.item == WorkflowItem::Overview
                                    || context.item == WorkflowItem::Overview
                            })
                }) {
                    return Err(invalid("item has a conflicting live assignment"));
                }
                self.workflow_assignments
                    .insert(assignment.assignment_id.clone(), context.clone());
                self.assignments.push(assignment.clone());
            }
            WorkflowEvent::ItemSeen {
                task_id,
                assignment_id,
            } => {
                let (assignment, context) =
                    self.validate_workflow_actor(task_id, assignment_id, event)?;
                if assignment.kind == AssignmentKind::Annotation
                    && let WorkflowItem::Object { object } = &context.item
                    && !self
                        .workflow_preparations
                        .get(task_id)
                        .is_some_and(|prepared| prepared.objects.contains(object))
                {
                    return Err(invalid("prepared object changed before display"));
                }
                self.workflow_seen
                    .entry(assignment_id.clone())
                    .or_insert_with(|| WorkflowSeen {
                        sequence: event.event_sequence,
                        timestamp: event.timestamp,
                        event_id: event.event_id.clone(),
                    });
            }
            WorkflowEvent::ItemConfirmed { confirmation } => {
                self.validate_workflow_confirmation(confirmation, event)?;
                self.workflow_confirmations
                    .insert(confirmation.assignment_id.clone(), confirmation.clone());
                self.workflow_confirmation_sequences
                    .insert(confirmation.assignment_id.clone(), event.event_sequence);
                let assignment = self
                    .assignments
                    .iter_mut()
                    .find(|a| a.assignment_id == confirmation.assignment_id)
                    .expect("validated assignment");
                assignment.status = AssignmentStatus::Completed;
                assignment.updated_at = event.timestamp;
            }
        }
        Ok(())
    }

    fn validate_workflow_actor(
        &self,
        task_id: &TaskId,
        assignment_id: &AssignmentId,
        event: &EventLogEntry,
    ) -> DomainResult<(&Assignment, &WorkflowAssignmentContext)> {
        let assignment = self
            .assignments
            .iter()
            .find(|a| a.assignment_id == *assignment_id)
            .ok_or_else(|| invalid("item assignment is missing"))?;
        let context = self
            .workflow_assignments
            .get(assignment_id)
            .ok_or_else(|| invalid("item assignment context is missing"))?;
        if assignment.task_id != *task_id
            || assignment.assigned_to != event.actor_user_id
            || assignment.status != AssignmentStatus::Active
            || assignment
                .expires_at
                .is_none_or(|expires| expires <= event.timestamp)
            || event.actor_role
                != if assignment.kind == AssignmentKind::Annotation {
                    DatasetRole::Annotator
                } else {
                    DatasetRole::Reviewer
                }
        {
            return Err(invalid("item assignment is not owned and active"));
        }
        Ok((assignment, context))
    }

    fn validate_workflow_confirmation(
        &self,
        confirmation: &WorkflowConfirmation,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        let (assignment, context) = self.validate_workflow_actor(
            &confirmation.task_id,
            &confirmation.assignment_id,
            event,
        )?;
        if !self.workflow_seen.contains_key(&confirmation.assignment_id)
            || self
                .workflow_confirmations
                .contains_key(&confirmation.assignment_id)
        {
            return Err(invalid("confirmation requires a displayed unfinished item"));
        }
        if let Some(confirmed) = &confirmation.annotation {
            let annotation = self
                .current_annotation(&confirmed.annotation_id)
                .ok_or_else(|| invalid("confirmed annotation is missing"))?;
            if annotation.task_id != confirmation.task_id
                || annotation.version != confirmed.version
                || annotation.deleted
                || !self.workflow_object_matches_annotation(&context.item, annotation)
            {
                return Err(invalid(
                    "confirmation does not match current object geometry",
                ));
            }
        }
        match assignment.kind {
            AssignmentKind::Annotation => {
                if confirmation.review.is_some() || !confirmation.reviewed_targets.is_empty() {
                    return Err(invalid("annotation cannot submit a review"));
                }
                if let WorkflowItem::Object { object } = &context.item {
                    match object {
                        WorkflowObject::Annotation { annotation_id } => {
                            if self
                                .current_annotation(annotation_id)
                                .is_none_or(|a| !a.deleted && confirmation.annotation.is_none())
                            {
                                return Err(invalid(
                                    "object needs final geometry or a saved deletion",
                                ));
                            }
                        }
                        WorkflowObject::Migration { object_group_id } => {
                            if !self.workflow_migration_object_complete(
                                &confirmation.task_id,
                                object_group_id,
                            ) {
                                return Err(invalid("migration object is unfinished"));
                            }
                        }
                        WorkflowObject::Prelabel { .. } => {}
                    }
                }
            }
            AssignmentKind::Review => {
                let review = confirmation
                    .review
                    .as_ref()
                    .ok_or_else(|| invalid("review decision is missing"))?;
                if Some(&review.target) != context.review_target.as_ref()
                    || review.reviewer_user_id != event.actor_user_id
                    || self
                        .reviews
                        .iter()
                        .find(|r| r.review_id == review.review_id)
                        != Some(review)
                    || review.timestamp != event.timestamp
                {
                    return Err(invalid(
                        "review does not match its captured target and actor",
                    ));
                }
                if confirmation.reviewed_targets.iter().any(|target| {
                    self.review_target_task(target) != Some(&confirmation.task_id)
                        || !context.item.matches_review_target(target)
                }) {
                    return Err(invalid("corrected review target belongs to different work"));
                }
            }
            AssignmentKind::LegacyAdjudication => {
                return Err(invalid("unsupported item assignment"));
            }
        }
        Ok(())
    }

    fn validate_workflow_preparation(
        &self,
        task_id: &TaskId,
        preparation: &WorkflowPreparation,
    ) -> DomainResult<()> {
        let unique = preparation
            .objects
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        if preparation.objects.len() > MAX_MIGRATION_TARGETS_PER_EVENT
            || unique.len() != preparation.objects.len()
            || preparation.prelabels.len() > preparation.objects.len()
            || !self.assignment_eligible(task_id)
        {
            return Err(invalid("invalid workflow preparation"));
        }
        for object in &preparation.objects {
            match object {
                WorkflowObject::Annotation { annotation_id } => {
                    if self
                        .current_annotation(annotation_id)
                        .is_none_or(|a| a.task_id != *task_id)
                    {
                        return Err(invalid("prepared annotation belongs to another workflow"));
                    }
                }
                WorkflowObject::Migration { object_group_id } => {
                    if !self.migration_target_sets.get(task_id).is_some_and(|set| {
                        set.targets
                            .iter()
                            .any(|t| t.object_group_id == *object_group_id)
                    }) {
                        return Err(invalid("prepared migration target is missing"));
                    }
                }
                WorkflowObject::Prelabel { suggestion_id } => {
                    if suggestion_id.is_empty()
                        || suggestion_id.len() > 256
                        || preparation
                            .prelabels
                            .iter()
                            .filter(|s| s.suggestion_id == *suggestion_id)
                            .count()
                            != 1
                    {
                        return Err(invalid("prepared prediction is missing or duplicated"));
                    }
                }
            }
        }
        for suggestion in &preparation.prelabels {
            if suggestion.task_id != *task_id
                || !unique.contains(&WorkflowObject::Prelabel {
                    suggestion_id: suggestion.suggestion_id.clone(),
                })
                || !suggestion.confidence.is_finite()
                || !(0.0..=1.0).contains(&suggestion.confidence)
                || suggestion.evidence.as_ref().is_none_or(|e| {
                    e.provenance.image_id != self.image_id
                        || e.provenance.task_id != *task_id
                        || e.provenance.suggestion_id != suggestion.suggestion_id
                        || e.predicted_geometry != suggestion.geometry
                })
            {
                return Err(invalid("prepared prediction evidence does not match"));
            }
        }
        if let Some(previous) = self.workflow_preparations.get(task_id) {
            for object in &previous.objects {
                if self.workflow_item_seen(
                    task_id,
                    &WorkflowItem::Object {
                        object: object.clone(),
                    },
                ) {
                    if !preparation.objects.contains(object) {
                        return Err(invalid(
                            "displayed objects cannot be removed by preparation",
                        ));
                    }
                    if let WorkflowObject::Prelabel { suggestion_id } = object
                        && previous
                            .prelabels
                            .iter()
                            .find(|s| s.suggestion_id == *suggestion_id)
                            != preparation
                                .prelabels
                                .iter()
                                .find(|s| s.suggestion_id == *suggestion_id)
                    {
                        return Err(invalid("displayed predictions cannot be replaced"));
                    }
                }
            }
            if self.workflow_item_seen(task_id, &WorkflowItem::Overview) && preparation != previous
            {
                return Err(invalid("preparation cannot interrupt an overview"));
            }
        }
        Ok(())
    }
}
