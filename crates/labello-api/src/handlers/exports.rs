use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use labello_domain::{DatasetId, DatasetRole, ExportOptions};
use labello_storage::export::{ExportFailure, ExportService};

use crate::{
    ApiState,
    auth::{actor_from_headers, ensure_dataset_role},
    error::{ApiError, ApiResult},
};

pub(super) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/datasets/{dataset_id}/exports/capabilities",
            get(capabilities),
        )
        .route("/datasets/{dataset_id}/exports", get(list).post(preflight))
        .route("/datasets/{dataset_id}/exports/{job_id}", get(status))
        .route("/datasets/{dataset_id}/exports/{job_id}/start", post(start))
        .route(
            "/datasets/{dataset_id}/exports/{job_id}/cancel",
            post(cancel),
        )
        .route(
            "/datasets/{dataset_id}/exports/{job_id}/download",
            get(download),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
}

async fn authorize(state: &ApiState, headers: &HeaderMap, dataset: &DatasetId) -> ApiResult<()> {
    let actor = actor_from_headers(state, headers)?;
    let metadata = state.repo(dataset)?.load_dataset_config().await?;
    ensure_dataset_role(&metadata, &actor, DatasetRole::DataAdmin)
}

fn service(state: &ApiState) -> ApiResult<&ExportService> {
    state
        .export_service()
        .map(AsRef::as_ref)
        .ok_or_else(|| ApiError::Conflict("dataset export is unavailable on this server".into()))
}

async fn capabilities(
    State(state): State<ApiState>,
    Path(dataset): Path<DatasetId>,
    headers: HeaderMap,
) -> ApiResult<Json<labello_client::ExportCapabilities>> {
    authorize(&state, &headers, &dataset).await?;
    let limits = state
        .export_service()
        .map_or_else(Default::default, |service| service.limits().clone());
    Ok(Json(labello_client::ExportCapabilities {
        available: state.export_service().is_some(),
        limits: serde_json::from_value(serde_json::to_value(limits)?)?,
    }))
}

async fn preflight(
    State(state): State<ApiState>,
    Path(dataset): Path<DatasetId>,
    headers: HeaderMap,
    Json(options): Json<ExportOptions>,
) -> ApiResult<Json<labello_client::ExportJob>> {
    authorize(&state, &headers, &dataset).await?;
    let job = service(&state)?
        .preflight(&dataset, state.repo(&dataset)?.as_ref().clone(), options)
        .await
        .map_err(map_failure)?;
    convert_job(job)
}

async fn list(
    State(state): State<ApiState>,
    Path(dataset): Path<DatasetId>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<labello_client::ExportJob>>> {
    authorize(&state, &headers, &dataset).await?;
    let jobs = service(&state)?.jobs(&dataset).await.map_err(map_failure)?;
    Ok(Json(
        jobs.into_iter()
            .map(|job| convert_job(job).map(|Json(job)| job))
            .collect::<ApiResult<_>>()?,
    ))
}

async fn status(
    State(state): State<ApiState>,
    Path((dataset, job)): Path<(DatasetId, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<labello_client::ExportJob>> {
    authorize(&state, &headers, &dataset).await?;
    convert_job(
        service(&state)?
            .job(&dataset, &job)
            .await
            .map_err(map_failure)?,
    )
}

async fn start(
    State(state): State<ApiState>,
    Path((dataset, job)): Path<(DatasetId, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<labello_client::ExportJob>> {
    authorize(&state, &headers, &dataset).await?;
    convert_job(
        service(&state)?
            .start(&dataset, &job)
            .await
            .map_err(map_failure)?,
    )
}

async fn cancel(
    State(state): State<ApiState>,
    Path((dataset, job)): Path<(DatasetId, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<labello_client::ExportJob>> {
    authorize(&state, &headers, &dataset).await?;
    convert_job(
        service(&state)?
            .cancel(&dataset, &job)
            .await
            .map_err(map_failure)?,
    )
}

async fn download(
    State(state): State<ApiState>,
    Path((dataset, job)): Path<(DatasetId, String)>,
    headers: HeaderMap,
    method: Method,
) -> ApiResult<Response> {
    authorize(&state, &headers, &dataset).await?;
    let (file, job, permit) = service(&state)?
        .download(&dataset, &job)
        .await
        .map_err(map_failure)?;
    // Recheck after checksum I/O so a revoked role/session cannot gain a new stream.
    authorize(&state, &headers, &dataset).await?;
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        let file = tokio::fs::File::from_std(file);
        let stream = futures::stream::try_unfold((file, permit), |(mut file, permit)| async move {
            use tokio::io::AsyncReadExt;
            let mut buffer = vec![0_u8; 64 * 1024];
            let count = file.read(&mut buffer).await?;
            if count == 0 {
                return Ok::<_, std::io::Error>(None);
            }
            buffer.truncate(count);
            Ok(Some((Bytes::from(buffer), (file, permit))))
        });
        Body::from_stream(stream)
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"labello-export-{}.zip\"", job.job_id),
            ),
            (
                header::CONTENT_LENGTH,
                job.archive_bytes
                    .ok_or_else(|| ApiError::Internal("invalid export artifact metadata".into()))?
                    .to_string(),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_owned()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        body,
    )
        .into_response())
}

fn convert_job(
    job: labello_storage::export::ExportJob,
) -> ApiResult<Json<labello_client::ExportJob>> {
    // Storage and client own separate contracts; the API checks their wire compatibility.
    Ok(Json(serde_json::from_value(serde_json::to_value(job)?)?))
}

fn map_failure(failure: ExportFailure) -> ApiError {
    let message = failure.to_string();
    match failure {
        ExportFailure::NotFound => ApiError::NotFound(message),
        ExportFailure::Busy | ExportFailure::NotReady | ExportFailure::SourceChanged => {
            ApiError::Conflict(message)
        }
        ExportFailure::Limit => ApiError::PayloadTooLarge(message),
        ExportFailure::Policy(_)
        | ExportFailure::InvalidInput
        | ExportFailure::AmbiguousObjects
        | ExportFailure::UnsupportedImage => ApiError::Unprocessable(message),
        ExportFailure::Cancelled | ExportFailure::Interrupted => ApiError::Conflict(message),
        ExportFailure::Storage | ExportFailure::Verification => ApiError::Internal(message),
    }
}

#[cfg(test)]
mod tests;
