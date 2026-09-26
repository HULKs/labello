//! Derived hints are private, disposable data. This service never writes workflow events.
use crate::{
    DatasetRepository,
    fsjson::{read_json, write_json_atomic},
};
use labello_domain::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard, Semaphore, watch};

mod files;
mod predictions;
mod runs;
#[cfg(test)]
mod tests;
pub use predictions::AcceptanceGuard;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PrelabelFailure {
    #[error("invalid prelabel configuration or model contract")]
    Invalid,
    #[error("prelabel data is unavailable")]
    Storage,
    #[error("prelabel resource limit reached")]
    Limit,
    #[error("prelabel workers are busy; retry later")]
    Busy,
    #[error("prelabel generation was reset or its model/workflow changed; refresh hints")]
    Stale,
    #[error("prelabel generation is paused; a dataset administrator can resume it")]
    Paused,
    #[error("model execution failed or exceeded its resource limit")]
    Inference,
    #[error("model file is missing, unreadable, or outside the configured models directory")]
    ModelUnavailable,
    #[error("file is not a supported, self-contained ONNX model")]
    ModelInvalid,
    #[error("prelabel run is not ready for this action; check remaining workflows again")]
    NotReady,
    #[error("prelabel run was not found")]
    NotFound,
}
type Result<T> = std::result::Result<T, PrelabelFailure>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PrelabelLimits {
    pub max_concurrent_inferences: usize,
    pub max_work_items: usize,
    pub max_retained_runs: usize,
    pub max_retained_results: usize,
    pub max_result_bytes: usize,
    pub max_total_result_bytes: u64,
    pub retention_seconds: u64,
}
impl Default for PrelabelLimits {
    fn default() -> Self {
        Self {
            max_concurrent_inferences: 1,
            max_work_items: 100_000,
            max_retained_runs: 16,
            max_retained_results: 100_000,
            max_result_bytes: 8 * 1024 * 1024,
            max_total_result_bytes: 2 * 1024 * 1024 * 1024,
            retention_seconds: 7 * 24 * 3600,
        }
    }
}
impl PrelabelLimits {
    fn validate(&self) -> Result<()> {
        if self.max_concurrent_inferences == 0
            || self.max_concurrent_inferences > 8
            || self.max_work_items == 0
            || self.max_work_items > 1_000_000
            || self.max_retained_runs == 0
            || self.max_retained_runs > 64
            || self.max_retained_results == 0
            || self.max_retained_results > 1_000_000
            || self.max_result_bytes == 0
            || self.max_result_bytes > 32 * 1024 * 1024
            || self.max_total_result_bytes < self.max_result_bytes as u64
            || self.retention_seconds == 0
            || self.retention_seconds > 90 * 24 * 3600
        {
            return Err(PrelabelFailure::Invalid);
        }
        Ok(())
    }
}

pub type InferenceFuture = Pin<Box<dyn Future<Output = Result<PrelabelInferenceResult>> + Send>>;
pub type ModelInspectionFuture =
    Pin<Box<dyn Future<Output = Result<PrelabelModelInspection>> + Send>>;
