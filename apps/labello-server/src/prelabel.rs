use labello_domain::{PrelabelConfig, TaskDefinition};
use labello_storage::prelabel::{
    InferenceFuture, PrelabelFailure, PrelabelLimits, PrelabelRunner, PrelabelService,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    Ok(PrelabelService::new(
        root,
        &config.models_root,
        config.limits,
        Arc::new(ProcessRunner {
            timeout: Duration::from_secs(config.timeout_seconds),
            runtime: config.runtime,
        }),
    )
    .await?)
}

struct ProcessRunner {
    timeout: Duration,
    runtime: labello_inference::NativeRuntimeConfig,
}

struct WorkerProcess(Option<tokio::process::Child>);
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.start_kill();
            // Tokio's best-effort orphan reaping can leave a zombie until another spawn.
            // Own the wait after cancellation so repeated timeouts cannot accumulate workers.
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
        }
    }
}
impl PrelabelRunner for ProcessRunner {
    fn infer(
        &self,
        model: Vec<u8>,
        image: Vec<u8>,
        config: PrelabelConfig,
        task: TaskDefinition,
    ) -> InferenceFuture {
        let timeout = self.timeout;
        let runtime = self.runtime.clone();
        Box::pin(infer_with_fallback(timeout, move |provider| {
            let future = run_worker(
                timeout,
                runtime.clone(),
                WorkerOperation::Infer {
                    config: Box::new(config.clone()),
                    task: Box::new(task.clone()),
                    provider,
                },
                model.clone(),
                image.clone(),
            );
            async move {
                serde_json::from_slice::<labello_inference::InferenceResult>(&future.await?)
                    .map_err(|_| PrelabelFailure::Inference)
            }
        }))
    }

    fn inspect(&self, model: Vec<u8>) -> labello_storage::prelabel::ModelInspectionFuture {
        let future = run_worker(
            self.timeout,
            self.runtime.clone(),
            WorkerOperation::Inspect,
            model,
            vec![],
        );
        Box::pin(async move {
            serde_json::from_slice::<Result<labello_domain::PrelabelModelInspection, String>>(
                &future.await?,
            )
            .map_err(|_| PrelabelFailure::Inference)?
            .map_err(|_| PrelabelFailure::ModelInvalid)
        })
    }
}

async fn infer_with_fallback<F, Fut>(
    timeout: Duration,
    mut attempt: F,
) -> Result<labello_domain::PrelabelInferenceResult, PrelabelFailure>
where
    F: FnMut(labello_inference::NativeProvider) -> Fut,
    Fut: std::future::Future<
            Output = Result<labello_domain::PrelabelInferenceResult, PrelabelFailure>,
        >,
{
    tokio::time::timeout(timeout, async {
        for provider in labello_inference::NativeProvider::PREFERENCE {
            let budget = if provider == labello_inference::NativeProvider::Cpu {
                timeout
            } else {
                timeout / 4
            };
            if let Ok(Ok(result)) = tokio::time::timeout(budget, attempt(provider)).await {
                return Ok(result);
            }
        }
        Err(PrelabelFailure::Inference)
    })
    .await
    .map_err(|_| PrelabelFailure::Inference)?
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum WorkerOperation {
    Infer {
        config: Box<PrelabelConfig>,
        task: Box<TaskDefinition>,
        provider: labello_inference::NativeProvider,
    },
    Inspect,
}

async fn run_worker(
    timeout: Duration,
    runtime: labello_inference::NativeRuntimeConfig,
    operation: WorkerOperation,
    model: Vec<u8>,
    image: Vec<u8>,
) -> Result<Vec<u8>, PrelabelFailure> {
    let execute = async move {
        let execute = async {
            let header = serde_json::to_vec(&(&runtime, &operation))
                .map_err(|_| PrelabelFailure::Invalid)?;
            if header.len() > 1024 * 1024
                || model.len() > labello_inference::MAX_MODEL_BYTES
                || image.len() > labello_inference::MAX_IMAGE_BYTES
            {
                return Err(PrelabelFailure::Limit);
            }
            let executable = std::env::current_exe().map_err(|_| PrelabelFailure::Inference)?;
            let mut command = tokio::process::Command::new(executable);
            command.arg("--prelabel-worker").env_clear();
            if let Some(parent) = runtime.onnx_library.as_ref().and_then(|path| path.parent()) {
                command.env("LD_LIBRARY_PATH", parent);
            }
            let child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| PrelabelFailure::Inference)?;
            let mut worker = WorkerProcess(Some(child));
            let child = worker.0.as_mut().ok_or(PrelabelFailure::Inference)?;
            let mut input = child.stdin.take().ok_or(PrelabelFailure::Inference)?;
            let output = child.stdout.take().ok_or(PrelabelFailure::Inference)?;
            let write = async move {
                for bytes in [&header, &model, &image] {
                    input
                        .write_u64_le(bytes.len() as u64)
                        .await
                        .map_err(|_| PrelabelFailure::Inference)?;
                    input
                        .write_all(bytes)
                        .await
                        .map_err(|_| PrelabelFailure::Inference)?;
                }
                input
                    .shutdown()
                    .await
                    .map_err(|_| PrelabelFailure::Inference)
            };
            let read = async move {
                let mut bytes = Vec::new();
                output
                    .take(32 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| PrelabelFailure::Inference)?;
                if bytes.len() > 32 * 1024 * 1024 {
                    return Err(PrelabelFailure::Limit);
                }
                Ok(bytes)
            };
            let (_, bytes) = tokio::try_join!(write, read)?;
            if !child
                .wait()
                .await
                .map_err(|_| PrelabelFailure::Inference)?
                .success()
            {
                return Err(PrelabelFailure::Inference);
            }
            worker.0.take();
            Ok(bytes)
        };
        tokio::time::timeout(timeout, execute)
            .await
            .map_err(|_| PrelabelFailure::Inference)?
    };
    execute.await
}

