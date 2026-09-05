use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("unprocessable entity: {0}")]
    Unprocessable(String),

    // Retain the public rejection while distinguishing enforced quotas from conflicts.
    #[error("{0}")]
    ResourceLimit(Box<ApiError>),

    // Some ownership denials deliberately use a public 404.
    #[error("{0}")]
    HiddenDenial(Box<ApiError>),

    #[error("invalid id: {0}")]
    InvalidId(#[from] labello_domain::IdValidationError),

    #[error("storage error: {0}")]
    Storage(#[from] labello_storage::StorageError),

    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let error_kind = self.kind();
        let mut response = (
            status,
            Json(ErrorBody {
                error: self.public_message(),
            }),
        )
            .into_response();
        response
            .extensions_mut()
            .insert(crate::logging::FailureDiagnostic {
                error_kind,
                warn: matches!(self, Self::ResourceLimit(_) | Self::HiddenDenial(_)),
            });
        response
    }
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::ResourceLimit(error) | ApiError::HiddenDenial(error) => error.status(),
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::InvalidId(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Storage(labello_storage::StorageError::NotFound(_)) => StatusCode::NOT_FOUND,
            ApiError::Storage(labello_storage::StorageError::AlreadyExists(_)) => {
                StatusCode::CONFLICT
            }
            ApiError::Storage(labello_storage::StorageError::OutsideDatasetRoot(_)) => {
                StatusCode::BAD_REQUEST
            }
            ApiError::Storage(labello_storage::StorageError::Unauthorized(_)) => {
                StatusCode::UNAUTHORIZED
            }
            ApiError::Storage(labello_storage::StorageError::InvalidAssignment(_)) => {
                StatusCode::BAD_REQUEST
            }
            ApiError::Storage(labello_storage::StorageError::InvalidCorrection(_)) => {
                StatusCode::BAD_REQUEST
            }
            ApiError::Storage(labello_storage::StorageError::Domain(
                labello_domain::DomainError::InvalidKeybindings(_),
            )) => StatusCode::BAD_REQUEST,
            ApiError::Storage(labello_storage::StorageError::Domain(
                labello_domain::DomainError::KeybindingConflict { .. },
            )) => StatusCode::CONFLICT,
            ApiError::Storage(labello_storage::StorageError::AssignmentConflict(_)) => {
                StatusCode::CONFLICT
            }
            ApiError::Storage(_)
            | ApiError::Http(_)
            | ApiError::Internal(_)
            | ApiError::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::ResourceLimit(_) => "resource_limit",
            Self::HiddenDenial(_) => "forbidden",
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::PayloadTooLarge(_) => "payload_too_large",
            Self::Unprocessable(_) => "unprocessable_entity",
            Self::InvalidId(_) => "invalid_id",
            Self::Storage(error) => error.kind(),
            Self::Http(_) => "http_client",
            Self::Internal(_) => "internal",
            Self::Json(_) => "serialization",
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::ResourceLimit(error) | Self::HiddenDenial(error) => error.public_message(),
            Self::Storage(labello_storage::StorageError::NotFound(_)) => "not found".to_string(),
            Self::Storage(labello_storage::StorageError::AlreadyExists(_)) => {
                "already exists".to_string()
            }
            Self::Storage(labello_storage::StorageError::OutsideDatasetRoot(_)) => {
                "path is outside the dataset root".to_string()
            }
            Self::Storage(labello_storage::StorageError::Unauthorized(_)) => {
                "unauthorized".to_string()
            }
            Self::Storage(labello_storage::StorageError::InvalidAssignment(message))
            | Self::Storage(labello_storage::StorageError::InvalidCorrection(message))
            | Self::Storage(labello_storage::StorageError::AssignmentConflict(message)) => {
                message.clone()
            }
            Self::Storage(labello_storage::StorageError::Domain(
                error @ (labello_domain::DomainError::InvalidKeybindings(_)
                | labello_domain::DomainError::KeybindingConflict { .. }),
            )) => error.to_string(),
            error if error.status().is_server_error() => "internal server error".to_string(),
            error => error.to_string(),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybinding_validation_errors_are_client_errors() {
        let invalid = ApiError::Storage(labello_storage::StorageError::Domain(
            labello_domain::DomainError::InvalidKeybindings("missing action".to_string()),
        ));
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let conflict = ApiError::Storage(labello_storage::StorageError::Domain(
            labello_domain::DomainError::KeybindingConflict {
                chord: "P".to_string(),
                actions: vec!["A".to_string(), "B".to_string()],
            },
        ));
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
    }
}
