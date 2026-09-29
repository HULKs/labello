use super::*;
use labello_domain::{PrelabelAdminCommand, PrelabelAdminState};
use labello_storage::prelabel::PrelabelFailure;

pub(crate) fn failure(error: PrelabelFailure) -> ApiError {
    match error {
        PrelabelFailure::Storage => ApiError::Internal("prelabel storage failed".into()),
        PrelabelFailure::Invalid => ApiError::BadRequest(error.to_string()),
        PrelabelFailure::NotFound => ApiError::NotFound(error.to_string()),
        PrelabelFailure::Limit | PrelabelFailure::Busy => {
            ApiError::ResourceLimit(Box::new(ApiError::Conflict(error.to_string())))
        }
        PrelabelFailure::Stale | PrelabelFailure::Paused | PrelabelFailure::NotReady => {
            ApiError::Conflict(error.to_string())
        }
        PrelabelFailure::Inference
        | PrelabelFailure::ModelUnavailable
        | PrelabelFailure::ModelInvalid => ApiError::Unprocessable(error.to_string()),
    }
}

pub(super) async fn inspect_model(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<labello_client::PrelabelModelCheckRequest>,
) -> ApiResult<Json<labello_domain::PrelabelModelInspection>> {
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        DatasetRole::DataAdmin,
    )?;
    Ok(Json(
        state
            .prelabel_service()?
            .inspect_model(&request.location)
            .await
            .map_err(failure)?,
    ))
}

pub(super) async fn admin_state(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
) -> ApiResult<Json<PrelabelAdminState>> {
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        DatasetRole::DataAdmin,
    )?;
    state
        .synchronize_workflow_prelabels(&dataset_id, &repo, false)
        .await?;
    Ok(Json(
        state
            .prelabel_service()?
            .admin_state(&dataset_id)
            .await
            .map_err(failure)?,
    ))
}

pub(super) async fn admin_command(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(command): Json<PrelabelAdminCommand>,
) -> ApiResult<Json<PrelabelAdminState>> {
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        DatasetRole::DataAdmin,
    )?;
    Ok(Json(
        state
            .prelabel_service()?
            .command(&dataset_id, repo.as_ref().clone(), command)
            .await
            .map_err(failure)?,
    ))
}
