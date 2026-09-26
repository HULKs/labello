use labello_domain::{PrelabelConfig, TaskDefinition};
use labello_storage::prelabel::{
    InferenceFuture, PrelabelFailure, PrelabelLimits, PrelabelRunner, PrelabelService,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
mod pool;
mod process;
#[cfg(test)]
mod tests;
mod worker;

use labello_inference::NativeProvider;
use labello_storage::prelabel::InferenceOwner;
use pool::{Owner, Pool};
use process::{WorkerOperation, run_inspection};
pub use worker::worker;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrelabelFileConfig {
    pub models_root: PathBuf,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub limits: PrelabelLimits,
    #[serde(default)]
    pub runtime: labello_inference::NativeRuntimeConfig,
    #[serde(default)]
    pub workers: WorkerConfig,
}
fn default_timeout() -> u64 {
    120
}

pub async fn service(
    root: &std::path::Path,
    config: PrelabelFileConfig,
) -> anyhow::Result<PrelabelService> {
    if config.timeout_seconds == 0 || config.timeout_seconds > 300 {
        anyhow::bail!("prelabel timeoutSeconds must be 1..300");
    }
    for path in [&config.runtime.onnx_library, &config.runtime.webgpu_library]
        .into_iter()
        .flatten()
    {
        if !path.is_absolute() {
            anyhow::bail!("prelabel runtime library paths must be absolute");
        }
    }
    config.workers.validate()?;
    let pool = Pool::new(config.workers.clone());
    Ok(PrelabelService::new(
        root,
        &config.models_root,
        config.limits,
        Arc::new(ProcessRunner {
            timeout: Duration::from_secs(config.timeout_seconds),
            runtime: config.runtime,
            pool,
        }),
    )
    .await?)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct WorkerConfig {
    pub max_workers: usize,
    pub idle_timeout_seconds: u64,
    pub threads_per_worker: usize,
}
impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_workers: 4,
            idle_timeout_seconds: 120,
            threads_per_worker: 1,
        }
    }
}
impl WorkerConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if !(1..=8).contains(&self.max_workers)
            || !(1..=3600).contains(&self.idle_timeout_seconds)
            || !(1..=16).contains(&self.threads_per_worker)
        {
            anyhow::bail!("invalid prelabel worker limits");
        }
        Ok(())
    }
}

struct ProcessRunner {
    timeout: Duration,
    runtime: labello_inference::NativeRuntimeConfig,
    pool: Arc<Pool>,
}

impl PrelabelRunner for ProcessRunner {
    fn infer(
        &self,
        owner: InferenceOwner,
        model: Vec<u8>,
        image: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture {
        let timeout = self.timeout;
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + timeout;
            let mut lease =
                tokio::time::timeout_at(deadline, pool.acquire(Owner::Inference(owner)))
                    .await
                    .map_err(|_| PrelabelFailure::Busy)??;
            let digest = labello_inference::model_digest(&model);
            let preferred = lease.worker.as_ref().map(|worker| worker.provider);
            let result = tokio::time::timeout_at(deadline, async {
                for provider in provider_order(preferred) {
                    let budget = if provider == NativeProvider::Cpu { timeout } else { timeout / 4 };
                    let attempt = async {
                        if lease.worker.as_ref().is_some_and(|worker| worker.provider != provider)
                            && let Some(worker) = lease.worker.take()
                        {
                            worker.stop().await;
                        }
                        if lease.worker.is_none() {
                            lease.worker = Some(process::Worker::spawn(provider, &runtime, pool.budget.clone(), pool.config.threads_per_worker).await?);
                        }
                        lease.worker.as_mut().ok_or(PrelabelFailure::Inference)?
                            .infer(&model, &digest, &image, &config, &task).await
                    };
                    match tokio::time::timeout(budget, attempt).await {
                        Ok(Ok(result)) => return Ok(result),
                        failure => tracing::warn!(event = "prelabel_provider_fallback", provider = ?provider,
                            reason = if failure.is_err() { "timeout" } else { "worker_failed" }),
                    }
                }
                Err(PrelabelFailure::Inference)
            }).await.map_err(|_| PrelabelFailure::Inference)?;
            if result.is_ok() {
                lease.keep();
            }
            result
        })
    }

    fn shutdown(&self) {
        self.pool.shutdown();
    }

    fn release(&self, owner: &InferenceOwner) {
        self.pool.release(&Owner::Inference(owner.clone()));
    }

    fn inspect(&self, model: Vec<u8>) -> labello_storage::prelabel::ModelInspectionFuture {
        let timeout = self.timeout;
        let runtime = self.runtime.clone();
        let pool = self.pool.clone();
        Box::pin(async move {
            let deadline = tokio::time::Instant::now() + timeout;
            let _lease = tokio::time::timeout_at(deadline, pool.acquire(pool.inspection_owner()))
                .await
                .map_err(|_| PrelabelFailure::Busy)??;
            tokio::time::timeout_at(
                deadline,
                run_inspection(&runtime, &model, pool.budget.clone()),
            )
            .await
            .map_err(|_| PrelabelFailure::Inference)?
        })
    }
}

fn provider_order(preferred: Option<NativeProvider>) -> impl Iterator<Item = NativeProvider> {
    preferred.into_iter().chain(
        NativeProvider::PREFERENCE
            .into_iter()
            .filter(move |p| Some(*p) != preferred),
    )
}
