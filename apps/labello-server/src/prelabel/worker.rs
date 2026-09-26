use super::{
    WorkerOperation,
    process::{MAX_REPLY, SESSION_LIMIT, WorkerFailure, WorkerReply},
};
use labello_inference::{InferenceSession, NativeProvider, NativeRuntimeConfig};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
};

fn read_frame(input: &mut impl Read, limit: usize) -> Result<Vec<u8>, ()> {
    let mut length = [0; 8];
    input.read_exact(&mut length).map_err(|_| ())?;
    let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| ())?;
    if length > limit {
        return Err(());
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes).map_err(|_| ())?;
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn limits(provider: NativeProvider) -> Result<(), ()> {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let memory = if provider == NativeProvider::Cpu {
        Resource::As
    } else {
        Resource::Data
    };
    setrlimit(
        memory,
        Rlimit {
            current: Some(4 * 1024 * 1024 * 1024),
            maximum: Some(4 * 1024 * 1024 * 1024),
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
    // CPU time is cumulative for the process. Renew a per-request soft limit;
    // the parent independently enforces the shorter wall-clock deadline.
    let used = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime).tv_sec;
    setrlimit(
        Resource::Cpu,
        Rlimit {
            current: Some(used as u64 + 300),
            maximum: None,
        },
    )
    .map_err(|_| ())
}
#[cfg(not(target_os = "linux"))]
fn limits(_: NativeProvider) -> Result<(), ()> {
    Err(())
}

/// Operator-owned protocol. Model bytes are sent only on a cache miss.
pub fn worker() -> Result<(), ()> {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    let mut identity: Option<(NativeRuntimeConfig, NativeProvider, usize)> = None;
    let mut sessions: VecDeque<(String, InferenceSession)> = VecDeque::new();
    loop {
        if input.fill_buf().map_err(|_| ())?.is_empty() {
            return Ok(());
        }
        let header = read_frame(&mut input, 1024 * 1024)?;
        let (runtime, operation): (NativeRuntimeConfig, WorkerOperation) =
            serde_json::from_slice(&header).map_err(|_| ())?;
        let (provider, threads) = match &operation {
            WorkerOperation::Infer {
                provider, threads, ..
            } => (*provider, *threads),
            WorkerOperation::Inspect => (NativeProvider::Cpu, 1),
        };
        if !(1..=16).contains(&threads) {
            return Err(());
        }
        if let Some(previous) = &identity {
            if previous != &(runtime.clone(), provider, threads) {
                return Err(());
            }
        } else {
            identity = Some((runtime.clone(), provider, threads));
        }
        limits(provider)?;
        let model = read_frame(&mut input, labello_inference::MAX_MODEL_BYTES)?;
        let image = read_frame(&mut input, labello_inference::MAX_IMAGE_BYTES)?;
        labello_inference::configure_runtime(&runtime);
        let WorkerOperation::Infer {
            config,
            task,
            digest,
            ..
        } = operation
        else {
            let bytes = serde_json::to_vec(&labello_inference::inspect(&model)).map_err(|_| ())?;
            if bytes.len() > MAX_REPLY {
                return Err(());
            }
            return output.write_all(&bytes).map_err(|_| ());
        };
        if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(());
        }
        if !model.is_empty() && labello_inference::model_digest(&model) != digest {
            return Err(());
        }
        let mut loaded = false;
        let result = (|| {
            let session = if let Some(index) = sessions.iter().position(|(key, _)| key == &digest) {
                sessions.remove(index).ok_or(WorkerFailure::Model)?
            } else {
                if sessions.len() == SESSION_LIMIT {
                    sessions.pop_front();
                }
                let session = InferenceSession::new(&model, provider, threads)
                    .map_err(|_| WorkerFailure::Model)?;
                loaded = true;
                (digest, session)
            };
            sessions.push_back(session);
            sessions
                .back_mut()
                .ok_or(WorkerFailure::Model)?
                .1
                .infer(&image, &config, &task)
                .map_err(|_| WorkerFailure::Execution)
        })();
        let bytes = serde_json::to_vec(&WorkerReply { result, loaded }).map_err(|_| ())?;
        if bytes.len() > MAX_REPLY {
            return Err(());
        }
        output
            .write_all(&(bytes.len() as u64).to_le_bytes())
            .map_err(|_| ())?;
        output.write_all(&bytes).map_err(|_| ())?;
        output.flush().map_err(|_| ())?;
    }
}
