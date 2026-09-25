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
    Ok(PrelabelService::new(
        root,
        &config.models_root,
        config.limits,
        Arc::new(ProcessRunner {
            timeout: Duration::from_secs(config.timeout_seconds),
        }),
    )
    .await?)
}

struct ProcessRunner {
    timeout: Duration,
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
        Box::pin(async move {
            let execute = async {
                let header =
                    serde_json::to_vec(&(config, task)).map_err(|_| PrelabelFailure::Invalid)?;
                if header.len() > 1024 * 1024
                    || model.len() > labello_inference::MAX_MODEL_BYTES
                    || image.len() > labello_inference::MAX_IMAGE_BYTES
                {
                    return Err(PrelabelFailure::Limit);
                }
                let executable = std::env::current_exe().map_err(|_| PrelabelFailure::Inference)?;
                let child = tokio::process::Command::new(executable)
                    .arg("--prelabel-worker")
                    .env_clear()
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
                serde_json::from_slice::<labello_inference::InferenceResult>(&bytes)
                    .map(|result| result.suggestions)
                    .map_err(|_| PrelabelFailure::Inference)
            };
            tokio::time::timeout(timeout, execute)
                .await
                .map_err(|_| PrelabelFailure::Inference)?
        })
    }
}

/// Fixed protocol only. No paths, shell commands, environment credentials or network fetches.
pub fn worker() -> Result<(), ()> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Resource, Rlimit, setrlimit};
        setrlimit(
            Resource::As,
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
    let mut input = std::io::stdin().lock();
    let mut read = |limit: usize| {
        let mut length = [0; 8];
        input.read_exact(&mut length).map_err(|_| ())?;
        let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| ())?;
        if length == 0 || length > limit {
            return Err(());
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).map_err(|_| ())?;
        Ok(bytes)
    };
    let (config, task): (PrelabelConfig, TaskDefinition) =
        serde_json::from_slice(&read(1024 * 1024)?).map_err(|_| ())?;
    let model = read(labello_inference::MAX_MODEL_BYTES)?;
    let image = read(labello_inference::MAX_IMAGE_BYTES)?;
    let result = labello_inference::infer(&model, &image, &config, &task).map_err(|_| ())?;
    let bytes = serde_json::to_vec(&result).map_err(|_| ())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(());
    }
    std::io::stdout().lock().write_all(&bytes).map_err(|_| ())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

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
