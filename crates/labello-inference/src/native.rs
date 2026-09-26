use super::*;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::OnceLock};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeRuntimeConfig {
    /// Operator-controlled library paths, never dataset configuration or request input.
    pub onnx_library: Option<PathBuf>,
    pub webgpu_library: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProvider {
    Cuda,
    WebGpu,
    Cpu,
}

impl NativeProvider {
    pub const PREFERENCE: [Self; 3] = [Self::Cuda, Self::WebGpu, Self::Cpu];

    pub fn execution(self) -> PrelabelExecutionKind {
        match self {
            Self::Cuda => PrelabelExecutionKind::ServerCuda,
            Self::WebGpu => PrelabelExecutionKind::ServerWebGpu,
            Self::Cpu => PrelabelExecutionKind::ServerCpu,
        }
    }
}

struct Backend {
    native: bool,
    webgpu_library: Option<PathBuf>,
}
static BACKEND: OnceLock<Backend> = OnceLock::new();

pub fn configure_runtime(config: &NativeRuntimeConfig) {
    BACKEND.get_or_init(|| {
        let library = config
            .onnx_library
            .clone()
            .unwrap_or_else(|| "libonnxruntime.so".into());
        let native = ort::init_from(library).is_ok_and(|environment| environment.commit());
        Backend {
            native,
            webgpu_library: config.webgpu_library.clone(),
        }
    });
}

pub(super) fn available() -> bool {
    configure_runtime(&NativeRuntimeConfig::default());
    BACKEND.get().expect("configured runtime").native
}

pub(super) struct NativeSession {
    // Fields drop in declaration order: release the session before its provider library.
    pub session: Session,
    _provider: Option<RegisteredProvider>,
}

struct RegisteredProvider(Option<ort::ep::ExecutionProviderLibrary>);

impl Drop for RegisteredProvider {
    fn drop(&mut self) {
        if let Some(library) = self.0.take() {
            // Release plugin resources before the process runs native C++ destructors.
            let _ = library.unregister();
        }
    }
}

pub(super) fn load_session(
    model: &[u8],
    provider: NativeProvider,
    threads: usize,
) -> Result<NativeSession, String> {
    configure_runtime(&NativeRuntimeConfig::default());
    let backend = BACKEND.get().expect("configured runtime");
    if !backend.native {
        return Err("GPU execution provider unavailable".into());
    }
    let mut registered_provider = None;
    let mut builder = Session::builder()
        .map_err(|_| "model runtime initialization failed")?
        .with_intra_threads(threads)
        .map_err(|_| "model thread configuration failed")?
        .with_inter_threads(1)
        .map_err(|_| "model thread configuration failed")?;
    builder = match provider {
        NativeProvider::Cpu => builder,
        NativeProvider::Cuda => builder
            .with_execution_providers([ort::ep::CUDA::default()
                .with_memory_limit(1024 * 1024 * 1024)
                .build()
                .error_on_failure()])
            .map_err(|_| "CUDA execution provider unavailable")?,
        NativeProvider::WebGpu => {
            if let Some(library) = &backend.webgpu_library {
                let environment = ort::environment::Environment::current()
                    .map_err(|_| "WebGPU environment unavailable")?;
                registered_provider = Some(RegisteredProvider(Some(
                    environment
                        .register_ep_library("labello_webgpu", library)
                        .map_err(|_| "WebGPU execution provider unavailable")?,
                )));
                let devices = environment
                    .devices()
                    .filter(|device| {
                        device
                            .ep()
                            .is_ok_and(|name| name == "WebGpuExecutionProvider")
                    })
                    .take(1)
                    .collect::<Vec<_>>();
                if devices.is_empty() {
                    return Err("WebGPU device unavailable".into());
                }
                builder
                    .with_devices(devices, None)
                    .map_err(|_| "WebGPU execution provider unavailable")?
            } else {
                builder
                    .with_execution_providers([ort::ep::WebGPU::default()
                        .build()
                        .error_on_failure()])
                    .map_err(|_| "WebGPU execution provider unavailable")?
            }
        }
    };
    let session = builder
        .commit_from_memory(model)
        .map_err(|_| "model loading failed")?;
    Ok(NativeSession {
        session,
        _provider: registered_provider,
    })
}
