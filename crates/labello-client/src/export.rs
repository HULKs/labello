//! Dataset export transport contract. Storage retains capture and publication policy.

use std::collections::BTreeMap;

use labello_domain::{
    DatasetId, ExportClassMapping, ExportOmissionReason, ExportOptions, ImageId, Timestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOmittedImage {
    pub image_id: ImageId,
    pub reason: ExportOmissionReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportBlocker {
    pub image_id: ImageId,
    pub reason: ExportFailure,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub classes: Vec<ExportClassMapping>,
    pub included_images: usize,
    pub empty_images: usize,
    pub objects: usize,
    pub source_bytes: u64,
    pub omitted_images: usize,
    pub omission_counts: BTreeMap<ExportOmissionReason, usize>,
    /// At most 100 examples; complete omissions are retained in the manifest.
    pub omitted_samples: Vec<ExportOmittedImage>,
    pub blocking_images: usize,
    /// At most 100 examples. A corrected preflight may reveal further blockers.
    pub blockers: Vec<ExportBlocker>,
}

impl ExportSummary {
    pub fn can_start(&self) -> bool {
        self.included_images > 0 && self.blocking_images == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPhase {
    Capturing,
    Ready,
    Blocked,
    Building,
    Cancelling,
    Cancelled,
    Failed,
    Succeeded,
}

impl ExportPhase {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Capturing | Self::Building | Self::Cancelling)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub job_id: String,
    pub dataset_id: DatasetId,
    pub options: ExportOptions,
    pub phase: ExportPhase,
    pub summary: Option<ExportSummary>,
    pub failure: Option<ExportFailure>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub expires_at: Timestamp,
    pub archive_bytes: Option<u64>,
    pub archive_blake3: Option<String>,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCapabilities {
    pub available: bool,
    pub limits: ExportLimits,
}
