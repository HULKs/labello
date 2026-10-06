//! Compact, image-local facts shared by presence and availability readers.
use super::{DatasetRepository, StorageResult};
use labello_domain::{
    AssignmentStatus, EventLogEntry, ImageId, ImageState, ReviewDecision, ReviewTarget, TaskId,
    UserId,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug)]
pub(crate) struct PollingImage {
    // Deliberately partial: only the fields consumed by assignment eligibility.
    pub(super) state: ImageState,
    workflow: parking_lot::Mutex<Arc<ImageState>>,
    normal: BTreeMap<TaskId, FinalReviews>,
    migration: BTreeMap<TaskId, FinalReviews>,
}

#[derive(Debug, Default)]
struct FinalReviews {
    users: BTreeSet<UserId>,
    approved: bool,
}

impl PollingImage {
    pub(crate) fn completion(&self) -> crate::completion_projection::ImageCompletion {
        crate::completion_projection::ImageCompletion::from_state(&self.state)
    }

    fn new(state: &ImageState, events: &[EventLogEntry]) -> Self {
        let mut compact = ImageState::new(state.image_id.clone());
        compact.current_sequence = state.current_sequence;
        compact.task_states = state.task_states.clone();
        compact.import_coverage = state.import_coverage.clone();
        compact.included_import_tasks = state.included_import_tasks.clone();
        compact.assignments = state
            .assignments
            .iter()
            .filter(|a| a.status == AssignmentStatus::Active)
            .cloned()
            .collect();
        for assignment in &compact.assignments {
            if let Some(context) = state
                .review_assignment_contexts
                .get(&assignment.assignment_id)
            {
                compact
                    .review_assignment_contexts
                    .insert(assignment.assignment_id.clone(), context.clone());
            }
        }
        let tasks = state
            .task_states
            .keys()
            .cloned()
            .chain(
                state
                    .reviews
                    .iter()
                    .filter_map(|review| match &review.target {
                        ReviewTarget::Task { task_id }
                        | ReviewTarget::MigrationConfirmation { task_id, .. } => {
                            Some(task_id.clone())
                        }
                        _ => None,
                    }),
            )
            .collect::<BTreeSet<_>>();
        let mut normal = BTreeMap::new();
        let mut migration = BTreeMap::new();
        for task in tasks {
            let mut ordinary = FinalReviews::default();
            for review in labello_domain::current_task_reviews(events, &task) {
                ordinary.approved |= review.decision == ReviewDecision::Approved;
                ordinary.users.insert(review.reviewer_user_id);
            }
            normal.insert(task.clone(), ordinary);
            let mut migrated = FinalReviews::default();
            for review in labello_domain::current_migration_reviews(events, &task) {
                if matches!(&review.target, ReviewTarget::MigrationConfirmation { task_id, .. } if task_id == &task)
                {
                    migrated.approved |= review.decision == ReviewDecision::Approved;
                    migrated.users.insert(review.reviewer_user_id.clone());
                }
            }
            migration.insert(task, migrated);
        }
        Self {
            state: compact,
            workflow: parking_lot::Mutex::new(Arc::new(state.clone())),
            normal,
            migration,
        }
    }

    pub(super) fn already_final(&self, task: &TaskId, user: &UserId, migration: bool) -> bool {
        let reviews = if migration {
            &self.migration
        } else {
            &self.normal
        };
        reviews
            .get(task)
            .is_some_and(|r| r.approved || r.users.contains(user))
    }
}

impl DatasetRepository {
    pub(crate) async fn workflow_polling_state(
        &self,
        image: &ImageId,
        visibility: labello_domain::BoundingBoxVisibility,
    ) -> StorageResult<Arc<ImageState>> {
        let image = self.polling_image(image).await?;
        let mut state = image.workflow.lock();
        if state.bounding_box_visibility != Some(visibility) {
            Arc::make_mut(&mut state).bounding_box_visibility = Some(visibility);
        }
        Ok(state.clone())
    }

    pub(crate) async fn polling_image(&self, image: &ImageId) -> StorageResult<Arc<PollingImage>> {
        self.ensure_artifact_migration().await?;
        let lock = self.image_lock(image);
        let _guard = lock.lock().await;
        self.polling_image_unlocked(image).await
    }

    pub(super) async fn polling_image_unlocked(
        &self,
        image: &ImageId,
    ) -> StorageResult<Arc<PollingImage>> {
        if let Some(value) = self.polling_images.lock().get(image).cloned() {
            return Ok(value);
        }
        let (state, events) = self.load_image_state_with_events(image).await?;
        let value = Arc::new(PollingImage::new(&state, &events));
        self.polling_images
            .lock()
            .insert(image.clone(), value.clone());
        Ok(value)
    }
}
