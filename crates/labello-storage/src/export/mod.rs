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

#[cfg(test)]
mod round_trip;

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
    #[error("selected mappings omit known objects from an included task")]
    UnmappedObjects,
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
    pub max_images: Option<usize>,
    pub max_files: Option<usize>,
    pub max_source_bytes: Option<u64>,
    pub max_file_bytes: Option<u64>,
    pub max_decoded_image_bytes: Option<u64>,
    pub max_archive_bytes: Option<u64>,
    pub max_metadata_bytes: Option<u64>,
    pub max_concurrent_jobs: usize,
    pub max_concurrent_downloads: usize,
    pub max_retained_jobs: usize,
    pub retention_seconds: u64,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_images: None,
            max_files: None,
            max_source_bytes: None,
            max_file_bytes: None,
            max_decoded_image_bytes: None,
            max_archive_bytes: None,
            max_metadata_bytes: None,
            max_concurrent_jobs: 1,
            max_concurrent_downloads: 2,
            max_retained_jobs: 8,
            retention_seconds: 24 * 60 * 60,
        }
    }
}

impl ExportLimits {
    pub fn validate(&self) -> Result<(), ExportFailure> {
        if self.max_images == Some(0)
            || self.max_files.is_some_and(|n| n < 3)
            || self.max_file_bytes == Some(0)
            || self.max_decoded_image_bytes == Some(0)
            || self.max_source_bytes == Some(0)
            || self.max_archive_bytes == Some(0)
            || self.max_metadata_bytes == Some(0)
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

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn export_size_limits_are_optional_and_have_no_fixed_upper_ceiling() {
        let defaults: ExportLimits = serde_json::from_str("{}").unwrap();
        defaults.validate().unwrap();
        for field in [
            "maxImages",
            "maxFiles",
            "maxSourceBytes",
            "maxFileBytes",
            "maxDecodedImageBytes",
            "maxArchiveBytes",
            "maxMetadataBytes",
        ] {
            assert!(serde_json::to_value(&defaults).unwrap()[field].is_null());
        }
        let explicit: ExportLimits = serde_json::from_value(serde_json::json!({
            "maxImages": 200_000, "maxFiles": 600_010,
            "maxSourceBytes": 2_u64 * 1024 * 1024 * 1024 * 1024,
            "maxArchiveBytes": 3_u64 * 1024 * 1024 * 1024 * 1024,
            "maxDecodedImageBytes": 2_u64 * 1024 * 1024 * 1024,
            "maxMetadataBytes": 512_u64 * 1024 * 1024
        }))
        .unwrap();
        explicit.validate().unwrap();
        assert_eq!(explicit.max_images, Some(200_000));
        assert_eq!(explicit.max_file_bytes, None);
    }
}
