//! Presentation of server-owned Objects/Overview queues and their history.
use labello_domain::{
    Assignment, WorkflowAssignmentContext, WorkflowHistoryEntry, WorkflowItemRef,
    WorkflowSelection, WorkflowVariant,
};

use crate::app::{LabelloApp, LoadedImage};
mod edits;

#[derive(Default)]
pub(crate) struct WorkflowSession {
    pub variant: WorkflowVariant,
    pub variant_selected: bool,
    pub history: Vec<WorkflowHistoryEntry>,
    pub history_request: Option<u64>,
    pub current_root: Option<labello_domain::AssignmentId>,
    pub excluded: Option<WorkflowItemRef>,
    pub change_notice: Option<String>,
    pub navigation_error: Option<String>,
    pub revalidating_history: bool,
    pub returning_forward: bool,
}

impl LoadedImage {
    pub(crate) fn workflow_item(&self) -> Option<WorkflowItemRef> {
        let context = self
            .state
            .workflow_assignments
            .get(&self.assignment.assignment_id)?;
        Some(WorkflowItemRef {
            image_id: self.assignment.image_id.clone(),
            item: context.item.clone(),
        })
    }
}

impl LabelloApp {
    pub(crate) fn select_initial_workflow_variant(&mut self, task: &labello_domain::TaskId) {
        let objects = self.workflow_variant_availability(task, WorkflowVariant::Objects);
        let overview = self.workflow_variant_availability(task, WorkflowVariant::Overview);
        let overview_available = overview.is_some_and(|entry| entry.available);
        let objects_available = objects.is_some_and(|entry| entry.available);
        let variant = if objects_available
            || (objects.is_some_and(|entry| entry.split) && !overview_available)
        {
            WorkflowVariant::Objects
        } else {
            WorkflowVariant::Overview
        };
        let resolved = overview.is_none() || overview_available || objects_available;
        self.work.workflow.variant = variant;
        self.work.workflow.variant_selected = resolved;
    }

    pub(crate) fn advance_work_item(&mut self, ctx: &eframe::egui::Context) -> bool {
        if self.workflow_context().is_none() {
            return false;
        }
        self.work.pending_transition = None;
        if let Some(entry) = self.forward_work_item().cloned() {
            self.request_history_item(entry);
        } else if !self.promote_prepared_assignment(ctx, None) {
            self.retire_current_image();
            self.request_next_image();
        }
        true
    }
    pub(crate) fn current_workflow_draft(
        &self,
    ) -> Option<labello_client::SaveWorkflowDraftRequest> {
        let context = self.workflow_context()?;
        if context.item.variant() != WorkflowVariant::Objects
            || self.view == crate::app::AppView::Review
        {
            return self.current_workflow_edits();
        }
        let assignment = self.work.assignment.as_ref()?;
        let geometry = self
            .work
            .migration
            .draft
            .clone()
            .map(|mut skeleton| {
                skeleton
                    .keypoints
                    .truncate(self.work.migration.keypoint_index);
                labello_domain::AnnotationGeometry::Skeleton(skeleton)
            })
            .or_else(|| {
                self.selected_prelabel_object()
                    .map(|item| item.annotation.geometry.clone())
            })
            .or_else(|| {
                self.work
                    .annotations
                    .iter()
                    .find(|a| self.annotation_matches_selected_workflow(a))
                    .map(|a| a.geometry.clone())
            })?;
        Some(labello_client::SaveWorkflowDraftRequest {
            assignment: labello_client::AssignmentActionRequest {
                assignment_id: assignment.assignment_id.clone(),
                image_id: assignment.image_id.clone(),
                task_id: assignment.task_id.clone(),
                kind: assignment.kind.clone(),
            },
            draft: labello_client::WorkflowDraftInput::Geometry { geometry },
            expected_sequence: self
                .work
                .current_state
                .as_ref()?
                .workflow_object_draft(&assignment.task_id, &context.item)
                .map_or(0, |draft| draft.sequence),
        })
    }
    pub(crate) fn scope_workflow_annotations(&mut self) {
        let Some(context) = self.workflow_context().cloned() else {
            return;
        };
        let state = self
            .work
            .current_state
            .as_ref()
            .expect("workflow context has state");
        self.work.annotations.retain(|annotation| {
            state.workflow_object_matches_annotation(&context.item, annotation)
        });
        self.work.persisted_annotations = state
            .annotations
            .values()
            .filter_map(|versions| versions.last())
            .filter(|annotation| {
                state.workflow_object_matches_annotation(&context.item, annotation)
            })
            .map(|annotation| annotation.annotation_id.clone())
            .collect();
        if self.view == crate::app::AppView::Annotate
            && let labello_domain::WorkflowItem::Object { object } = &context.item
            && let Some(task) = self.work.selected_task_id.as_ref()
            && !state.workflow_annotation_confirmed(task, object)
            && let Some(draft) = state.workflow_object_draft(task, &context.item)
            && let Some(annotation) = self
                .work
                .annotations
                .iter_mut()
                .find(|a| &a.task_id == task)
        {
            annotation.geometry = draft.geometry.clone();
            self.recompute_modified_annotations();
        }
    }

