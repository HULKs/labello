use labello_domain::{Assignment, ReviewDecision, ReviewId, ReviewRecord, ReviewTarget};

use crate::app::{LabelloApp, ReviewPhase, UiCommand};

impl LabelloApp {
    pub(crate) fn review_revision_active(&self) -> bool {
        self.work
            .assignment
            .as_ref()
            .and_then(|assignment| {
                self.work
                    .current_state
                    .as_ref()?
                    .review_assignment_contexts
                    .get(&assignment.assignment_id)
            })
            .is_some_and(|context| context.decision_revision)
    }

    pub(crate) fn staged_review_decision(&self, target: &ReviewTarget) -> Option<&ReviewRecord> {
        self.review_revision_active()
            .then(|| {
                self.work
                    .staged_review_decisions
                    .iter()
                    .rev()
                    .find(|review| review.target == *target)
            })
            .flatten()
    }

    pub(crate) fn stage_revision_review(
        &mut self,
        assignment: Assignment,
        target: ReviewTarget,
        decision: ReviewDecision,
        phase: ReviewPhase,
    ) -> bool {
        if let Some(commit) = &self.work.review_revision_commit {
            if commit
                .reviews
                .last()
                .is_none_or(|review| review.decision != decision)
            {
                self.runtime.error =
                    Some("Retry the pending revised decision before changing it.".into());
                return false;
            }
        } else {
            let review = ReviewRecord {
                review_id: ReviewId::generate(),
                target: target.clone(),
                reviewer_user_id: self.config.user_id.clone(),
                decision: decision.clone(),
                timestamp: labello_domain::now(),
                comment: None,
            };
            self.work
                .staged_review_decisions
                .retain(|old| old.target != target);
            self.work.staged_review_decisions.push(review);
            if phase == ReviewPhase::Object && decision == ReviewDecision::Approved {
                self.work.review_index += 1;
                self.work.migration.review_index += 1;
                self.sync_review_selection();
                return true;
            }
            if phase == ReviewPhase::Object {
                self.work.review_rejected = true;
                let count = self
                    .work
                    .current_state
                    .as_ref()
                    .and_then(|state| {
                        state
                            .review_assignment_contexts
                            .get(&assignment.assignment_id)
                    })
                    .map_or(0, |context| context.targets.len().saturating_sub(1));
                self.work.review_index = count;
                self.work.migration.review_index = count;
                self.sync_review_selection();
                return true;
            }
            if decision == ReviewDecision::Approved
                && self
                    .work
                    .staged_review_decisions
                    .iter()
                    .any(|review| review.decision == ReviewDecision::Rejected)
            {
                self.runtime.error =
                    Some("A staged object rejection requires sending this review back.".into());
                self.work.staged_review_decisions.pop();
                return false;
            }
            self.work.review_revision_commit = Some(labello_domain::ReviewRevisionCommit {
                reviews: self.work.staged_review_decisions.clone(),
            });
        }
        let replacement = self
            .work
            .review_revision_commit
            .clone()
            .expect("staged above");
        let review = replacement
            .reviews
            .last()
            .expect("final decision exists")
            .clone();
        let operation_id = self.begin_operation();
        let request = self.operation_identity(operation_id, self.config.dataset_id.clone());
        self.queue_command(UiCommand::Review {
            request,
            operation_id,
            dataset_id: self.config.dataset_id.clone(),
            assignment,
            review,
            revision: Some(replacement),
            phase: ReviewPhase::FullImage,
        })
    }
}
