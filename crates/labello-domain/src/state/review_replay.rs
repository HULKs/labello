use super::*;

impl ImageState {
    pub(super) fn apply_review_record(&mut self, review: &ReviewRecord) {
        if let Some(round) = self
            .review_target_task(&review.target)
            .and_then(|task| self.review_round(task))
            .cloned()
        {
            self.review_record_rounds
                .insert(review.review_id.clone(), round.event_id);
        }
        self.reviews.push(review.clone());
    }

    pub(super) fn apply_review_assignment_opened(
        &mut self,
        context: &crate::ReviewAssignmentContext,
        assignment: &Assignment,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        if assignment.kind != AssignmentKind::Review
            || event.actor_role != crate::DatasetRole::Reviewer
            || assignment.status != AssignmentStatus::Active
            || assignment.image_id != self.image_id
            || assignment.assigned_to != event.actor_user_id
            || context.assignment_id != assignment.assignment_id
            || context.task.task_id != assignment.task_id
            || self.review_round(&assignment.task_id) != Some(&context.round)
            || self
                .assignments
                .iter()
                .any(|old| old.assignment_id == assignment.assignment_id)
            || context.target_fingerprint != self.review_target_fingerprint(&context.task)
            || context.targets != self.review_targets(&context.task)?
        {
            return Err(DomainError::InvalidReviewRevision(
                "assignment context does not match current work".into(),
            ));
        }
        let expected_superseded = if context.decision_revision {
            self.effective_reviews_for_task(&assignment.task_id)
                .filter(|review| review.reviewer_user_id == assignment.assigned_to)
                .map(|review| review.review_id.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if context.superseded_review_ids != expected_superseded {
            return Err(DomainError::InvalidReviewRevision(
                "captured supersession set is incomplete".into(),
            ));
        }
        if let Some(source_id) = &context.source_assignment_id {
            let source = self
                .assignments
                .iter()
                .find(|old| &old.assignment_id == source_id)
                .ok_or_else(|| {
                    DomainError::InvalidReviewRevision("source assignment is missing".into())
                })?;
            if source.task_id != assignment.task_id
                || source.assigned_to != assignment.assigned_to
                || source.kind != AssignmentKind::Review
                || !matches!(
                    source.status,
                    AssignmentStatus::Cancelled | AssignmentStatus::Completed
                )
                || context.decision_revision
                    != (source.status == AssignmentStatus::Completed
                        || self
                            .review_assignment_contexts
                            .get(source_id)
                            .is_some_and(|context| context.decision_revision))
            {
                return Err(DomainError::InvalidReviewRevision(
                    "source assignment is incompatible".into(),
                ));
            }
        } else if context.decision_revision || !context.superseded_review_ids.is_empty() {
            return Err(DomainError::InvalidReviewRevision(
                "revision source is missing".into(),
            ));
        }
        self.review_assignment_contexts
            .insert(assignment.assignment_id.clone(), context.clone());
        self.assignments.push(assignment.clone());
        Ok(())
    }

    pub(super) fn apply_review_revision(
        &mut self,
        assignment: &Assignment,
        superseded: &[crate::ReviewId],
        replacement: &crate::ReviewRevisionCommit,
        task_state: &TaskState,
        event: &EventLogEntry,
    ) -> DomainResult<()> {
        let invalid = || {
            DomainError::InvalidReviewRevision(
                "replacement does not match the active revision".into(),
            )
        };
        let context = self
            .review_assignment_contexts
            .get(&assignment.assignment_id)
            .ok_or_else(invalid)?;
        let old = self
            .assignments
            .iter()
            .find(|old| old.assignment_id == assignment.assignment_id)
            .ok_or_else(invalid)?;
        if !context.decision_revision
            || old.status != AssignmentStatus::Active
            || event.actor_role != crate::DatasetRole::Reviewer
            || assignment.status != AssignmentStatus::Completed
            || assignment.kind != AssignmentKind::Review
            || assignment.assigned_to != event.actor_user_id
            || assignment.assigned_to != old.assigned_to
            || assignment.task_id != old.task_id
            || assignment.image_id != old.image_id
            || task_state.task_id != assignment.task_id
            || context.superseded_review_ids != superseded
            || self.review_round(&assignment.task_id) != Some(&context.round)
            || context.target_fingerprint != self.review_target_fingerprint(&context.task)
            || replacement.reviews.is_empty()
            || replacement.reviews.len() > 10_001
            || self
                .review_revision_commits
                .contains_key(&assignment.assignment_id)
        {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        for (index, review) in replacement.reviews.iter().enumerate() {
            if review.reviewer_user_id != assignment.assigned_to
                || !context.targets.contains(&review.target)
                || !seen.insert(review.review_id.clone())
                || self
                    .reviews
                    .iter()
                    .any(|old| old.review_id == review.review_id)
                || replacement.reviews[..index]
                    .iter()
                    .any(|old| old.target == review.target)
            {
                return Err(invalid());
            }
        }
        let final_review = replacement.reviews.last().expect("nonempty above");
        if !matches!(
            final_review.target,
            ReviewTarget::Task { .. } | ReviewTarget::MigrationConfirmation { .. }
        ) || (final_review.decision == ReviewDecision::Approved
            && (replacement
                .reviews
                .iter()
                .any(|review| review.decision == ReviewDecision::Rejected)
                || context.targets.iter().any(|target| {
                    !replacement
                        .reviews
                        .iter()
                        .any(|review| &review.target == target)
                })))
        {
            return Err(invalid());
        }
        for id in superseded {
            if self.superseded_review_ids.contains(id)
                || !self
                    .effective_reviews_for_task(&assignment.task_id)
                    .any(|review| {
                        &review.review_id == id && review.reviewer_user_id == assignment.assigned_to
                    })
            {
                return Err(invalid());
            }
        }
        // Mutate a clone so every public replay boundary preserves the old
        // outcome if any part of the replacement is invalid.
        let mut next = self.clone();
        next.superseded_review_ids
            .extend(superseded.iter().cloned());
        for review in &replacement.reviews {
            next.apply_review_record(review);
        }
        let expected_outcome = next.effective_review_outcome(&context.task);
        if (task_state.status.clone(), task_state.outcome.clone()) != expected_outcome {
            return Err(invalid());
        }
        for review in &replacement.reviews {
            if review.decision == ReviewDecision::Rejected
                && let Some((group_id, marker)) = next.migration_review_correction_marker(
                    &assignment.task_id,
                    &review.target,
                    &event.event_id,
                    event.timestamp,
                )?
            {
                next.apply_dependency_marker(&assignment.task_id, &group_id, &marker)?;
            }
        }
        next.apply_task_state(task_state)?;
        *next
            .assignments
            .iter_mut()
            .find(|old| old.assignment_id == assignment.assignment_id)
            .expect("validated above") = assignment.clone();
        next.review_revision_commits
            .insert(assignment.assignment_id.clone(), replacement.clone());
        *self = next;
        Ok(())
    }

    pub(super) fn capture_review_round(&mut self, task_id: &TaskId, event: &EventLogEntry) {
        self.review_rounds.insert(
            task_id.clone(),
            crate::ReviewRound {
                event_id: event.event_id.clone(),
                event_sequence: event.event_sequence,
                submitted_by: event.actor_user_id.clone(),
            },
        );
    }
}