pub trait PrelabelRunner: Send + Sync {
    fn inspect(&self, model: Vec<u8>) -> ModelInspectionFuture;
    /// Implementations must bound execution time/memory and terminate on future cancellation.
    fn infer(
        &self,
        model: Vec<u8>,
        image: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture;
}

#[derive(Clone)]
pub struct PrelabelService {
    inner: Arc<Inner>,
}
struct Inner {
    _root: std::fs::File,
    path: PathBuf,
    models: std::fs::File,
    limits: PrelabelLimits,
    runner: Arc<dyn PrelabelRunner>,
    workers: Arc<Semaphore>,
    datasets: Mutex<BTreeMap<DatasetId, Arc<Mutex<Control>>>>,
    running: Mutex<BTreeMap<String, watch::Sender<bool>>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Control {
    version: u32,
    key: [u8; 32],
    generation: u64,
    scopes: Vec<ScopeControl>,
    runs: Vec<Run>,
    results: BTreeMap<String, CachedResult>,
}
impl Default for Control {
    fn default() -> Self {
        let mut key = [0; 32];
        key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self {
            version: 1,
            key,
            generation: 0,
            scopes: Vec::new(),
            runs: Vec::new(),
            results: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct ScopeControl {
    task_id: TaskId,
    config_id: PrelabelConfigId,
    generation: u64,
    paused: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct CachedResult {
    #[serde(default = "server_cpu")]
    execution: PrelabelExecutionKind,
    task_id: TaskId,
    config_id: PrelabelConfigId,
    created_at: Timestamp,
    bytes: u64,
    empty: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Run {
    summary: PrelabelRunSummary,
    items: Vec<WorkItem>,
}
#[derive(Clone, Serialize, Deserialize)]
struct WorkItem {
    image_id: ImageId,
    image_hash: String,
    task_id: TaskId,
    config_id: PrelabelConfigId,
    config_digest: String,
    model_digest: String,
    generation: PrelabelGeneration,
    outcome: PrelabelItemOutcome,
}

impl PrelabelService {
    pub async fn new(
        root: &Path,
        models: &Path,
        limits: PrelabelLimits,
        runner: Arc<dyn PrelabelRunner>,
    ) -> Result<Self> {
        limits.validate()?;
        let (root, path) = files::private_root(root)?;
        let models = std::fs::File::open(models).map_err(|_| PrelabelFailure::Invalid)?;
        if !models
            .metadata()
            .map_err(|_| PrelabelFailure::Storage)?
            .is_dir()
        {
            return Err(PrelabelFailure::Invalid);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                _root: root,
                path,
                models,
                workers: Arc::new(Semaphore::new(limits.max_concurrent_inferences)),
                limits,
                runner,
                datasets: Mutex::new(BTreeMap::new()),
                running: Mutex::new(BTreeMap::new()),
            }),
        })
    }

    fn directory(&self, dataset: &DatasetId) -> Result<PathBuf> {
        dataset
            .validate_path_segment()
            .map_err(|_| PrelabelFailure::Invalid)?;
        let directory = self.inner.path.join(dataset.as_str());
        files::ensure_private_directory(&directory)?;
        Ok(directory)
    }

    async fn lock(&self, dataset: &DatasetId) -> Result<OwnedMutexGuard<Control>> {
        let mut datasets = self.inner.datasets.lock().await;
        if !datasets.contains_key(dataset) {
            let directory = self.directory(dataset)?;
            let path = directory.join("control.json");
            let mut control = if path.try_exists().map_err(|_| PrelabelFailure::Storage)? {
                read_json::<Control>(&path)
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?
            } else {
                Control::default()
            };
            if control.version != 1 {
                return Err(PrelabelFailure::Storage);
            }
            for run in &mut control.runs {
                if run.summary.phase == PrelabelRunPhase::Running {
                    run.summary.phase = PrelabelRunPhase::Interrupted;
                    run.summary.updated_at = now();
                }
            }
            self.persist(dataset, &control).await?;
            self.cleanup_files(dataset, &control).await?;
            datasets.insert(dataset.clone(), Arc::new(Mutex::new(control)));
        }
        let lock = Arc::clone(&datasets[dataset]);
        drop(datasets);
        Ok(lock.lock_owned().await)
    }

    async fn persist(&self, dataset: &DatasetId, control: &Control) -> Result<()> {
        write_json_atomic(&self.directory(dataset)?.join("control.json"), control)
            .await
            .map_err(|_| PrelabelFailure::Storage)
    }

    async fn commit(&self, dataset: &DatasetId, guard: &mut Control, next: Control) -> Result<()> {
        // Publish durable control first; a failed write must never appear successful in memory.
        self.persist(dataset, &next).await?;
        *guard = next;
        Ok(())
    }

    fn generation(
        control: &Control,
        task: &TaskId,
        config: &PrelabelConfigId,
    ) -> PrelabelGeneration {
        let scope = control
            .scopes
            .iter()
            .find(|s| &s.task_id == task && &s.config_id == config);
        PrelabelGeneration {
            generation: control.generation,
            scope_generation: scope.map_or(0, |s| s.generation),
            paused: scope.is_some_and(|s| s.paused),
        }
    }

    pub async fn model(&self, config: &PrelabelConfig) -> Result<Vec<u8>> {
        config.validate().map_err(|_| PrelabelFailure::Invalid)?;
        let model = self.model_file(&config.model.location)?;
        if config
            .yolo
            .as_ref()
            .and_then(|spec| spec.model_digest.as_ref())
            .is_some_and(|digest| blake3::hash(&model).to_hex().as_str() != digest)
        {
            return Err(PrelabelFailure::Stale);
        }
        Ok(model)
    }

    fn model_file(&self, location: &str) -> Result<Vec<u8>> {
        ModelSpec::validate_location(location).map_err(|_| PrelabelFailure::Invalid)?;
        files::read_beneath(&self.inner.models, Path::new(location), 256 * 1024 * 1024).map_err(
            |error| match error {
                PrelabelFailure::Limit => error,
                _ => PrelabelFailure::ModelUnavailable,
            },
        )
    }

    /// An unsaved filename can be checked before a class mapping or configuration exists.
    pub async fn inspect_model(&self, location: &str) -> Result<PrelabelModelInspection> {
        let _permit = self
            .inner
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PrelabelFailure::Busy)?;
        self.inner.runner.inspect(self.model_file(location)?).await
    }

    pub async fn shutdown(&self) {
        for cancel in self.inner.running.lock().await.values() {
            let _ = cancel.send(true);
        }
    }

    async fn cleanup_files(&self, dataset: &DatasetId, control: &Control) -> Result<()> {
        let directory = self.directory(dataset)?;
        let mut entries = tokio::fs::read_dir(&directory)
            .await
            .map_err(|_| PrelabelFailure::Storage)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| PrelabelFailure::Storage)?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("result-")
                && !control
                    .results
                    .keys()
                    .any(|key| name == format!("result-{key}.json"))
            {
                tokio::fs::remove_file(entry.path())
                    .await
                    .map_err(|_| PrelabelFailure::Storage)?;
            }
        }
        Ok(())
    }

