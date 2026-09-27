use super::*;
use labello_client::{FeedbackDetail, FeedbackDismissal, FeedbackEntry, FeedbackThreshold};
use labello_domain::{EventId, TaskId};

fn reviewer(state: &ApiState, id: &labello_domain::UserId) -> ApiResult<String> {
    Ok(state
        .server_store
        .users()?
        .into_iter()
        .find(|a| &a.user_id == id)
        .and_then(|a| {
            a.github_login
                .map(|login| format!("@{login}"))
                .or_else(|| (!a.display_name.trim().is_empty()).then_some(a.display_name))
        })
        .unwrap_or_else(|| "Unknown reviewer".into()))
}
pub(super) async fn inbox(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<FeedbackEntry>>> {
    let actor = actor_from_headers(&state, &headers)?;
    let _guard = state.feedback.transaction().await;
    let items = state.feedback.inbox(&actor.user_id).await?;
    Ok(Json(
        items
            .into_iter()
            .map(|summary| {
                Ok(FeedbackEntry {
                    reviewer_name: reviewer(&state, &summary.reviewer)?,
                    summary,
                })
            })
            .collect::<ApiResult<_>>()?,
    ))
}
pub(super) async fn detail(
    State(state): State<ApiState>,
    Path(event): Path<EventId>,
    headers: HeaderMap,
) -> ApiResult<Json<FeedbackDetail>> {
    event.validate_path_segment()?;
    let actor = actor_from_headers(&state, &headers)?;
    let _guard = state.feedback.transaction().await;
    let feedback = state.feedback.detail(&actor.user_id, &event).await?;
    let repo = state.repo(&feedback.summary.dataset_id)?;
    let record = repo.load_image_record(&feedback.summary.image_id).await?;
    // Only issue presentation acknowledgement after the actual image can load.
    state
        .previews
        .get(&repo, &record, labello_storage::PreviewProfile::DataSaverV1)
        .await
        .map_err(|_| ApiError::Conflict("Feedback image could not be loaded. Retry.".into()))?;
    state
        .feedback
        .mark_presented(&actor.user_id, &event)
        .await?;
    Ok(Json(FeedbackDetail {
        reviewer_name: reviewer(&state, &feedback.summary.reviewer)?,
        feedback,
    }))
}
pub(super) async fn dismiss(
    State(state): State<ApiState>,
    Path(event): Path<EventId>,
    headers: HeaderMap,
    Json(body): Json<FeedbackDismissal>,
) -> ApiResult<Json<()>> {
    event.validate_path_segment()?;
    let actor = actor_from_headers(&state, &headers)?;
    let _guard = state.feedback.transaction().await;
    state
        .feedback
        .acknowledge(&actor.user_id, &event, body.viewed)
        .await?;
    Ok(Json(()))
}
async fn authorize_threshold(
    state: &ApiState,
    headers: &HeaderMap,
    dataset: &DatasetId,
    task: &TaskId,
) -> ApiResult<()> {
    let actor = actor_from_headers(state, headers)?;
    task.validate_path_segment()?;
    let metadata = state.repo(dataset)?.load_dataset_config().await?;
    ensure_dataset_role(&metadata, &actor, DatasetRole::DataAdmin)?;
    if metadata.task(task).is_none() {
        return Err(ApiError::Unprocessable("Unknown workflow".into()));
    }
    Ok(())
}
pub(super) async fn threshold(
    State(state): State<ApiState>,
    Path((dataset, task)): Path<(DatasetId, TaskId)>,
    headers: HeaderMap,
) -> ApiResult<Json<FeedbackThreshold>> {
    authorize_threshold(&state, &headers, &dataset, &task).await?;
    let _guard = state.feedback.transaction().await;
    Ok(Json(FeedbackThreshold {
        threshold: state.feedback.threshold(&dataset, &task).await?,
    }))
}
pub(super) async fn set_threshold(
    State(state): State<ApiState>,
    Path((dataset, task)): Path<(DatasetId, TaskId)>,
    headers: HeaderMap,
    Json(body): Json<FeedbackThreshold>,
) -> ApiResult<Json<FeedbackThreshold>> {
    authorize_threshold(&state, &headers, &dataset, &task).await?;
    let _guard = state.feedback.transaction().await;
    state
        .feedback
        .set_threshold(&dataset, &task, body.threshold)
        .await?;
    Ok(Json(body))
}

/// All workflow writers share the root admission guard with feedback dismissal.
/// Corrections can activate a cross-dataset gate before any subsequent write.
pub(super) async fn workflow_gate(
    State(state): State<ApiState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|p| p.as_str())
        .unwrap_or("");
    let mutation = request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.method() != Method::OPTIONS;
    let workflow = mutation
        && route.starts_with("/datasets/")
        && (route.contains("/assignments/")
            || route.contains("/images/")
            || route.ends_with("/offline-sync")
            || route.ends_with("/prelabel-accept"));
    if !workflow {
        return next.run(request).await;
    }
    let actor = match actor_from_headers(&state, request.headers()) {
        Ok(actor) => actor,
        // Handlers retain authentication and input-validation ownership.
        // Unauthenticated requests cannot label and need no feedback admission.
        Err(_) => return next.run(request).await,
    };
    let _guard = state.feedback.transaction().await;
    // Release/revalidation preserve leases and drafts without labeling. Cache
    // repair remains available; every annotation or review mutation is gated.
    let exempt = route.ends_with("/release")
        || route.ends_with("/assignments/revalidate")
        || route.ends_with("/rebuild");
    let result = if exempt {
        state.feedback.inbox(&actor.user_id).await.map(|_| ())
    } else {
        state
            .feedback
            .require_labeling_allowed(&actor.user_id)
            .await
    };
    if let Err(error) = result {
        return ApiError::from(error).into_response();
    }
    let correction = route.ends_with("/corrections") || route.ends_with("/review-corrections");
    let response = next.run(request).await;
    if correction && let Err(error) = state.feedback.observe_corrections().await {
        return ApiError::from(error).into_response();
    }
    response
}
