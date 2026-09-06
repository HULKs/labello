/// Closed, versioned display policies; arbitrary sizes and qualities are not wire input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImagePreviewProfile {
    #[default]
    StandardV1,
    DataSaverV1,
}
impl ImagePreviewProfile {
    pub fn as_str(self) -> &'static str { match self { Self::StandardV1 => "standard_v1", Self::DataSaverV1 => "data_saver_v1" } }
    pub fn max_edge(self) -> u32 { match self { Self::StandardV1 => 1600, Self::DataSaverV1 => 1280 } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedImagePreview {
    pub image_id: ImageId,
    pub profile: ImagePreviewProfile,
    pub width: u32,
    pub height: u32,
    pub original_width: u32,
    pub original_height: u32,
    pub webp: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
    pub image_id: ImageId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreview {
    pub image_id: ImageId,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestReport {
    pub discovered_files: usize,
    pub new_images: usize,
    pub duplicate_files: Vec<DuplicateImage>,
    pub changed_paths: Vec<ChangedPath>,
    pub unreadable_files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestJobStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestJob {
    pub job_id: String,
    pub dataset_id: DatasetId,
    pub status: IngestJobStatus,
    pub report: Option<IngestReport>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateImage {
    pub image_id: ImageId,
    pub canonical_path: String,
    pub duplicate_path: String,
    pub blake3: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPath {
    pub relative_path: String,
    pub previous_blake3: String,
    pub current_blake3: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSummary {
    pub image: ImageRecord,
    pub event_sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageExplorerQuery {
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    pub search: Option<String>,
    pub status: Option<TaskStatus>,
    pub task_id: Option<TaskId>,
    pub class_id: Option<ClassId>,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    25
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    pub file_name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}
