//! Private capture and archive delivery for explicit ground-truth export profiles.

pub(crate) mod archive;
mod capture;
mod encoding;
mod image;
mod service;
mod source;
mod types;

pub use service::ExportService;
pub use types::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ExportFailure {
    #[error("export job was not found")]
    NotFound,
    #[error("export workers are busy")]
    Busy,
    #[error("export job is not ready for this operation")]
    NotReady,
    #[error("export selection is incompatible: {0}")]
    Policy(labello_domain::ExportPolicyError),
    #[error("export limits were exceeded")]
    Limit,
    #[error("export was cancelled")]
    Cancelled,
    #[error("export input is invalid")]
    InvalidInput,
    #[error("selected objects collapse to duplicate labels in the target reader")]
    AmbiguousObjects,
    #[error("an original image is incompatible with the selected export profile")]
    UnsupportedImage,
    #[error("export source changed during capture")]
    SourceChanged,
    #[error("export storage operation failed")]
    Storage,
    #[error("export archive verification failed")]
    Verification,
    #[error("export was interrupted by a server restart")]
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportLimits {
    pub max_images: usize,
    pub max_files: usize,
    pub max_source_bytes: u64,
    pub max_file_bytes: u64,
    pub max_decoded_image_bytes: u64,
    pub max_archive_bytes: u64,
    pub max_metadata_bytes: u64,
    pub max_concurrent_jobs: usize,
    pub max_concurrent_downloads: usize,
    pub max_retained_jobs: usize,
    pub retention_seconds: u64,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_images: 10_000,
            max_files: 30_010,
            max_source_bytes: 10 * 1024 * 1024 * 1024,
            max_file_bytes: 512 * 1024 * 1024,
            max_decoded_image_bytes: 256 * 1024 * 1024,
            max_archive_bytes: 12 * 1024 * 1024 * 1024,
            max_metadata_bytes: 32 * 1024 * 1024,
            max_concurrent_jobs: 1,
            max_concurrent_downloads: 2,
            max_retained_jobs: 8,
            retention_seconds: 24 * 60 * 60,
        }
    }
}

impl ExportLimits {
    pub fn validate(&self) -> Result<(), ExportFailure> {
        if self.max_images == 0
            || self.max_images > 100_000
            || self.max_files < 3
            || self.max_files > 300_010
            || self.max_file_bytes == 0
            || self.max_file_bytes > self.max_source_bytes
            || self.max_decoded_image_bytes == 0
            || self.max_decoded_image_bytes > 1024 * 1024 * 1024
            || self.max_source_bytes > self.max_archive_bytes
            || self.max_archive_bytes > 1024 * 1024 * 1024 * 1024
            || self.max_metadata_bytes == 0
            || self.max_metadata_bytes > 256 * 1024 * 1024
            || self.max_concurrent_jobs == 0
            || self.max_concurrent_jobs > 4
            || self.max_concurrent_downloads == 0
            || self.max_concurrent_downloads > 8
            || self.max_retained_jobs < self.max_concurrent_jobs
            || self.max_retained_jobs > 64
            || self.retention_seconds == 0
            || self.retention_seconds > 7 * 24 * 60 * 60
        {
            return Err(ExportFailure::Limit);
        }
        Ok(())
    }
}
