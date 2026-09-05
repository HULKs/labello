use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Seek},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use labello_domain::{DatasetId, ExportOptions, now};
use tokio::sync::{Mutex, Semaphore};

use crate::{DatasetRepository, fsjson::write_json_atomic};

use super::{
    ExportFailure, ExportJob, ExportLimits, ExportPhase, archive,
    capture::{self, Capture},
};

#[derive(Clone)]
pub struct ExportService {
    inner: Arc<Inner>,
}

struct Inner {
    // Keep the descriptor alive: every job path resolves beneath this pinned root.
    _root: File,
    path: PathBuf,
    limits: ExportLimits,
    jobs: Mutex<BTreeMap<String, Entry>>,
    workers: Arc<Semaphore>,
    downloads: Arc<Semaphore>,
}

struct Entry {
    job: ExportJob,
    capture: Option<Arc<Capture>>,
    cancel: Arc<AtomicBool>,
}

impl ExportService {
    pub async fn new(datasets_root: &Path, limits: ExportLimits) -> Result<Self, ExportFailure> {
        limits.validate()?;
        let (root, path) = private_root(datasets_root)?;
        let service = Self {
            inner: Arc::new(Inner {
                _root: root,
                path,
                workers: Arc::new(Semaphore::new(limits.max_concurrent_jobs)),
                downloads: Arc::new(Semaphore::new(limits.max_concurrent_downloads)),
                limits,
                jobs: Mutex::new(BTreeMap::new()),
            }),
        };
        service.recover().await?;
        let weak = Arc::downgrade(&service.inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let service = Self { inner };
                let mut jobs = service.inner.jobs.lock().await;
                if service.prune(&mut jobs).await.is_err() {
                    tracing::warn!(event = "export_retention_cleanup_failed");
                }
            }
        });
        Ok(service)
    }

    pub fn limits(&self) -> &ExportLimits {
        &self.inner.limits
    }

    pub async fn jobs(&self, dataset_id: &DatasetId) -> Result<Vec<ExportJob>, ExportFailure> {
        let mut jobs = self.inner.jobs.lock().await;
        self.prune(&mut jobs).await?;
        let mut result = jobs
            .values()
            .filter(|entry| &entry.job.dataset_id == dataset_id)
            .map(|entry| entry.job.clone())
            .collect::<Vec<_>>();
        result.sort_by_key(|job| std::cmp::Reverse(job.created_at));
        Ok(result)
    }

    pub async fn preflight(
        &self,
        dataset_id: &DatasetId,
        repository: DatasetRepository,
        options: ExportOptions,
    ) -> Result<ExportJob, ExportFailure> {
        dataset_id
            .validate_path_segment()
            .map_err(|_| ExportFailure::InvalidInput)?;
        if options.classes.is_empty()
            || options.classes.len() > 256
            || options.split_choices.len() > self.inner.limits.max_images
        {
            return Err(ExportFailure::InvalidInput);
        }
        let permit = Arc::clone(&self.inner.workers)
            .try_acquire_owned()
            .map_err(|_| ExportFailure::Busy)?;
        let mut jobs = self.inner.jobs.lock().await;
        self.prune(&mut jobs).await?;
        if jobs.len() >= self.inner.limits.max_retained_jobs {
            return Err(ExportFailure::Limit);
        }
        let timestamp = now();
        let job_id = uuid::Uuid::new_v4().to_string();
        let job = ExportJob {
            job_id: job_id.clone(),
            dataset_id: dataset_id.clone(),
            options: options.clone(),
            phase: ExportPhase::Capturing,
            summary: None,
            failure: None,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: timestamp
                + std::time::Duration::from_secs(self.inner.limits.retention_seconds),
            archive_bytes: None,
            archive_blake3: None,
        };
        let directory = self.job_dir(&job_id)?;
        create_private_directory(&directory)?;
        let spool = directory.join("spool");
        let setup = async {
            create_private_directory(&spool)?;
            self.persist(&job).await
        }
        .await;
        if let Err(error) = setup {
            tokio::fs::remove_dir_all(&directory)
                .await
                .map_err(|_| ExportFailure::Storage)?;
            return Err(error);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        jobs.insert(
            job_id.clone(),
            Entry {
                job: job.clone(),
                capture: None,
                cancel: Arc::clone(&cancel),
            },
        );
        let service = self.clone();
        let limits = self.inner.limits.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = tokio::spawn(capture::prepare(
                repository,
                spool,
                job_id.clone(),
                options,
                limits,
                cancel,
            ))
            .await
            .unwrap_or(Err(ExportFailure::Storage));
            if service.finish_capture(&job_id, result).await.is_err() {
                tracing::warn!(event = "export_status_write_failed", job_id = %job_id);
            }
        });
        Ok(job)
    }

    pub async fn job(
        &self,
        dataset_id: &DatasetId,
        job_id: &str,
    ) -> Result<ExportJob, ExportFailure> {
        self.job_dir(job_id)?;
        let mut jobs = self.inner.jobs.lock().await;
        self.prune(&mut jobs).await?;
        Ok(entry(&jobs, dataset_id, job_id)?.job.clone())
    }

    pub async fn start(
        &self,
        dataset_id: &DatasetId,
        job_id: &str,
    ) -> Result<ExportJob, ExportFailure> {
        self.job_dir(job_id)?;
        let mut jobs = self.inner.jobs.lock().await;
        self.prune(&mut jobs).await?;
        let current = entry(&jobs, dataset_id, job_id)?;
        if current.job.phase != ExportPhase::Ready {
            return Err(ExportFailure::NotReady);
        }
        let capture = Arc::clone(current.capture.as_ref().ok_or(ExportFailure::NotReady)?);
        if !capture.summary.can_start() {
            return Err(ExportFailure::NotReady);
        }
        let permit = Arc::clone(&self.inner.workers)
            .try_acquire_owned()
            .map_err(|_| ExportFailure::Busy)?;
        let cancel = Arc::clone(&current.cancel);
        let mut job = current.job.clone();
        job.phase = ExportPhase::Building;
        job.updated_at = now();
        self.persist(&job).await?;
        jobs.get_mut(job_id).expect("existing job").job = job.clone();
        let service = self.clone();
        let job_id = job_id.to_owned();
        tokio::spawn(async move {
            let _permit = permit;
            let worker = service.clone();
            let id = job_id.clone();
            let result = tokio::task::spawn_blocking(move || worker.build(&id, &capture, &cancel))
                .await
                .unwrap_or(Err(ExportFailure::Storage));
            if service.finish_build(&job_id, result).await.is_err() {
                tracing::warn!(event = "export_status_write_failed", job_id = %job_id);
            }
        });
        Ok(job)
    }

    pub async fn cancel(
        &self,
        dataset_id: &DatasetId,
        job_id: &str,
    ) -> Result<ExportJob, ExportFailure> {
        self.job_dir(job_id)?;
        let mut jobs = self.inner.jobs.lock().await;
        self.prune(&mut jobs).await?;
        let current = entry(&jobs, dataset_id, job_id)?;
        if matches!(
            current.job.phase,
            ExportPhase::Succeeded | ExportPhase::Failed | ExportPhase::Cancelled
        ) {
            return Ok(current.job.clone());
        }
        current.cancel.store(true, Ordering::Release);
        let active = current.job.phase.is_active();
        let mut job = current.job.clone();
        job.phase = if active {
            ExportPhase::Cancelling
        } else {
            ExportPhase::Cancelled
        };
        job.updated_at = now();
        if !active {
            self.clean_payload(job_id).await?;
        }
        self.persist(&job).await?;
        let current = jobs.get_mut(job_id).expect("existing job");
        current.job = job.clone();
        current.capture = None;
        Ok(job)
    }

    /// The API must authorize this request against current dataset roles before calling.
    /// Return an open descriptor so the transport can stream without allocating the ZIP.
    pub async fn download(
        &self,
        dataset_id: &DatasetId,
        job_id: &str,
    ) -> Result<(File, ExportJob, tokio::sync::OwnedSemaphorePermit), ExportFailure> {
        let job = self.job(dataset_id, job_id).await?;
        if job.phase != ExportPhase::Succeeded {
            return Err(ExportFailure::NotReady);
        }
        let permit = Arc::clone(&self.inner.downloads)
            .try_acquire_owned()
            .map_err(|_| ExportFailure::Busy)?;
        let path = self.job_dir(job_id)?;
        let expected = job.clone();
        let limits = self.inner.limits.clone();
        let file = tokio::task::spawn_blocking(move || {
            let root = File::open(path).map_err(|_| ExportFailure::Storage)?;
            let mut file = archive::open_regular(&root, "dataset.zip")?;
            let (bytes, hash) =
                hash_file(&mut file, limits.max_archive_bytes, &AtomicBool::new(false))?;
            if Some(bytes) != expected.archive_bytes || Some(hash) != expected.archive_blake3 {
                return Err(ExportFailure::Verification);
            }
            Ok(file)
        })
        .await
        .map_err(|_| ExportFailure::Storage)??;
        Ok((file, job, permit))
    }

    pub async fn shutdown(&self) {
        for current in self.inner.jobs.lock().await.values() {
            if current.job.phase.is_active() {
                current.cancel.store(true, Ordering::Release);
            }
        }
    }

    fn job_dir(&self, job_id: &str) -> Result<PathBuf, ExportFailure> {
        if !uuid::Uuid::parse_str(job_id).is_ok_and(|id| id.to_string() == job_id) {
            return Err(ExportFailure::NotFound);
        }
        Ok(self.inner.path.join(job_id))
    }

    async fn persist(&self, job: &ExportJob) -> Result<(), ExportFailure> {
        write_json_atomic(&self.job_dir(&job.job_id)?.join("job.json"), job)
            .await
            .map_err(|_| ExportFailure::Storage)
    }

    async fn clean_payload(&self, job_id: &str) -> Result<(), ExportFailure> {
        let root = self.job_dir(job_id)?;
        for name in ["spool", "building.zip", "dataset.zip"] {
            let path = root.join(name);
            match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) if metadata.is_dir() => tokio::fs::remove_dir_all(path)
                    .await
                    .map_err(|_| ExportFailure::Storage)?,
                Ok(_) => tokio::fs::remove_file(path)
                    .await
                    .map_err(|_| ExportFailure::Storage)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ExportFailure::Storage),
            }
        }
        Ok(())
    }

    async fn finish_capture(
        &self,
        job_id: &str,
        result: Result<Capture, ExportFailure>,
    ) -> Result<(), ExportFailure> {
        let mut jobs = self.inner.jobs.lock().await;
        let current = jobs.get_mut(job_id).ok_or(ExportFailure::NotFound)?;
        let mut job = current.job.clone();
        job.updated_at = now();
        let capture = match result {
            _ if current.cancel.load(Ordering::Acquire) => {
                job.phase = ExportPhase::Cancelled;
                None
            }
            Ok(capture) if capture.dataset_id() == &job.dataset_id => {
                job.summary = Some(capture.summary.clone());
                if capture.summary.can_start() {
                    job.phase = ExportPhase::Ready;
                    Some(Arc::new(capture))
                } else {
                    job.phase = ExportPhase::Blocked;
                    None
                }
            }
            Ok(_) => {
                job.phase = ExportPhase::Failed;
                job.failure = Some(ExportFailure::SourceChanged);
                None
            }
            Err(failure) => {
                job.phase = ExportPhase::Failed;
                job.failure = Some(failure);
                None
            }
        };
        if capture.is_none() && self.clean_payload(job_id).await.is_err() {
            job.phase = ExportPhase::Failed;
            job.failure = Some(ExportFailure::Storage);
        }
        if let Err(error) = self.persist(&job).await {
            current.job.phase = ExportPhase::Failed;
            current.job.failure = Some(ExportFailure::Storage);
            current.capture = None;
            self.clean_payload(job_id).await?;
            return Err(error);
        }
        current.job = job;
        current.capture = capture;
        Ok(())
    }

    fn build(
        &self,
        job_id: &str,
        capture: &Capture,
        cancel: &AtomicBool,
    ) -> Result<(u64, String), ExportFailure> {
        capture.verify_source(&self.inner.limits, cancel)?;
        let directory = self.job_dir(job_id)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let output = options
            .open(directory.join("building.zip"))
            .map_err(|_| ExportFailure::Storage)?;
        let mut output = archive::build(
            &directory.join("spool"),
            output,
            &capture.files,
            &self.inner.limits,
            cancel,
        )?;
        let artifact = hash_file(&mut output, self.inner.limits.max_archive_bytes, cancel)?;
        capture.verify_source(&self.inner.limits, cancel)?;
        Ok(artifact)
    }

    async fn finish_build(
        &self,
        job_id: &str,
        result: Result<(u64, String), ExportFailure>,
    ) -> Result<(), ExportFailure> {
        let mut jobs = self.inner.jobs.lock().await;
        let current = jobs.get_mut(job_id).ok_or(ExportFailure::NotFound)?;
        let mut job = current.job.clone();
        job.updated_at = now();
        match result {
            _ if current.cancel.load(Ordering::Acquire) => {
                job.phase = ExportPhase::Cancelled;
            }
            Ok((bytes, digest)) => {
                let directory = self.job_dir(job_id)?;
                // The destination is private and absent. A no-replace hard link atomically
                // publishes the already synced artifact; only the final status exposes it.
                let publication = async {
                    tokio::fs::hard_link(
                        directory.join("building.zip"),
                        directory.join("dataset.zip"),
                    )
                    .await
                    .map_err(|_| ExportFailure::Storage)?;
                    tokio::fs::remove_file(directory.join("building.zip"))
                        .await
                        .map_err(|_| ExportFailure::Storage)?;
                    File::open(&directory)
                        .and_then(|file| file.sync_all())
                        .map_err(|_| ExportFailure::Storage)
                }
                .await;
                match publication {
                    Ok(()) => {
                        job.phase = ExportPhase::Succeeded;
                        job.archive_bytes = Some(bytes);
                        job.archive_blake3 = Some(digest);
                    }
                    Err(failure) => {
                        job.phase = ExportPhase::Failed;
                        job.failure = Some(failure);
                    }
                }
            }
            Err(failure) => {
                job.phase = ExportPhase::Failed;
                job.failure = Some(failure);
            }
        }
        current.capture = None;
        let cleanup = if job.phase == ExportPhase::Succeeded {
            tokio::fs::remove_dir_all(self.job_dir(job_id)?.join("spool"))
                .await
                .map_err(|_| ExportFailure::Storage)
        } else {
            self.clean_payload(job_id).await
        };
        if cleanup.is_err() {
            job.phase = ExportPhase::Failed;
            job.failure = Some(ExportFailure::Storage);
            job.archive_bytes = None;
            job.archive_blake3 = None;
        }
        if let Err(error) = self.persist(&job).await {
            current.job.phase = ExportPhase::Failed;
            current.job.failure = Some(ExportFailure::Storage);
            self.clean_payload(job_id).await?;
            return Err(error);
        }
        current.job = job;
        Ok(())
    }

    async fn prune(&self, jobs: &mut BTreeMap<String, Entry>) -> Result<(), ExportFailure> {
        let expired = jobs
            .iter()
            .filter(|(_, current)| current.job.expires_at <= now())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            let current = &jobs[&id];
            if current.job.phase.is_active() {
                current.cancel.store(true, Ordering::Release);
                continue;
            }
            tokio::fs::remove_dir_all(self.job_dir(&id)?)
                .await
                .map_err(|_| ExportFailure::Storage)?;
            jobs.remove(&id);
        }
        Ok(())
    }

    async fn recover(&self) -> Result<(), ExportFailure> {
        let mut directories = tokio::fs::read_dir(&self.inner.path)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let mut jobs = self.inner.jobs.lock().await;
        while let Some(directory) = directories
            .next_entry()
            .await
            .map_err(|_| ExportFailure::Storage)?
        {
            let id = directory
                .file_name()
                .into_string()
                .map_err(|_| ExportFailure::InvalidInput)?;
            let path = self.job_dir(&id)?;
            if !directory
                .file_type()
                .await
                .map_err(|_| ExportFailure::Storage)?
                .is_dir()
            {
                return Err(ExportFailure::InvalidInput);
            }
            let root = File::open(&path).map_err(|_| ExportFailure::Storage)?;
            let job_file = match archive::open_regular(&root, "job.json") {
                Ok(file) => file,
                Err(_) if !path.join("job.json").exists() => {
                    // Crash between private directory reservation and its first durable status.
                    tokio::fs::remove_dir_all(path)
                        .await
                        .map_err(|_| ExportFailure::Storage)?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if job_file
                .metadata()
                .map_err(|_| ExportFailure::Storage)?
                .len()
                > self.inner.limits.max_metadata_bytes
            {
                return Err(ExportFailure::Limit);
            }
            let mut text = Vec::new();
            job_file
                .take(self.inner.limits.max_metadata_bytes + 1)
                .read_to_end(&mut text)
                .map_err(|_| ExportFailure::Storage)?;
            if text.len() as u64 > self.inner.limits.max_metadata_bytes {
                return Err(ExportFailure::Limit);
            }
            let mut job: ExportJob =
                serde_json::from_slice(&text).map_err(|_| ExportFailure::InvalidInput)?;
            if job.job_id != id {
                return Err(ExportFailure::InvalidInput);
            }
            job.dataset_id
                .validate_path_segment()
                .map_err(|_| ExportFailure::InvalidInput)?;
            if job.expires_at <= now() {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .map_err(|_| ExportFailure::Storage)?;
                continue;
            }
            if jobs.len() >= self.inner.limits.max_retained_jobs {
                return Err(ExportFailure::Limit);
            }
            if job.phase.is_active() || job.phase == ExportPhase::Ready {
                job.phase = ExportPhase::Failed;
                job.failure = Some(ExportFailure::Interrupted);
                job.updated_at = now();
                self.clean_payload(&id).await?;
                self.persist(&job).await?;
            } else if job.phase != ExportPhase::Succeeded {
                self.clean_payload(&id).await?;
            }
            jobs.insert(
                id,
                Entry {
                    job,
                    capture: None,
                    cancel: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        self.prune(&mut jobs).await
    }
}

fn entry<'a>(
    jobs: &'a BTreeMap<String, Entry>,
    dataset: &DatasetId,
    job_id: &str,
) -> Result<&'a Entry, ExportFailure> {
    jobs.get(job_id)
        .filter(|current| &current.job.dataset_id == dataset)
        .ok_or(ExportFailure::NotFound)
}

fn hash_file(
    file: &mut File,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<(u64, String), ExportFailure> {
    file.rewind().map_err(|_| ExportFailure::Storage)?;
    let mut digest = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(ExportFailure::Cancelled);
        }
        let count = file.read(&mut buffer).map_err(|_| ExportFailure::Storage)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or(ExportFailure::Limit)?;
        if bytes > limit {
            return Err(ExportFailure::Limit);
        }
        digest.update(&buffer[..count]);
    }
    file.rewind().map_err(|_| ExportFailure::Storage)?;
    Ok((bytes, digest.finalize().to_hex().to_string()))
}

fn create_private_directory(path: &Path) -> Result<(), ExportFailure> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| ExportFailure::Storage)
}

#[cfg(target_os = "linux")]
fn private_root(datasets_root: &Path) -> Result<(File, PathBuf), ExportFailure> {
    use rustix::fs::{Mode, OFlags, ResolveFlags, mkdirat, openat2};
    use std::os::fd::AsRawFd;
    let mut root = File::open(datasets_root).map_err(|_| ExportFailure::Storage)?;
    for name in [".labello-server", "exports"] {
        match mkdirat(&root, name, Mode::from_bits_truncate(0o700)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(_) => return Err(ExportFailure::Storage),
        }
        root = File::from(
            openat2(
                &root,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
            )
            .map_err(|_| ExportFailure::InvalidInput)?,
        );
    }
    use std::os::unix::fs::PermissionsExt;
    root.set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|_| ExportFailure::Storage)?;
    let path = PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
    Ok((root, path))
}

#[cfg(not(target_os = "linux"))]
fn private_root(_datasets_root: &Path) -> Result<(File, PathBuf), ExportFailure> {
    Err(ExportFailure::InvalidInput)
}

#[cfg(test)]
mod tests;
