use super::*;
use std::{
    future::Future,
    task::{Context, Waker},
};

fn user(name: &str) -> Owner {
    Owner::Inference(InferenceOwner::Interactive {
        dataset: "dataset".into(),
        user: name.into(),
    })
}

#[test]
fn worker_configuration_is_optional_and_bounded() {
    let config: PrelabelFileConfig = toml::from_str("modelsRoot = '/models'").unwrap();
    assert_eq!(config.workers, WorkerConfig::default());
    for (workers, idle, threads) in [
        (0, 120, 1),
        (9, 120, 1),
        (1, 0, 1),
        (1, 3601, 1),
        (1, 120, 0),
        (1, 120, 17),
    ] {
        assert!(
            WorkerConfig {
                max_workers: workers,
                idle_timeout_seconds: idle,
                threads_per_worker: threads
            }
            .validate()
            .is_err()
        );
    }
}

#[tokio::test]
async fn owner_slots_are_exclusive_bounded_and_release_on_cancellation() {
    let pool = Pool::new(WorkerConfig {
        max_workers: 1,
        ..Default::default()
    });
    let first = pool.acquire(user("one")).await.unwrap();
    let mut context = Context::from_waker(Waker::noop());
    let mut same = Box::pin(pool.acquire(user("one")));
    assert!(same.as_mut().poll(&mut context).is_pending());
    let mut another = Box::pin(pool.acquire(user("two")));
    assert!(another.as_mut().poll(&mut context).is_pending());
    drop(first);
    let mut same = same.await.unwrap();
    assert!(another.as_mut().poll(&mut context).is_pending());
    same.keep();
    // An idle owner's slot may be evicted to honor the global bound.
    let _another = another.await.unwrap();
}

#[tokio::test]
async fn run_release_retires_workers() {
    let pool = Pool::new(WorkerConfig {
        max_workers: 1,
        idle_timeout_seconds: 1,
        ..Default::default()
    });
    let owner = Owner::Inference(InferenceOwner::Batch {
        dataset: "dataset".into(),
        run: "run".into(),
    });
    let mut active = pool.acquire(owner.clone()).await.unwrap();
    pool.release(&owner);
    active.keep(); // A completed/cancelled scope cannot put its worker back.
    drop(active);
    let mut next = pool.acquire(user("one")).await.unwrap();
    next.keep();
    drop(next);
    let _next = pool.acquire(user("two")).await.unwrap();
}

#[tokio::test]
async fn cancelled_worker_is_killed_reaped_and_releases_capacity() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1));
    let child = tokio::process::Command::new("/bin/sleep")
        .arg("30")
        .env_clear()
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let path = PathBuf::from(format!("/proc/{}", child.id().unwrap()));
    let worker = process::WorkerProcess {
        child: Some(child),
        permit: Some(budget.clone().acquire_owned().await.unwrap()),
    };
    drop(worker);
    let _permit = tokio::time::timeout(Duration::from_secs(2), budget.acquire())
        .await
        .unwrap()
        .unwrap();
    assert!(
        !path.exists(),
        "capacity must not be reused until the old worker is reaped"
    );
}

#[tokio::test]
async fn shutdown_rejects_waiting_and_new_worker_scopes() {
    let pool = Pool::new(WorkerConfig {
        max_workers: 1,
        ..Default::default()
    });
    let _active = pool.acquire(user("one")).await.unwrap();
    let mut waiting = Box::pin(pool.acquire(user("two")));
    assert!(
        waiting
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    pool.shutdown();
    assert!(waiting.await.is_err());
    assert!(pool.acquire(user("one")).await.is_err());
}
#[test]
fn warm_provider_precedes_cold_preference_without_duplicate_attempts() {
    assert_eq!(
        provider_order(None).collect::<Vec<_>>(),
        NativeProvider::PREFERENCE
    );
    assert_eq!(
        provider_order(Some(NativeProvider::Cpu)).collect::<Vec<_>>(),
        [
            NativeProvider::Cpu,
            NativeProvider::Cuda,
            NativeProvider::WebGpu
        ]
    );
    assert_eq!(
        provider_order(Some(NativeProvider::Cuda)).collect::<Vec<_>>(),
        NativeProvider::PREFERENCE
    );
}
