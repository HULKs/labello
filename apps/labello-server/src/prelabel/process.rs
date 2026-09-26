use super::*;
use std::{collections::VecDeque, process::Stdio};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout},
    sync::{OwnedSemaphorePermit, Semaphore},
};

pub(super) const MAX_REPLY: usize = 32 * 1024 * 1024;
pub(super) const SESSION_LIMIT: usize = 2;

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(super) enum WorkerOperation {
    Infer {
        config: Box<PrelabelConfig>,
        task: Box<TaskDefinition>,
        provider: NativeProvider,
        digest: String,
        threads: usize,
    },
    Inspect,
}

#[derive(Serialize, Deserialize)]
pub(super) struct WorkerReply {
    pub result: Result<labello_inference::InferenceResult, WorkerFailure>,
    pub loaded: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WorkerFailure {
    Model,
    Execution,
}

pub(super) struct WorkerProcess {
    pub child: Option<Child>,
    pub permit: Option<OwnedSemaphorePermit>,
}
impl WorkerProcess {
    async fn stop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.child.take();
        self.permit.take();
    }
}
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let permit = self.permit.take();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = child.wait().await;
                    drop(permit);
                });
            }
        }
    }
}

pub(super) struct Worker {
    process: WorkerProcess,
    input: ChildStdin,
    output: ChildStdout,
    pub provider: NativeProvider,
    models: VecDeque<String>,
    runtime: labello_inference::NativeRuntimeConfig,
    threads: usize,
}
impl Worker {
    pub async fn spawn(
        provider: NativeProvider,
        runtime: &labello_inference::NativeRuntimeConfig,
        budget: Arc<Semaphore>,
        threads: usize,
    ) -> Result<Self, PrelabelFailure> {
        let permit = budget
            .acquire_owned()
            .await
            .map_err(|_| PrelabelFailure::Busy)?;
        let executable = std::env::current_exe().map_err(|_| PrelabelFailure::Inference)?;
        let mut command = tokio::process::Command::new(executable);
        command.arg("--prelabel-worker").env_clear();
        if let Some(parent) = runtime.onnx_library.as_ref().and_then(|path| path.parent()) {
            command.env("LD_LIBRARY_PATH", parent);
        }
        command
            .env("OMP_NUM_THREADS", threads.to_string())
            .env("OPENBLAS_NUM_THREADS", threads.to_string());
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| PrelabelFailure::Inference)?;
        let input = child.stdin.take().ok_or(PrelabelFailure::Inference)?;
        let output = child.stdout.take().ok_or(PrelabelFailure::Inference)?;
        tracing::debug!(event = "prelabel_worker_started", provider = ?provider);
        Ok(Self {
            process: WorkerProcess {
                child: Some(child),
                permit: Some(permit),
            },
            input,
            output,
            provider,
            models: VecDeque::new(),
            runtime: runtime.clone(),
            threads,
        })
    }

    async fn write(
        &mut self,
        operation: WorkerOperation,
        model: &[u8],
        image: &[u8],
    ) -> Result<(), PrelabelFailure> {
        let header = serde_json::to_vec(&(&self.runtime, operation))
            .map_err(|_| PrelabelFailure::Invalid)?;
        if header.len() > 1024 * 1024
            || model.len() > labello_inference::MAX_MODEL_BYTES
            || image.len() > labello_inference::MAX_IMAGE_BYTES
        {
            return Err(PrelabelFailure::Limit);
        }
        for bytes in [&header[..], model, image] {
            self.input
                .write_u64_le(bytes.len() as u64)
                .await
                .map_err(|_| PrelabelFailure::Inference)?;
            self.input
                .write_all(bytes)
                .await
                .map_err(|_| PrelabelFailure::Inference)?;
        }
        self.input
            .flush()
            .await
            .map_err(|_| PrelabelFailure::Inference)
    }

    pub async fn infer(
        &mut self,
        model: &[u8],
        digest: &str,
        image: &[u8],
        config: &PrelabelConfig,
        task: &TaskDefinition,
    ) -> Result<labello_inference::InferenceResult, PrelabelFailure> {
        let cached = self.models.iter().any(|key| key == digest);
        let operation = WorkerOperation::Infer {
            config: Box::new(config.clone()),
            task: Box::new(task.clone()),
            provider: self.provider,
            digest: digest.to_owned(),
            threads: self.threads,
        };
        self.write(operation, if cached { &[] } else { model }, image)
            .await?;
        let length = self
            .output
            .read_u64_le()
            .await
            .map_err(|_| PrelabelFailure::Inference)?;
        if length > MAX_REPLY as u64 {
            return Err(PrelabelFailure::Limit);
        }
        let mut bytes = vec![0; length as usize];
        self.output
            .read_exact(&mut bytes)
            .await
            .map_err(|_| PrelabelFailure::Inference)?;
        let reply: WorkerReply =
            serde_json::from_slice(&bytes).map_err(|_| PrelabelFailure::Inference)?;
        let result = reply.result.map_err(|failure| {
            tracing::warn!(event = "prelabel_worker_failed", provider = ?self.provider, phase = ?failure);
            PrelabelFailure::Inference
        })?;
        if result.execution != self.provider.execution() {
            return Err(PrelabelFailure::Inference);
        }
        self.models.retain(|key| key != digest);
        self.models.push_back(digest.to_owned());
        while self.models.len() > SESSION_LIMIT {
            self.models.pop_front();
        }
        tracing::debug!(event = "prelabel_inference_completed", provider = ?self.provider, session_loaded = reply.loaded);
        Ok(result)
    }

    pub async fn stop(mut self) {
        self.process.stop().await;
    }
}

pub(super) async fn run_inspection(
    runtime: &labello_inference::NativeRuntimeConfig,
    model: &[u8],
    budget: Arc<Semaphore>,
) -> Result<labello_domain::PrelabelModelInspection, PrelabelFailure> {
    let mut worker = Worker::spawn(NativeProvider::Cpu, runtime, budget, 1).await?;
    worker.write(WorkerOperation::Inspect, model, &[]).await?;
    worker
        .input
        .shutdown()
        .await
        .map_err(|_| PrelabelFailure::Inference)?;
    let mut bytes = Vec::new();
    (&mut worker.output)
        .take(MAX_REPLY as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| PrelabelFailure::Inference)?;
    if bytes.len() > MAX_REPLY {
        return Err(PrelabelFailure::Limit);
    }
    let child = worker
        .process
        .child
        .as_mut()
        .ok_or(PrelabelFailure::Inference)?;
    if !child
        .wait()
        .await
        .map_err(|_| PrelabelFailure::Inference)?
        .success()
    {
        return Err(PrelabelFailure::Inference);
    }
    worker.process.child.take();
    serde_json::from_slice::<Result<labello_domain::PrelabelModelInspection, String>>(&bytes)
        .map_err(|_| PrelabelFailure::Inference)?
        .map_err(|_| PrelabelFailure::ModelInvalid)
}
