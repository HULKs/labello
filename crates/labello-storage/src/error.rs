use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("json error at {path:?}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("toml decode error at {path:?}: {source}")]
    TomlDecode {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("toml encode error at {path:?}: {source}")]
    TomlEncode {
        path: PathBuf,
        source: toml::ser::Error,
    },

    #[error("image error at {path:?}: {source}")]
    Image {
        path: PathBuf,
        source: image::ImageError,
    },

    #[error("domain error: {0}")]
    Domain(#[from] labello_domain::DomainError),

    #[error("required file does not exist: {0:?}")]
    NotFound(PathBuf),

    #[error("dataset is already initialized: {0:?}")]
    AlreadyExists(PathBuf),

    #[error("path is outside dataset root: {0:?}")]
    OutsideDatasetRoot(PathBuf),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("invalid assignment: {0}")]
    InvalidAssignment(String),

    #[error("invalid reviewer correction: {0}")]
    InvalidCorrection(String),

    #[error("assignment conflict: {0}")]
    AssignmentConflict(String),

    #[error("background storage task failed: {0}")]
    BackgroundTask(String),

    #[error("dataset import error ({code}): {message}")]
    Import { code: String, message: String },
}

impl StorageError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "storage_io",
            Self::Json { .. } => "storage_json",
            Self::TomlDecode { .. } => "storage_toml_decode",
            Self::TomlEncode { .. } => "storage_toml_encode",
            Self::Image { .. } => "storage_image",
            Self::Domain(_) => "storage_domain",
            Self::NotFound(_) => "storage_not_found",
            Self::AlreadyExists(_) => "storage_already_exists",
            Self::OutsideDatasetRoot(_) => "storage_outside_dataset_root",
            Self::Unauthorized(_) => "storage_unauthorized",
            Self::InvalidAssignment(_) => "storage_invalid_assignment",
            Self::InvalidCorrection(_) => "storage_invalid_correction",
            Self::AssignmentConflict(message) => review_conflict_kind(message),
            Self::BackgroundTask(_) => "storage_background_task",
            Self::Import { .. } => "storage_import",
        }
    }

    pub fn safe_diagnostic(&self) -> Option<String> {
        match self {
            Self::Io { source, .. } => Some(format!("{}: {source}", source.kind())),
            Self::Json { source, .. } => Some(source.to_string()),
            Self::TomlDecode { source, .. } => Some(source.message().to_string()),
            Self::TomlEncode { source, .. } => Some(source.to_string()),
            Self::Image { source, .. } => Some(source.to_string()),
            Self::Import { code, .. } => Some(code.clone()),
            _ => None,
        }
    }
}

// Only exact known messages receive specific codes. Never expose arbitrary
// conflict text through diagnostics, including domain validation messages.
fn review_conflict_kind(message: &str) -> &'static str {
    match message {
        "approval review is no longer enabled for this task" => "storage_review_disabled",
        "previous review assignment is missing" => "storage_review_assignment_missing",
        "previous review is not a skipped or completed assignment for this task" => {
            "storage_review_assignment_unfinished"
        }
        "this historical review has no captured revision context; claim current work instead" => {
            "storage_review_context_missing"
        }
        "previous review task configuration changed" => "storage_review_task_changed",
        "previous review submission changed" => "storage_review_submission_changed",
        "a later assignment attempt superseded this previous review" => {
            "storage_review_later_assignment"
        }
        "another assignment still owns this task" => "storage_review_task_owned",
        "previous assignment terminal boundary is missing" => "storage_review_boundary_missing",
        "later work superseded the previous review" => "storage_review_later_work",
        "previous review targets or migration confirmation changed" => {
            "storage_review_targets_changed"
        }
        "skipped review is no longer eligible" => "storage_review_skip_ineligible",
        "review revision context is missing" => "storage_review_context_missing",
        "review revision targets or task configuration changed" => {
            "storage_review_revision_context_changed"
        }
        "review revision opening event is missing" => "storage_review_opening_missing",
        "later work invalidated this review revision" => "storage_review_later_work",
        "revision retry contains different decisions" => "storage_review_retry_changed",
        "review history is refreshing; retry Previous" => "storage_review_history_refreshing",
        "previous review image is no longer in the dataset" => "storage_review_image_removed",
        "this is no longer the immediately previous review assignment" => {
            "storage_review_not_previous"
        }
        _ => "storage_assignment_conflict",
    }
}

pub type StorageResult<T> = Result<T, StorageError>;

pub(crate) trait PathIo<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> StorageResult<T>;
}

impl<T> PathIo<T> for Result<T, std::io::Error> {
    fn with_path(self, path: impl Into<PathBuf>) -> StorageResult<T> {
        self.map_err(|source| StorageError::Io {
            path: path.into(),
            source,
        })
    }
}

pub(crate) trait PathJson<T> {
    fn with_json_path(self, path: impl Into<PathBuf>) -> StorageResult<T>;
}

impl<T> PathJson<T> for Result<T, serde_json::Error> {
    fn with_json_path(self, path: impl Into<PathBuf>) -> StorageResult<T> {
        self.map_err(|source| StorageError::Json {
            path: path.into(),
            source,
        })
    }
}

pub(crate) trait PathTomlDecode<T> {
    fn with_toml_decode_path(self, path: impl Into<PathBuf>) -> StorageResult<T>;
}

impl<T> PathTomlDecode<T> for Result<T, toml::de::Error> {
    fn with_toml_decode_path(self, path: impl Into<PathBuf>) -> StorageResult<T> {
        self.map_err(|source| StorageError::TomlDecode {
            path: path.into(),
            source,
        })
    }
}

pub(crate) trait PathTomlEncode<T> {
    fn with_toml_encode_path(self, path: impl Into<PathBuf>) -> StorageResult<T>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_conflict_diagnostics_use_allowlisted_kinds_and_hide_text() {
        let known = StorageError::AssignmentConflict("previous review submission changed".into());
        assert_eq!(known.kind(), "storage_review_submission_changed");
        assert_eq!(known.safe_diagnostic(), None);

        let untrusted =
            StorageError::AssignmentConflict("private-conflict-sentinel: /secret/path".into());
        assert_eq!(untrusted.kind(), "storage_assignment_conflict");
        assert_eq!(untrusted.safe_diagnostic(), None);
    }
}

impl<T> PathTomlEncode<T> for Result<T, toml::ser::Error> {
    fn with_toml_encode_path(self, path: impl Into<PathBuf>) -> StorageResult<T> {
        self.map_err(|source| StorageError::TomlEncode {
            path: path.into(),
            source,
        })
    }
}