/// Fixed protocol with operator-owned runtime libraries. No model paths, shell commands,
/// inherited credentials, or network fetches.
pub fn worker() -> Result<(), ()> {
    let mut input = std::io::stdin().lock();
    let mut read = |limit: usize| {
        let mut length = [0; 8];
        input.read_exact(&mut length).map_err(|_| ())?;
        let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| ())?;
        if length > limit {
            return Err(());
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).map_err(|_| ())?;
        Ok(bytes)
    };
    let (runtime, operation): (labello_inference::NativeRuntimeConfig, WorkerOperation) =
        serde_json::from_slice(&read(1024 * 1024)?).map_err(|_| ())?;
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Resource, Rlimit, setrlimit};
        setrlimit(
            if matches!(
                operation,
                WorkerOperation::Infer {
                    provider: labello_inference::NativeProvider::Cuda
                        | labello_inference::NativeProvider::WebGpu,
                    ..
                }
            ) {
                Resource::Data
            } else {
                Resource::As
            },
            Rlimit {
                current: Some(4 * 1024 * 1024 * 1024),
                maximum: Some(4 * 1024 * 1024 * 1024),
            },
        )
        .map_err(|_| ())?;
        setrlimit(
            Resource::Cpu,
            Rlimit {
                current: Some(300),
                maximum: Some(300),
            },
        )
        .map_err(|_| ())?;
        setrlimit(
            Resource::Core,
            Rlimit {
                current: Some(0),
                maximum: Some(0),
            },
        )
        .map_err(|_| ())?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        return Err(());
    }
    let model = read(labello_inference::MAX_MODEL_BYTES)?;
    let image = read(labello_inference::MAX_IMAGE_BYTES)?;
    labello_inference::configure_runtime(&runtime);
    let bytes = match operation {
        WorkerOperation::Infer {
            config,
            task,
            provider,
        } => {
            let result =
                labello_inference::infer_with_provider(&model, &image, &config, &task, provider)
                    .map_err(|_| ())?;
            serde_json::to_vec(&result).map_err(|_| ())?
        }
        WorkerOperation::Inspect => {
            serde_json::to_vec(&labello_inference::inspect(&model)).map_err(|_| ())?
        }
    };
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(());
    }
    std::io::stdout().lock().write_all(&bytes).map_err(|_| ())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_failures_and_timeouts_fall_back_in_order_with_one_total_budget() {
        use labello_inference::NativeProvider;
        for success in [
            NativeProvider::Cuda,
            NativeProvider::WebGpu,
            NativeProvider::Cpu,
        ] {
            let calls = std::sync::Mutex::new(Vec::new());
            let result = infer_with_fallback(Duration::from_millis(200), |provider| {
                calls.lock().unwrap().push(provider);
                async move {
                    if provider == success {
                        Ok(labello_domain::PrelabelInferenceResult {
                            execution: provider.execution(),
                            suggestions: vec![],
                        })
                    } else {
                        if provider == NativeProvider::Cuda {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                        Err(PrelabelFailure::Inference)
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(result.execution, success.execution());
            let expected = NativeProvider::PREFERENCE
                .into_iter()
                .take_while(|provider| *provider != success)
                .chain([success])
                .collect::<Vec<_>>();
            assert_eq!(*calls.lock().unwrap(), expected);
        }
        let started = std::time::Instant::now();
        let failed = infer_with_fallback(Duration::from_millis(80), |_| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Err(PrelabelFailure::Inference)
        })
        .await;
        assert!(failed.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn cancelled_worker_is_killed_and_reaped() {
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .env_clear()
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let process = PathBuf::from(format!("/proc/{}", child.id().unwrap()));
        assert!(process.exists());
        drop(WorkerProcess(Some(child)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while process.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled worker was not reaped");
    }
}