    async fn prune(&self, dataset: &DatasetId, control: &mut Control) -> Result<()> {
        let cutoff = now() - std::time::Duration::from_secs(self.inner.limits.retention_seconds);
        if !control
            .results
            .values()
            .any(|result| result.created_at < cutoff)
            && !control.runs.iter().any(|run| {
                run.summary.updated_at < cutoff && run.summary.phase != PrelabelRunPhase::Running
            })
        {
            return Ok(());
        }
        let mut next = control.clone();
        next.results.retain(|_, result| result.created_at >= cutoff);
        next.runs.retain(|run| {
            run.summary.updated_at >= cutoff || run.summary.phase == PrelabelRunPhase::Running
        });
        self.commit(dataset, control, next).await?;
        self.cleanup_files(dataset, control).await
    }
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(
        blake3::hash(&serde_json::to_vec(value).map_err(|_| PrelabelFailure::Invalid)?)
            .to_hex()
            .to_string(),
    )
}
fn configuration<'a>(
    metadata: &'a DatasetMetadata,
    task_id: &TaskId,
    config_id: &PrelabelConfigId,
) -> Result<(&'a TaskDefinition, &'a PrelabelConfig, String)> {
    let task = metadata.task(task_id).ok_or(PrelabelFailure::Invalid)?;
    let config = metadata
        .prelabel_configs
        .iter()
        .find(|c| &c.config_id == config_id)
        .ok_or(PrelabelFailure::Invalid)?;
    config
        .validate_for_task(task)
        .map_err(|_| PrelabelFailure::Invalid)?;
    Ok((task, config, digest(&(task, config))?))
}

fn server_cpu() -> PrelabelExecutionKind {
    PrelabelExecutionKind::ServerCpu
}
