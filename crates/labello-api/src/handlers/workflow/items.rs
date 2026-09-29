use super::*;
use labello_client::{ClaimWorkflowRequest, SaveWorkflowDraftRequest};
use labello_domain::{ImageState, WorkflowItem, WorkflowObject};

fn validate_action(request: &AssignmentActionRequest) -> ApiResult<()> {
    request.assignment_id.validate_path_segment()?;
    request.image_id.validate_path_segment()?;
    request.task_id.validate_path_segment()?;
    if !matches!(
        request.kind,
        AssignmentKind::Annotation | AssignmentKind::Review
    ) {
        return Err(ApiError::BadRequest("unsupported workflow kind".into()));
    }
    Ok(())
}

pub(crate) async fn workflow_availability(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Query(request): Query<AssignmentAvailabilityRequest>,
) -> ApiResult<Json<Vec<labello_domain::WorkflowAvailability>>> {
    if !matches!(
        request.kind,
        AssignmentKind::Annotation | AssignmentKind::Review
    ) {
        return Err(ApiError::BadRequest("unsupported workflow kind".into()));
    }
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        if request.kind == AssignmentKind::Review {
            DatasetRole::Reviewer
        } else {
            DatasetRole::Annotator
        },
    )?;
    state
        .synchronize_workflow_prelabels(&dataset_id, &repo, false)
        .await?;
    Ok(Json(
        repo.workflow_availability(&actor.user_id, request.kind)
            .await?,
    ))
}

fn validate_selection(selection: &labello_domain::WorkflowSelection) -> ApiResult<()> {
    selection.task_id.validate_path_segment()?;
    if !matches!(
        selection.kind,
        AssignmentKind::Annotation | AssignmentKind::Review
    ) {
        return Err(ApiError::BadRequest("unsupported workflow kind".into()));
    }
    Ok(())
}

pub(crate) async fn workflow_history(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Query(selection): Query<labello_domain::WorkflowSelection>,
) -> ApiResult<Json<Vec<labello_domain::WorkflowHistoryEntry>>> {
    validate_selection(&selection)?;
    let actor = actor_from_headers(&state, &headers)?;
    Ok(Json(
        state
            .repo(&dataset_id)?
            .workflow_history(&actor.user_id, &selection)
            .await?,
    ))
}

pub(crate) async fn leave_workflow(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(selection): Json<labello_domain::WorkflowSelection>,
) -> ApiResult<Json<()>> {
    validate_selection(&selection)?;
    let actor = actor_from_headers(&state, &headers)?;
    state
        .repo(&dataset_id)?
        .leave_workflow(&actor.user_id, &selection)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn reopen_workflow_item(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<AssignmentActionRequest>,
) -> ApiResult<Json<Assignment>> {
    validate_action(&request)?;
    let actor = actor_from_headers(&state, &headers)?;
    Ok(Json(
        state
            .repo(&dataset_id)?
            .reopen_workflow_item(
                &actor.user_id,
                AssignmentContext {
                    assignment_id: &request.assignment_id,
                    image_id: &request.image_id,
                    task_id: &request.task_id,
                    kind: request.kind,
                },
            )
            .await?,
    ))
}

pub(crate) async fn claim_workflow_item(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<ClaimWorkflowRequest>,
) -> ApiResult<Json<Option<Assignment>>> {
    request.selection.task_id.validate_path_segment()?;
    if request.excluded.len()
        > labello_domain::MAX_PRELOAD_QUEUE_SIZE + labello_domain::MAX_WORKFLOW_HISTORY_DEPTH + 1
        || !matches!(
            request.selection.kind,
            AssignmentKind::Annotation | AssignmentKind::Review
        )
    {
        return Err(ApiError::BadRequest("invalid workflow claim".into()));
    }
    for excluded in &request.excluded {
        excluded.image_id.validate_path_segment()?;
        match &excluded.item {
            WorkflowItem::Object {
                object: WorkflowObject::Annotation { annotation_id },
            } => {
                annotation_id.validate_path_segment()?;
            }
            WorkflowItem::Object {
                object: WorkflowObject::Migration { object_group_id },
            } => {
                object_group_id.validate_path_segment()?;
            }
            WorkflowItem::Object {
                object: WorkflowObject::Prelabel { suggestion_id },
            } if suggestion_id.is_empty() || suggestion_id.len() > 256 => {
                return Err(ApiError::BadRequest("invalid prediction identity".into()));
            }
            _ => {}
        }
    }
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    ensure_dataset_role(
        &repo.load_dataset_config().await?,
        &actor,
        if request.selection.kind == AssignmentKind::Annotation {
            DatasetRole::Annotator
        } else {
            DatasetRole::Reviewer
        },
    )?;
    state
        .synchronize_workflow_prelabels(&dataset_id, &repo, false)
        .await?;
    Ok(Json(
        repo.claim_workflow_item_with_prefetch(
            &actor.user_id,
            &request.selection,
            &request.excluded,
            request.prefetch,
        )
        .await?,
    ))
}

pub(crate) async fn display_workflow_item(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<AssignmentActionRequest>,
) -> ApiResult<Json<ImageState>> {
    validate_action(&request)?;
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    Ok(Json(
        repo.display_workflow_item(
            &actor.user_id,
            AssignmentContext {
                assignment_id: &request.assignment_id,
                image_id: &request.image_id,
                task_id: &request.task_id,
                kind: request.kind,
            },
        )
        .await?,
    ))
}

pub(crate) async fn save_workflow_draft(
    State(state): State<ApiState>,
    Path(dataset_id): Path<DatasetId>,
    headers: HeaderMap,
    Json(request): Json<SaveWorkflowDraftRequest>,
) -> ApiResult<Json<ImageState>> {
    validate_action(&request.assignment)?;
    let actor = actor_from_headers(&state, &headers)?;
    let repo = state.repo(&dataset_id)?;
    let assignment = AssignmentContext {
        assignment_id: &request.assignment.assignment_id,
        image_id: &request.assignment.image_id,
        task_id: &request.assignment.task_id,
        kind: request.assignment.kind,
    };
    let result = match request.draft {
        labello_client::WorkflowDraftInput::Geometry { geometry } => {
            repo.save_workflow_draft(
                &actor.user_id,
                assignment,
                geometry,
                request.expected_sequence,
            )
            .await?
        }
        labello_client::WorkflowDraftInput::Edits { edits } => {
            repo.save_workflow_edits(&actor.user_id, assignment, edits, request.expected_sequence)
                .await?
        }
    };
    Ok(Json(result))
}
