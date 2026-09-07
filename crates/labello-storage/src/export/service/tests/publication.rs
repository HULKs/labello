use super::*;
use std::{future::Future, task::Poll};

async fn prepared_build() -> (
    tempfile::TempDir,
    DatasetRepository,
    ExportService,
    ExportJob,
    Arc<Capture>,
    Arc<AtomicBool>,
) {
    let (source, repository, options) = fixture().await;
    let service = ExportService::new(source.path(), ExportLimits::default())
        .await
        .unwrap();
    let job = service
        .preflight(&DatasetId::from("export"), repository.clone(), options)
        .await
        .unwrap();
    assert_eq!(
        settled(&service, &job.job_id).await.phase,
        ExportPhase::Ready
    );
    let mut jobs = service.inner.jobs.lock().await;
    let current = jobs.get_mut(&job.job_id).unwrap();
    current.job.phase = ExportPhase::Building;
    service.persist(&current.job).await.unwrap();
    let capture = current.capture.as_ref().unwrap().clone();
    let cancel = current.cancel.clone();
    let job = current.job.clone();
    drop(jobs);
    (source, repository, service, job, capture, cancel)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_serializes_config_and_index_writers_until_the_archive_cut() {
    let (_source, repository, service, job, capture, cancel) = prepared_build().await;
    let mut metadata = repository.load_dataset().await.unwrap();
    metadata.name = "Changed after the archive cut".into();
    let mut index = repository.load_images_index().await.unwrap();
    index.images_by_hash.clear();
    let pause = Arc::new(PublicationPause {
        reached: tokio::sync::Notify::new(),
        resume: std::sync::Barrier::new(2),
    });
    *service.inner.publication_pause.lock().unwrap() = Some(pause.clone());
    let worker = service.clone();
    let id = job.job_id.clone();
    let build = tokio::task::spawn_blocking(move || worker.build(&id, &capture, &cancel));
    tokio::time::timeout(std::time::Duration::from_secs(10), pause.reached.notified())
        .await
        .unwrap();
    let configuration_guarded = repository.review_config_lock.try_write().is_err();
    let index_guarded = repository.images_index_cache.try_write().is_err();
    let mut configuration_write = std::pin::pin!(repository.save_dataset(&metadata));
    let mut index_write = std::pin::pin!(repository.save_images_index(&index));
    let configuration_waited =
        std::future::poll_fn(|cx| Poll::Ready(configuration_write.as_mut().poll(cx).is_pending()))
            .await;
    let index_waited =
        std::future::poll_fn(|cx| Poll::Ready(index_write.as_mut().poll(cx).is_pending())).await;
    // Release even if a missing production guard let either writer complete.
    pause.resume.wait();
    let artifact = build.await.unwrap();
    assert!(
        configuration_waited,
        "configuration changed during publication"
    );
    assert!(index_waited, "index changed during publication");
    assert!(
        configuration_guarded,
        "publication did not retain the configuration guard"
    );
    assert!(index_guarded, "publication did not retain the index guard");
    assert!(artifact.is_ok());
    let directory = service.job_dir(&job.job_id).unwrap();
    assert!(directory.join("dataset.zip").exists());
    assert!(!directory.join("building.zip").exists());
    configuration_write.await.unwrap();
    index_write.await.unwrap();
    assert_eq!(
        service
            .download(&job.dataset_id, &job.job_id)
            .await
            .unwrap_err(),
        ExportFailure::NotReady
    );
    service.finish_build(&job.job_id, artifact).await.unwrap();
    // These changes occurred after the immutable archive cut and cannot alter it.
    assert!(service.download(&job.dataset_id, &job.job_id).await.is_ok());
}

#[tokio::test]
async fn cancellation_and_restart_after_the_archive_cut_never_expose_unfinished_downloads() {
    for restart in [false, true] {
        let (source, _repository, service, job, capture, cancel) = prepared_build().await;
        let worker = service.clone();
        let id = job.job_id.clone();
        let artifact = tokio::task::spawn_blocking(move || worker.build(&id, &capture, &cancel))
            .await
            .unwrap();
        assert!(artifact.is_ok());
        assert!(
            service
                .job_dir(&job.job_id)
                .unwrap()
                .join("dataset.zip")
                .exists()
        );
        assert_eq!(
            service
                .download(&job.dataset_id, &job.job_id)
                .await
                .unwrap_err(),
            ExportFailure::NotReady
        );
        let final_service = if restart {
            drop(service);
            ExportService::new(source.path(), ExportLimits::default())
                .await
                .unwrap()
        } else {
            assert_eq!(
                service
                    .cancel(&job.dataset_id, &job.job_id)
                    .await
                    .unwrap()
                    .phase,
                ExportPhase::Cancelling
            );
            service.finish_build(&job.job_id, artifact).await.unwrap();
            service
        };
        let finished = final_service
            .job(&job.dataset_id, &job.job_id)
            .await
            .unwrap();
        assert_eq!(
            finished.phase,
            if restart {
                ExportPhase::Failed
            } else {
                ExportPhase::Cancelled
            }
        );
        if restart {
            assert_eq!(finished.failure, Some(ExportFailure::Interrupted));
        }
        assert!(
            !final_service
                .job_dir(&job.job_id)
                .unwrap()
                .join("dataset.zip")
                .exists()
        );
        assert_eq!(
            final_service
                .download(&job.dataset_id, &job.job_id)
                .await
                .unwrap_err(),
            ExportFailure::NotReady
        );
    }
}
