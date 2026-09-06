use std::io::Read;

use crate::export::{ExportFailure, archive::open_regular};

use super::*;

impl DatasetRepository {
    /// Capture the authoritative log while holding the same lock as event mutations.
    /// Replaying locally avoids writing the source dataset's derived state cache.
    pub(crate) async fn export_image_cut(
        &self,
        image_id: &ImageId,
        maximum_bytes: u64,
    ) -> Result<(ImageState, Vec<EventLogEntry>), ExportFailure> {
        image_id
            .validate_path_segment()
            .map_err(|_| ExportFailure::InvalidInput)?;
        let lock = self.image_lock(image_id);
        let _guard = lock.lock().await;
        let path = self.events_path(image_id);
        let events = if tokio::fs::try_exists(&path)
            .await
            .map_err(|_| ExportFailure::Storage)?
        {
            let root = self.root().to_path_buf();
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| ExportFailure::InvalidInput)?
                .to_str()
                .ok_or(ExportFailure::InvalidInput)?
                .to_owned();
            tokio::task::spawn_blocking(move || {
                let root = std::fs::File::open(root).map_err(|_| ExportFailure::Storage)?;
                let file = open_regular(&root, &relative)?;
                if file.metadata().map_err(|_| ExportFailure::Storage)?.len() > maximum_bytes {
                    return Err(ExportFailure::Limit);
                }
                let mut text = String::new();
                file.take(maximum_bytes.saturating_add(1))
                    .read_to_string(&mut text)
                    .map_err(|_| ExportFailure::Storage)?;
                if text.len() as u64 > maximum_bytes {
                    return Err(ExportFailure::Limit);
                }
                text.lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| {
                        serde_json::from_str::<EventLogEntry>(line)
                            .map_err(|_| ExportFailure::InvalidInput)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(|_| ExportFailure::Storage)??
        } else {
            Vec::new()
        };
        for event in &events {
            labello_domain::validate_supported_schema_version(event.schema_version)
                .map_err(|_| ExportFailure::InvalidInput)?;
        }
        let state =
            rebuild_state(image_id.clone(), &events).map_err(|_| ExportFailure::InvalidInput)?;
        Ok((state, events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labello_domain::{DatasetId, DatasetRole, TaskOutcome, TaskState, TaskStatus};

    #[tokio::test]
    async fn export_cut_waits_for_event_transaction_and_does_not_modify_source_cache() {
        let root = tempfile::tempdir().unwrap();
        let repository = DatasetRepository::new(root.path());
        repository
            .initialize(DatasetMetadata::new(
                DatasetId::from("export"),
                "Export",
                now(),
            ))
            .await
            .unwrap();
        let image = ImageId::from("image");
        let lock = repository.image_lock(&image);
        let guard = lock.lock().await;
        let reader = repository.clone();
        let read_image = image.clone();
        let capture = tokio::spawn(async move { reader.export_image_cut(&read_image, 4096).await });
        tokio::task::yield_now().await;
        assert!(!capture.is_finished());
        let timestamp = now();
        let event = EventLogEntry::new(
            1,
            image.clone(),
            UserId::from("user"),
            DatasetRole::Annotator,
            timestamp,
            EventPayload::TaskStateChanged {
                task_state: TaskState {
                    task_id: TaskId::from("task"),
                    status: TaskStatus::Completed,
                    outcome: Some(TaskOutcome::AnnotationCompleted),
                    assigned_to: None,
                    completed_by: Some(UserId::from("user")),
                    completed_at: Some(timestamp),
                    updated_at: timestamp,
                },
            },
        );
        std::fs::create_dir_all(repository.annotations_dir(&image)).unwrap();
        std::fs::write(
            repository.events_path(&image),
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .unwrap();
        drop(guard);
        let (state, events) = capture.await.unwrap().unwrap();
        assert_eq!(state.current_sequence, 1);
        assert_eq!(events, vec![event]);
        assert!(!repository.state_path(&image).exists());
        assert_eq!(
            repository.export_image_cut(&image, 1).await.unwrap_err(),
            ExportFailure::Limit
        );
    }
}
