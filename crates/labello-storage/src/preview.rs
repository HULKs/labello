//! Disposable, authorized-by-caller image representations. Originals remain authoritative.

use crate::DatasetRepository;
use labello_domain::ImageRecord;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

mod codec;
mod disk;

pub const MAX_ENCODED_PREVIEW_BYTES: usize = 16 * 1024 * 1024;
const POLICY: &str = "preview-v1/triangle-native-depth-then-rgba8/no-orientation/no-color-conversion/first-frame/alpha-exact";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewProfile {
    StandardV1,
    DataSaverV1,
}

impl PreviewProfile {
    pub fn max_edge(self) -> u32 {
        match self {
            Self::StandardV1 => 1600,
            Self::DataSaverV1 => 1280,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::StandardV1 => "standard_v1",
            Self::DataSaverV1 => "data_saver_v1",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewConfig {
    pub max_source_bytes: u64,
    pub max_pixels: u64,
    pub max_decoded_bytes: u64,
    pub workers: usize,
    pub cache_bytes: u64,
    pub cache_entries: usize,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024 * 1024,
            max_pixels: 32_000_000,
            max_decoded_bytes: 256 * 1024 * 1024,
            workers: 2,
            cache_bytes: 2 * 1024 * 1024 * 1024,
            cache_entries: 4096,
        }
    }
}

impl PreviewConfig {
    pub fn validate(&self) -> Result<(), PreviewError> {
        if self.max_source_bytes == 0
            || self.max_source_bytes > 512 * 1024 * 1024
            || self.max_pixels == 0
            || self.max_pixels > 100_000_000
            || self.max_decoded_bytes == 0
            || self.max_decoded_bytes > 1024 * 1024 * 1024
            || !(1..=8).contains(&self.workers)
            || self.cache_bytes == 0
            || self.cache_bytes > 1024 * 1024 * 1024 * 1024
            || !(1..=16_384).contains(&self.cache_entries)
        {
            return Err(PreviewError::Configuration);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    #[error("invalid preview limits")]
    Configuration,
    #[error("preview source exceeds configured limits")]
    SourceLimit,
    #[error("preview decoder exceeds configured limits")]
    DecoderLimit,
    #[error("preview workers are busy")]
    Busy,
    #[error("preview cache quota is exhausted")]
    Quota,
    #[error("preview source changed; reconcile the image index")]
    SourceChanged,
    #[error("preview source is unavailable or unsupported")]
    Source,
    #[error("preview cache is unavailable")]
    Cache,
    #[error("preview decoding failed")]
    Decode,
    #[error("preview encoding failed")]
    Encode,
}

#[derive(Clone, Debug)]
pub struct EncodedPreview {
    pub profile: PreviewProfile,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub webp: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RgbaPreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub struct PreviewCache {
    inner: Arc<Inner>,
}
struct Inner {
    root: PathBuf,
    config: PreviewConfig,
    workers: Arc<Semaphore>,
    flights: Mutex<BTreeMap<String, Weak<AsyncMutex<()>>>>,
    disk: Mutex<disk::CacheState>,
    #[cfg(test)]
    generations: std::sync::atomic::AtomicUsize,
}

impl PreviewCache {
    /// One cache root belongs to one live service. The process lock is acquired lazily.
    pub fn new(root: impl Into<PathBuf>, config: PreviewConfig) -> Result<Self, PreviewError> {
        config.validate()?;
        Ok(Self {
            inner: Arc::new(Inner {
                root: root.into(),
                workers: Arc::new(Semaphore::new(config.workers)),
                config,
                flights: Mutex::new(BTreeMap::new()),
                disk: Mutex::new(disk::CacheState::default()),
                #[cfg(test)]
                generations: std::sync::atomic::AtomicUsize::new(0),
            }),
        })
    }

    pub async fn get(
        &self,
        repo: &DatasetRepository,
        record: &ImageRecord,
        profile: PreviewProfile,
    ) -> Result<EncodedPreview, PreviewError> {
        let key = cache_key(repo, record, profile);
        let flight = {
            let mut flights = self.inner.flights.lock().map_err(|_| PreviewError::Cache)?;
            flights.retain(|_, value| value.strong_count() != 0);
            if let Some(flight) = flights.get(&key).and_then(Weak::upgrade) {
                flight
            } else {
                let flight = Arc::new(AsyncMutex::new(()));
                flights.insert(key.clone(), Arc::downgrade(&flight));
                flight
            }
        };
        let guard = flight.lock_owned().await;
        let permit = self
            .inner
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PreviewError::Busy)?;
        let inner = self.inner.clone();
        let repo_root = repo.root().to_path_buf();
        let record = record.clone();
        // Once started, a cancelled caller leaves one bounded worker to finish or
        // clean up publication. Its flight and worker permits remain held.
        tokio::task::spawn_blocking(move || {
            let (_guard, _permit) = (guard, permit);
            let source = codec::source_bytes(&repo_root, &record, &inner.config)?;
            {
                let mut disk = inner.disk.lock().map_err(|_| PreviewError::Cache)?;
                disk.initialize(&inner.root, &inner.config)?;
                if let Some(preview) = disk.read(&inner.root, &key, profile, &record)? {
                    return Ok(preview);
                }
            }
            let rgba = codec::resize(&source, &record, profile.max_edge(), &inner.config)?;
            let webp = codec::encode(&rgba, profile)?;
            let preview = EncodedPreview {
                profile,
                width: rgba.width,
                height: rgba.height,
                original_width: record.width,
                original_height: record.height,
                webp,
            };
            let mut disk = inner.disk.lock().map_err(|_| PreviewError::Cache)?;
            disk.publish(&inner.root, &inner.config, &key, &preview)?;
            #[cfg(test)]
            inner
                .generations
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(preview)
        })
        .await
        .map_err(|_| PreviewError::Encode)?
    }

    /// Explicit full-detail display only; downloads retain their separate contract.
    pub async fn original_detail(
        &self,
        repo: &DatasetRepository,
        record: &ImageRecord,
    ) -> Result<Vec<u8>, PreviewError> {
        let permit = self
            .inner
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PreviewError::Busy)?;
        let inner = self.inner.clone();
        let root = repo.root().to_path_buf();
        let record = record.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let source = codec::source_bytes(&root, &record, &inner.config)?;
            // Validate decoder headers and configured allocation/pixel bounds before transfer.
            drop(codec::decoder(&source, &record, &inner.config)?);
            Ok(source)
        })
        .await
        .map_err(|_| PreviewError::Decode)?
    }

