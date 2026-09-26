use super::*;
use labello_domain::{
    BrowserPrelabelResult, PrelabelAdminCommand, PrelabelAdminState, PrelabelGeneration,
    PrelabelResponse,
};
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

pub(super) async fn suggestions(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<labello_client::PrelabelSuggestionRequest>,
) -> ApiResult<Json<PrelabelResponse>> {
    request.image_id.validate_path_segment()?;
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    let metadata = repo.load_dataset_config().await?;
    ensure_dataset_role(&metadata, &actor, DatasetRole::Annotator)?;
    Ok(Json(
        state
            .prelabel_service()?
            .suggestions(
                &dataset_id,
                &repo,
                &actor.user_id,
                &request.image_id,
                &request.task_id,
                &request.config_id,
            )
            .await
            .map_err(failure)?,
    ))
}

pub(super) async fn generation(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Query(request): Query<labello_client::PrelabelSuggestionRequest>,
) -> ApiResult<Json<PrelabelGeneration>> {
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        DatasetRole::Annotator,
    )?;
    Ok(Json(
        state
            .prelabel_service()?
            .generation_status(&dataset_id, &request.task_id, &request.config_id)
            .await
            .map_err(failure)?,
    ))
}

pub(super) async fn browser_result(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(result): Json<BrowserPrelabelResult>,
) -> ApiResult<Json<PrelabelResponse>> {
    result.grant.image_id.validate_path_segment()?;
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        DatasetRole::Annotator,
    )?;
    Ok(Json(
        state
            .prelabel_service()?
            .certify_browser(&dataset_id, &repo, result)
            .await
            .map_err(failure)?,
    ))
}

pub(super) async fn model(
    State(state): State<ApiState>,
    Path((dataset_id, config_id)): Path<(DatasetId, labello_domain::PrelabelConfigId)>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    config_id.validate_path_segment()?;
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    let metadata = repo.load_dataset_config().await?;
    ensure_any_dataset_role(&metadata, &actor)?;
    let config = metadata
        .prelabel_configs
        .iter()
        .find(|c| {
            c.config_id == config_id
                && (c.available_to_annotators
                    || has_dataset_role(&metadata, &actor.user_id, &DatasetRole::DataAdmin))
        })
        .ok_or_else(|| ApiError::NotFound("model".into()))?;
    let bytes = state
        .prelabel_service()?
        .model(config)
        .await
        .map_err(failure)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
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
