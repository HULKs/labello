use super::*;
use labello_domain::{KeybindingSet, UserId};

// Unlike the setup list, this enumeration fails closed on unreadable datasets:
// skipping one would make an incomplete total appear complete.
async fn accessible_datasets(state: &ApiState, actor: &Actor) -> ApiResult<Vec<DatasetMetadata>> {
    let root = state.datasets_root();
    let io = |source| labello_storage::StorageError::Io {
        path: root.to_owned(),
        source,
    };
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(error).into()),
    };
    let mut datasets = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(io)? {
        if !entry.file_type().await.map_err(io)?.is_dir() || entry.file_name() == ".labello-server"
        {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let id = DatasetId::from(name);
        if id.validate_path_segment().is_err() {
            continue;
        }
        if !tokio::fs::try_exists(entry.path().join("labello.dataset.toml"))
            .await
            .map_err(io)?
        {
            continue;
        }
        let metadata = state.repo(&id)?.load_dataset_config().await?;
        if ensure_any_dataset_role(&metadata, actor).is_ok() {
            datasets.push(metadata);
        }
    }
    datasets.sort_by(|a, b| a.dataset_id.cmp(&b.dataset_id));
    Ok(datasets)
}

pub(super) async fn statistics(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let actor = actor_from_headers(&state, &headers)?;
    let mut result = Vec::new();
    let users = state.server_store.users()?;
    for metadata in accessible_datasets(&state, &actor).await? {
        let mut stats = state.repo(&metadata.dataset_id)?.dataset_stats().await?;
        if let Some(contributors) = &mut stats.contributors {
            for account in &users {
                if let Some(person) = contributors.get_mut(&account.user_id) {
                    person.display_name = account.display_name.clone();
                    person.github_user_id = account.github_user_id.clone();
                }
            }
        }
        result.push(labello_client::DatasetStatistics {
            dataset_id: metadata.dataset_id,
            name: metadata.name,
            tasks: metadata.tasks,
            classes: metadata.label_classes,
            imbalance: metadata.imbalance,
            stats,
        });
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(result)))
}

pub(super) async fn keybindings(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let actor = actor_from_headers(&state, &headers)?;
    let value = state
        .preferences
        .load(&actor.user_id)
        .await?
        .unwrap_or_else(|| KeybindingSet::defaults_for(actor.user_id));
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(value)))
}

pub(super) async fn legacy_keybindings(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let actor = actor_from_headers(&state, &headers)?;
    let mut legacy = Vec::new();
    if state.preferences.load(&actor.user_id).await?.is_none() {
        for metadata in accessible_datasets(&state, &actor).await? {
            let repo = state.repo(&metadata.dataset_id)?;
            if let Some(bindings) = repo.existing_keybindings(&actor.user_id).await? {
                legacy.push(labello_client::LegacyKeybindings {
                    dataset_id: metadata.dataset_id,
                    name: metadata.name,
                    bindings,
                });
            }
        }
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(legacy)))
}

pub(super) async fn save_keybindings(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(mut bindings): Json<KeybindingSet>,
) -> ApiResult<Json<KeybindingSet>> {
    let actor = actor_from_headers(&state, &headers)?;
    validate_bindings(&mut bindings, &actor.user_id)?;
    state.preferences.save(&bindings).await?;
    Ok(Json(bindings))
}

pub(super) fn validate_bindings(bindings: &mut KeybindingSet, user: &UserId) -> ApiResult<()> {
    if &bindings.user_id != user {
        return Err(ApiError::Unauthorized(
            "cannot edit another user's keybindings".into(),
        ));
    }
    user.validate_path_segment()?;
    labello_domain::validate_schema_version(bindings.schema_version)
        .map_err(labello_storage::StorageError::from)?;
    if labello_domain::UserAction::ACTIVE
        .into_iter()
        .all(|action| bindings.bindings.contains_key(&action))
    {
        bindings
            .validate()
            .map_err(labello_storage::StorageError::from)?;
    }
    bindings.normalize();
    bindings
        .validate()
        .map_err(labello_storage::StorageError::from)?;
    Ok(())
}
