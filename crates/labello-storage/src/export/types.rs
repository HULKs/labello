use std::collections::BTreeMap;

use labello_domain::{
    DatasetId, ExportClassMapping, ExportOmissionReason, ExportOptions, ImageId, Timestamp,
};
use serde::{Deserialize, Serialize};

use super::ExportFailure;

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
