use crate::{ExportOmissionReason, ReviewWorkflow, TaskDefinition};

use super::*;

impl ImageState {
    /// Ground-truth coverage is established for the entire task before selecting rows.
    /// This prevents incomplete work or filtered suggestions becoming empty labels.
    pub fn export_task_omission(
        &self,
        task: &TaskDefinition,
        events: &[EventLogEntry],
    ) -> Option<ExportOmissionReason> {
        use ExportOmissionReason as Omitted;
        if !self.included_in_completion_denominator(&task.task_id) {
            return Some(Omitted::ExcludedCoverage);
        }
        let Some(state) = self
            .task_states
            .get(&task.task_id)
            .filter(|state| state.status == TaskStatus::Completed)
        else {
            return Some(Omitted::Unfinished);
        };
        let Some(completion) = events.iter().rfind(|event| {
            terminal_state(&event.payload, &task.task_id).is_some_and(|value| value == state)
        }) else {
            return Some(Omitted::Unfinished);
        };
        if events
            .iter()
            .filter(|event| event.event_sequence > completion.event_sequence)
            .any(|event| match &event.payload {
                EventPayload::AnnotationVersionCreated { annotation, .. } => {
                    annotation.task_id == task.task_id
                }
                EventPayload::AnnotationDeleted { annotation_id, .. } => self
                    .current_annotation(annotation_id)
                    .is_some_and(|annotation| annotation.task_id == task.task_id),
                _ => false,
            })
        {
            return Some(Omitted::Unfinished);
        }
        if self
            .active_annotations()
            .filter(|annotation| annotation.task_id == task.task_id)
            .any(|annotation| {
                matches!(
                    annotation.revision_source,
                    RevisionSource::PrelabelSuggestion { .. }
                )
            })
        {
            return Some(Omitted::UnverifiedAnnotations);
        }
        if task.manual_box_guide_migration.is_some() {
            if self.validate_migration_terminal(&task.task_id).is_err() {
                return Some(Omitted::UnresolvedMigration);
            }
            if self
                .migration_dispositions
                .get(&task.task_id)
                .is_some_and(|dispositions| {
                    dispositions.values().any(|disposition| {
                        matches!(
                            disposition.status,
                            MigrationDispositionStatus::Excluded { .. }
                        )
                    })
                })
            {
                return Some(Omitted::ExcludedCoverage);
            }
        }
        match state.outcome {
            Some(TaskOutcome::AnnotationCompleted) if task.review.workflow == ReviewWorkflow::None => None,
            Some(TaskOutcome::ImportedGroundTruth) => {
                matches!(self.import_coverage.get(&task.task_id), Some(ImportCoverage::Complete | ImportCoverage::VerifiedEmpty))
                    .then_some(())
                    .map_or(Some(Omitted::IncompleteCoverage), |_| None)
            }
            Some(TaskOutcome::Approved) if task.review.workflow == ReviewWorkflow::Approval => {
                (self.effective_review_outcome(task) == (TaskStatus::Completed, Some(TaskOutcome::Approved)))
                    .then_some(())
                    .map_or(Some(Omitted::ChangedReviewPolicy), |_| None)
            }
            Some(TaskOutcome::ReviewerCorrected) if task.review.workflow == ReviewWorkflow::Approval
                && task.review.allow_reviewer_corrections
                && self.reviewer_corrections.iter().any(|correction| {
                    correction.task_id == task.task_id
                        && self.current_annotation(&correction.annotation_id).is_some_and(|annotation| {
                            !annotation.deleted && annotation.version == correction.corrected_version
                                && matches!(&annotation.revision_source, RevisionSource::ReviewerCorrection { correction_id }
                                    if correction_id == &correction.correction_id)
                        })
                }) => None,
            _ => Some(Omitted::ChangedReviewPolicy),
        }
    }
}

fn terminal_state<'a>(payload: &'a EventPayload, task_id: &TaskId) -> Option<&'a TaskState> {
    let state = match payload {
        EventPayload::TaskStateChanged { task_state }
        | EventPayload::ReviewerCorrectionRecorded { task_state, .. }
        | EventPayload::ReviewRevisionCommitted { task_state, .. } => task_state,
        EventPayload::ImportInitialized {
            task_initializations,
            ..
        } => {
            &task_initializations
                .iter()
                .find(|value| &value.task_id == task_id)?
                .initial_state
        }
        _ => return None,
    };
    (&state.task_id == task_id && state.status == TaskStatus::Completed).then_some(state)
}

#[cfg(test)]
mod tests;
