use super::*;
use labello_domain::{ClassId, ImageExplorerItem, TaskDefinition, TaskStatus};

/// Only event-derived filter metadata is cached. Index records and configured
/// pending workflows are composed from the current request's metadata.
#[derive(Clone, Debug)]
pub(super) struct ImageExplorerSummary {
    task_statuses: BTreeMap<TaskId, TaskStatus>,
    class_ids: BTreeSet<ClassId>,
}

impl DatasetRepository {
    pub async fn image_explorer_item(
        &self,
        image: ImageRecord,
        tasks: &[TaskDefinition],
    ) -> StorageResult<ImageExplorerItem> {
        self.ensure_artifact_migration().await?;
        let lock = self.image_lock(&image.image_id);
        let _guard = lock.lock().await;
        let cached = self.explorer_cache.lock().get(&image.image_id).cloned();
        let summary = if let Some(summary) = cached {
            summary
        } else {
            let state = self.load_image_state(&image.image_id).await?;
            let summary = ImageExplorerSummary {
                task_statuses: state
                    .task_states
                    .iter()
                    .map(|(id, state)| (id.clone(), state.status.clone()))
                    .collect(),
                class_ids: state
                    .active_annotations()
                    .map(|a| a.class_id.clone())
                    .collect(),
            };
            self.explorer_cache
                .lock()
                .insert(image.image_id.clone(), summary.clone());
            summary
        };
        let mut task_statuses: BTreeMap<_, _> = tasks
            .iter()
            .map(|task| (task.task_id.clone(), TaskStatus::Pending))
            .collect();
        task_statuses.extend(summary.task_statuses);
        Ok(ImageExplorerItem {
            image,
            task_statuses,
            class_ids: summary.class_ids,
        })
    }
}
