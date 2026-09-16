use super::*;
use labello_domain::{ImageState, ReturnToReviewRequest};

impl DatasetRepository {
    pub async fn return_to_review(
        &self,
        user_id: &UserId,
        image_id: &ImageId,
        request: ReturnToReviewRequest,
    ) -> StorageResult<ImageState> {
        request.validate()?;
        image_id
            .validate_path_segment()
            .map_err(|_| StorageError::InvalidAssignment("invalid image identity".into()))?;
        let _config_guard = self.review_config_lock.read().await;
        let metadata = self.load_dataset().await?;
        let role = [DatasetRole::Reviewer, DatasetRole::DataAdmin]
            .into_iter()
            .find(|role| {
                require_role(
                    &metadata.role_assignments,
                    &metadata.dataset_id,
                    user_id,
                    role.clone(),
                )
                .is_ok()
            })
            .ok_or_else(|| {
                StorageError::Unauthorized("reviewer or data-admin role is required".into())
            })?;
        let tasks = request
            .task_ids
            .iter()
            .map(|id| {
                ensure_assignment_target_exists(&metadata, image_id, id)?;
                Ok(metadata.task(id).expect("validated task").clone())
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let lock = self.image_lock(image_id);
        let _guard = lock.lock().await;
        let state = self.load_image_state(image_id).await?;
        for event in self.load_events(image_id).await? {
            if let EventPayload::WorkReturnedToReview { request: saved, .. } = &event.payload
                && saved.request_id == request.request_id
            {
                return if **saved == request && event.actor_user_id == *user_id {
                    Ok(state)
                } else {
                    Err(StorageError::AssignmentConflict(
                        "return request identity was reused".into(),
                    ))
                };
            }
        }
        state.validate_return_to_review(&request, &tasks, labello_domain::now())
            .map_err(|_| StorageError::AssignmentConflict("work changed, is assigned, or is not completed approval work; refresh the image".into()))?;
        let (_, state) = self
            .append_payloads_with_state_unlocked(
                image_id,
                &Actor {
                    user_id: user_id.clone(),
                    role,
                },
                vec![EventPayload::WorkReturnedToReview {
                    request: Box::new(request),
                    tasks,
                }],
            )
            .await?;
        Ok(state)
    }
}