    /// The legacy fallback shares the same source, pixel, allocation and worker limits.
    pub async fn rgba(
        &self,
        repo: &DatasetRepository,
        record: &ImageRecord,
        max_edge: u32,
    ) -> Result<RgbaPreview, PreviewError> {
        let permit = self
            .inner
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| PreviewError::Busy)?;
        let inner = self.inner.clone();
        let root = repo.root().to_path_buf();
        let record = record.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let source = codec::source_bytes(&root, &record, &inner.config)?;
            codec::resize(&source, &record, max_edge.clamp(256, 4096), &inner.config)
        })
        .await
        .map_err(|_| PreviewError::Decode)?
    }
}

fn cache_key(repo: &DatasetRepository, record: &ImageRecord, profile: PreviewProfile) -> String {
    let mut hash = blake3::Hasher::new();
    let format = image::ImageFormat::from_path(&record.canonical_path)
        .map(|format| format.to_mime_type())
        .unwrap_or("unsupported");
    for value in [
        POLICY.as_bytes(),
        format.as_bytes(),
        env!("LABELLO_PREVIEW_DEPENDENCIES").as_bytes(),
        repo.root().as_os_str().as_encoded_bytes(),
        record.image_id.as_str().as_bytes(),
        record.blake3.as_bytes(),
        profile.name().as_bytes(),
    ] {
        hash.update(&(value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    hash.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests;