    pub(crate) fn install_workflow_item(&mut self) {
        use labello_domain::{WorkflowItem, WorkflowObject};
        let Some(context) = self.workflow_context().cloned() else {
            return;
        };
        self.work.workflow.variant = context.item.variant();
        self.work.workflow.variant_selected = true;
        self.work.workflow.current_root = self
            .work
            .assignment
            .as_ref()
            .map(|assignment| self.workflow_root(assignment));
        self.scope_workflow_annotations();
        let assignment = self
            .work
            .assignment
            .as_ref()
            .expect("workflow context has assignment");
        let state = self
            .work
            .current_state
            .as_ref()
            .expect("workflow context has state");
        let hints = match &context.item {
            WorkflowItem::Object {
                object: WorkflowObject::Prelabel { suggestion_id },
            } => state
                .workflow_preparations
                .get(&assignment.task_id)
                .map(|preparation| {
                    preparation
                        .prelabels
                        .iter()
                        .filter(|hint| &hint.suggestion_id == suggestion_id)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let draft = match &context.item {
            WorkflowItem::Object { object }
                if !state.workflow_annotation_confirmed(&assignment.task_id, object) =>
            {
                state
                    .workflow_object_draft(&assignment.task_id, &context.item)
                    .cloned()
            }
            _ => None,
        };
        if let Some(current) = &mut self.work.current {
            current.prelabels = hints;
        }
        self.sync_prelabel_review();
        if self.view == crate::app::AppView::Annotate {
            if let Some(draft) = draft {
                if let Some(pending) = self.work.prelabel_review.objects.first_mut() {
                    pending.annotation.geometry = draft.geometry;
                } else if let Some(annotation) = self
                    .work
                    .annotations
                    .iter_mut()
                    .find(|a| Some(&a.task_id) == self.work.selected_task_id.as_ref())
                {
                    annotation.geometry = draft.geometry;
                    self.recompute_modified_annotations();
                }
            }
            self.work.selected_annotation = self
                .work
                .prelabel_review
                .objects
                .first()
                .map(|item| item.annotation.annotation_id.clone())
                .or_else(|| {
                    self.work
                        .annotations
                        .iter()
                        .find(|a| self.annotation_matches_selected_workflow(a))
                        .map(|a| a.annotation_id.clone())
                });
            if let Some(annotation) = self
                .work
                .annotations
                .iter()
                .find(|a| Some(&a.annotation_id) == self.work.selected_annotation.as_ref())
                && let labello_domain::AnnotationGeometry::Skeleton(skeleton) = &annotation.geometry
                && self
                    .selected_task()
                    .and_then(|task| task.skeleton.as_ref())
                    .is_some_and(|spec| skeleton.keypoints.len() < spec.keypoints.len())
            {
                self.work.active_skeleton = Some(annotation.annotation_id.clone());
                self.work.skeleton_keypoint_index = skeleton.keypoints.len();
            }
        }
        self.work.review_index = match context.item {
            WorkflowItem::Object { .. } => 0,
            WorkflowItem::Overview => self
                .work
                .annotations
                .iter()
                .filter(|a| self.annotation_matches_selected_workflow(a))
                .count(),
        };
        self.restore_workflow_edits();
        self.sync_review_selection();
        if self.work.workflow.variant == WorkflowVariant::Objects {
            let annotation = self.annotation_objects().into_iter().next();
            if let Some(annotation) = annotation {
                self.work.canvas.focus_annotation(&annotation);
            }
        } else {
            self.work.canvas.fit_view();
        }
        self.request_workflow_history();
    }

    pub(crate) fn request_workflow_history(&mut self) {
        let Some(selection) = self.workflow_selection() else {
            return;
        };
        if self.runtime.api.is_none() {
            return;
        }
        // A new display supersedes history captured before it. Otherwise a slow
        // reply can omit the current visit and leave Previous unavailable.
        if let Some(previous) = self.work.workflow.history_request.take() {
            self.runtime.active_requests.remove(&previous);
            self.runtime.commands.retain(|command| !matches!(command,
                crate::app::UiCommand::WorkflowHistory { request, .. } if request.request_id == previous));
        }
        let operation_id = self.next_operation();
        let request = self.operation_identity(operation_id, self.config.dataset_id.clone());
        self.work.workflow.history_request = Some(operation_id);
        self.queue_command(crate::app::UiCommand::WorkflowHistory {
            request,
            dataset_id: self.config.dataset_id.clone(),
            selection,
        });
    }

    pub(crate) fn previous_work_item(&self) -> Option<&WorkflowHistoryEntry> {
        let current = self.work.workflow.current_root.as_ref()?;
        let index = self
            .work
            .workflow
            .history
            .iter()
            .position(|entry| &entry.assignment_id == current)?;
        self.work.workflow.history.get(index + 1)
    }

    pub(crate) fn forward_work_item(&self) -> Option<&WorkflowHistoryEntry> {
        let current = self.work.workflow.current_root.as_ref()?;
        let index = self
            .work
            .workflow
            .history
            .iter()
            .position(|entry| &entry.assignment_id == current)?;
        self.work.workflow.history.get(index.checked_sub(1)?)
    }

    pub(crate) fn request_history_item(&mut self, entry: WorkflowHistoryEntry) {
        let Some(selection) = self.workflow_selection() else {
            return;
        };
        if self.loading.image || self.loading.saving || self.runtime.api.is_none() {
            return;
        }
        self.work.workflow.returning_forward = self.forward_work_item() == Some(&entry);
        let operation_id = self.begin_load();
        let request = self.operation_identity(operation_id, self.config.dataset_id.clone());
        self.queue_command(crate::app::UiCommand::ReopenWorkItem {
            request,
            operation_id,
            dataset_id: self.config.dataset_id.clone(),
            item: labello_client::AssignmentActionRequest {
                assignment_id: entry.assignment_id,
                image_id: entry.image_id,
                task_id: selection.task_id,
                kind: selection.kind,
            },
        });
    }

    pub(crate) fn resume_work_item_navigation(&mut self) {
        match self.work.pending_transition.clone() {
            Some(crate::app::PendingTransition::WorkItemHistory(entry)) => {
                self.work.pending_transition = None;
                self.request_history_item(entry);
            }
            Some(_) => self.request_release(),
            None => {}
        }
    }

    pub(crate) fn workflow_context(&self) -> Option<&WorkflowAssignmentContext> {
        self.work
            .current_state
            .as_ref()?
            .workflow_assignments
            .get(&self.work.assignment.as_ref()?.assignment_id)
    }

    pub(crate) fn workflow_selection(&self) -> Option<WorkflowSelection> {
        Some(WorkflowSelection {
            task_id: self.work.selected_task_id.clone()?,
            kind: self.assignment_kind()?,
            variant: self.work.workflow.variant,
        })
    }

    pub(crate) fn workflow_item_ref(&self) -> Option<WorkflowItemRef> {
        Some(WorkflowItemRef {
            image_id: self.work.assignment.as_ref()?.image_id.clone(),
            item: self.workflow_context()?.item.clone(),
        })
    }

    pub(crate) fn workflow_exclusions(&self) -> Vec<WorkflowItemRef> {
        let mut items = self.work.queue.prepared_work_items();
        items.extend(self.workflow_item_ref().or_else(|| {
            self.work
                .assignment
                .as_ref()
                .map(|assignment| WorkflowItemRef {
                    image_id: assignment.image_id.clone(),
                    item: labello_domain::WorkflowItem::Overview,
                })
        }));
        items.extend(self.work.workflow.excluded.clone());
        for image_id in self.assignment_exclusions() {
            if !items.iter().any(|item| item.image_id == image_id) {
                items.push(WorkflowItemRef {
                    image_id,
                    item: labello_domain::WorkflowItem::Overview,
                });
            }
        }
        items.truncate(labello_domain::MAX_PRELOAD_QUEUE_SIZE + 2);
        items
    }

    pub(crate) fn workflow_variant_availability(
        &self,
        task_id: &labello_domain::TaskId,
        variant: WorkflowVariant,
    ) -> Option<&labello_domain::WorkflowAvailability> {
        let kind = self.assignment_kind()?;
        if self.work.availability.dataset_id.as_ref() != Some(&self.config.dataset_id)
            || self.work.availability.kind.as_ref() != Some(&kind)
        {
            return None;
        }
        self.work.availability.workflows.iter().find(|entry| {
            entry.selection.kind == kind
                && entry.selection.task_id == *task_id
                && entry.selection.variant == variant
        })
    }

    pub(crate) fn workflow_root(&self, assignment: &Assignment) -> labello_domain::AssignmentId {
        self.work
            .current_state
            .as_ref()
            .and_then(|state| state.workflow_assignments.get(&assignment.assignment_id))
            .and_then(|context| context.source_assignment_id.clone())
            .unwrap_or_else(|| assignment.assignment_id.clone())
    }
}
