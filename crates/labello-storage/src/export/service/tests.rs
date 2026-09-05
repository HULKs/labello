use super::super::capture::tests::fixture;
use super::*;

async fn settled(service: &ExportService, id: &str) -> ExportJob {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = service.job(&DatasetId::from("export"), id).await.unwrap();
            if !job.phase.is_active() {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bounded worker completion")
}

#[tokio::test]
async fn jobs_publish_only_verified_artifacts_and_completed_downloads_survive_restart() {
    let (source, repository, options) = fixture().await;
    let limits = ExportLimits::default();
    let service = ExportService::new(source.path(), limits.clone())
        .await
        .unwrap();
    let dataset = DatasetId::from("export");
    let job = service
        .preflight(&dataset, repository, options)
        .await
        .unwrap();
    assert_eq!(job.phase, ExportPhase::Capturing);
    assert_eq!(
        service.download(&dataset, &job.job_id).await.unwrap_err(),
        ExportFailure::NotReady
    );
    assert_eq!(
        service
            .job(&DatasetId::from("other"), &job.job_id)
            .await
            .unwrap_err(),
        ExportFailure::NotFound
    );
    assert_eq!(
        service
            .start(&DatasetId::from("other"), &job.job_id)
            .await
            .unwrap_err(),
        ExportFailure::NotFound
    );
    assert_eq!(
        settled(&service, &job.job_id).await.phase,
        ExportPhase::Ready
    );
    service.start(&dataset, &job.job_id).await.unwrap();
    let completed = settled(&service, &job.job_id).await;
    assert_eq!(completed.phase, ExportPhase::Succeeded);
    let (file, readback, _permit) = service.download(&dataset, &job.job_id).await.unwrap();
    assert_eq!(readback, completed);
    let second_download = service.download(&dataset, &job.job_id).await.unwrap();
    assert_eq!(
        service.download(&dataset, &job.job_id).await.unwrap_err(),
        ExportFailure::Busy
    );
    drop(second_download);
    let retry_download = service.download(&dataset, &job.job_id).await.unwrap();
    drop(retry_download);
    let zip = zip::ZipArchive::new(file).unwrap();
    assert!(zip.file_names().any(|name| name == "labello-export.json"));
    assert!(!service.job_dir(&job.job_id).unwrap().join("spool").exists());
    drop(service);
    let restarted = ExportService::new(source.path(), limits).await.unwrap();
    assert_eq!(
        restarted.download(&dataset, &job.job_id).await.unwrap().1,
        completed
    );
    std::fs::write(
        restarted.job_dir(&job.job_id).unwrap().join("dataset.zip"),
        b"changed",
    )
    .unwrap();
    assert_eq!(
        restarted.download(&dataset, &job.job_id).await.unwrap_err(),
        ExportFailure::Verification
    );
    assert_eq!(
        restarted.job(&dataset, "../../dataset").await.unwrap_err(),
        ExportFailure::NotFound
    );
}

#[tokio::test]
async fn cancellation_waits_for_the_capture_lock_then_removes_private_payload_and_releases_capacity()
 {
    let (source, repository, options) = fixture().await;
    let service = ExportService::new(source.path(), ExportLimits::default())
        .await
        .unwrap();
    let dataset = DatasetId::from("export");
    let lock = repository.image_lock(&labello_domain::ImageId::from("empty"));
    let guard = lock.lock().await;
    let job = service
        .preflight(&dataset, repository.clone(), options.clone())
        .await
        .unwrap();
    assert_eq!(
        service
            .preflight(&dataset, repository.clone(), options.clone())
            .await
            .unwrap_err(),
        ExportFailure::Busy
    );
    assert_eq!(
        service.cancel(&dataset, &job.job_id).await.unwrap().phase,
        ExportPhase::Cancelling
    );
    drop(guard);
    assert_eq!(
        settled(&service, &job.job_id).await.phase,
        ExportPhase::Cancelled
    );
    assert!(!service.job_dir(&job.job_id).unwrap().join("spool").exists());
    let retry = service
        .preflight(&dataset, repository, options)
        .await
        .unwrap();
    assert_eq!(
        settled(&service, &retry.job_id).await.phase,
        ExportPhase::Ready
    );
}

#[tokio::test]
async fn source_changes_and_atomic_publication_collisions_are_terminal_failures_without_downloads()
{
    let (source, repository, options) = fixture().await;
    let service = ExportService::new(source.path(), ExportLimits::default())
        .await
        .unwrap();
    let dataset = DatasetId::from("export");
    let job = service
        .preflight(&dataset, repository.clone(), options.clone())
        .await
        .unwrap();
    assert_eq!(
        settled(&service, &job.job_id).await.phase,
        ExportPhase::Ready
    );
    let mut metadata = repository.load_dataset().await.unwrap();
    metadata.name = "Changed configuration".into();
    repository.save_dataset(&metadata).await.unwrap();
    service.start(&dataset, &job.job_id).await.unwrap();
    let failed = settled(&service, &job.job_id).await;
    assert_eq!(failed.phase, ExportPhase::Failed);
    assert_eq!(failed.failure, Some(ExportFailure::SourceChanged));
    assert_eq!(
        service.download(&dataset, &job.job_id).await.unwrap_err(),
        ExportFailure::NotReady
    );
    let retry = service
        .preflight(&dataset, repository, options)
        .await
        .unwrap();
    assert_eq!(
        settled(&service, &retry.job_id).await.phase,
        ExportPhase::Ready
    );
    std::fs::write(
        service.job_dir(&retry.job_id).unwrap().join("dataset.zip"),
        b"collision",
    )
    .unwrap();
    service.start(&dataset, &retry.job_id).await.unwrap();
    let failed = settled(&service, &retry.job_id).await;
    assert_eq!(
        (failed.phase, failed.failure),
        (ExportPhase::Failed, Some(ExportFailure::Storage))
    );
    assert!(
        !service
            .job_dir(&retry.job_id)
            .unwrap()
            .join("dataset.zip")
            .exists()
    );
}

#[tokio::test]
async fn restart_marks_unpublished_artifacts_interrupted_and_retention_reclaims_capacity() {
    let (source, repository, options) = fixture().await;
    let limits = ExportLimits {
        max_retained_jobs: 1,
        ..ExportLimits::default()
    };
    let service = ExportService::new(source.path(), limits.clone())
        .await
        .unwrap();
    let dataset = DatasetId::from("export");
    let job = service
        .preflight(&dataset, repository.clone(), options.clone())
        .await
        .unwrap();
    assert_eq!(
        settled(&service, &job.job_id).await.phase,
        ExportPhase::Ready
    );
    assert_eq!(
        service
            .preflight(&dataset, repository.clone(), options.clone())
            .await
            .unwrap_err(),
        ExportFailure::Limit
    );
    // Simulate death after the private artifact link, before durable Succeeded.
    let mut interrupted = service.job(&dataset, &job.job_id).await.unwrap();
    interrupted.phase = ExportPhase::Building;
    service.persist(&interrupted).await.unwrap();
    std::fs::write(
        service.job_dir(&job.job_id).unwrap().join("building.zip"),
        b"partial",
    )
    .unwrap();
    std::fs::write(
        service.job_dir(&job.job_id).unwrap().join("dataset.zip"),
        b"unpublished",
    )
    .unwrap();
    drop(service);
    let restarted = ExportService::new(source.path(), limits).await.unwrap();
    let failed = restarted.job(&dataset, &job.job_id).await.unwrap();
    assert_eq!(
        (failed.phase, failed.failure),
        (ExportPhase::Failed, Some(ExportFailure::Interrupted))
    );
    assert!(
        !restarted
            .job_dir(&job.job_id)
            .unwrap()
            .join("dataset.zip")
            .exists()
    );
    {
        let mut jobs = restarted.inner.jobs.lock().await;
        let current = jobs.get_mut(&job.job_id).unwrap();
        current.job.expires_at = now() - std::time::Duration::from_secs(1);
        restarted.persist(&current.job).await.unwrap();
    }
    assert_eq!(
        restarted.job(&dataset, &job.job_id).await.unwrap_err(),
        ExportFailure::NotFound
    );
    assert!(!restarted.job_dir(&job.job_id).unwrap().exists());
    let retry = restarted
        .preflight(&dataset, repository, options)
        .await
        .unwrap();
    assert_eq!(
        settled(&restarted, &retry.job_id).await.phase,
        ExportPhase::Ready
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn private_control_root_rejects_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".labello-server")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join(".labello-server/exports"))
        .unwrap();
    assert!(matches!(
        ExportService::new(root.path(), ExportLimits::default()).await,
        Err(ExportFailure::InvalidInput)
    ));
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn restart_discards_orphans_and_expired_jobs_before_enforcing_retention_capacity() {
    let (source, repository, options) = fixture().await;
    let service = ExportService::new(source.path(), ExportLimits::default())
        .await
        .unwrap();
    let dataset = DatasetId::from("export");
    let first = service
        .preflight(&dataset, repository.clone(), options.clone())
        .await
        .unwrap();
    let mut expired = settled(&service, &first.job_id).await;
    let second = service
        .preflight(&dataset, repository, options)
        .await
        .unwrap();
    let retained = settled(&service, &second.job_id).await;
    assert_eq!(retained.phase, ExportPhase::Ready);
    expired.expires_at = now() - std::time::Duration::from_secs(1);
    service.persist(&expired).await.unwrap();
    let orphan = service.job_dir(&uuid::Uuid::new_v4().to_string()).unwrap();
    create_private_directory(&orphan).unwrap();
    let orphan_name = orphan.file_name().unwrap().to_owned();
    drop(service);
    let restarted = ExportService::new(
        source.path(),
        ExportLimits {
            max_retained_jobs: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(restarted.jobs(&dataset).await.unwrap().len(), 1);
    assert_eq!(
        restarted
            .job(&dataset, &retained.job_id)
            .await
            .unwrap()
            .failure,
        Some(ExportFailure::Interrupted)
    );
    assert!(!restarted.inner.path.join(orphan_name).exists());
    assert!(!restarted.job_dir(&expired.job_id).unwrap().exists());
}

mod publication;
