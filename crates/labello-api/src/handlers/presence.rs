use std::collections::BTreeMap;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header::CACHE_CONTROL},
    response::IntoResponse,
};
use labello_client::{PresenceDataset, PresentUser, ServerPresence};
use labello_domain::{DatasetId, UserId};

use crate::{ApiState, auth::actor_from_headers, error::ApiResult};

pub(super) async fn server_presence(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let actor = actor_from_headers(&state, &headers)?;
    // Authenticated presence intentionally spans datasets, including their names.
    // It does not confer access to any dataset or its assignments.
    let root = state.datasets_root();
    let io_error = |source| labello_storage::StorageError::Io {
        path: root.to_path_buf(),
        source,
    };
    let mut users: BTreeMap<UserId, Vec<PresenceDataset>> = BTreeMap::new();
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => Some(entries),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(labello_storage::StorageError::Io {
                path: root.to_path_buf(),
                source: error,
            }
            .into());
        }
    };
    while let Some(entries) = &mut entries {
        let Some(entry) = entries.next_entry().await.map_err(io_error)? else {
            break;
        };
        if !entry.file_type().await.map_err(io_error)?.is_dir()
            || entry.file_name() == ".labello-server"
        {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let dataset_id = DatasetId::from(name);
        if dataset_id.validate_path_segment().is_err() {
            continue;
        }
        // Ordinary non-dataset directories are not presence sources.
        if !tokio::fs::try_exists(entry.path().join("labello.dataset.toml"))
            .await
            .map_err(io_error)?
        {
            continue;
        }
        let repo = state.repo(&dataset_id)?;
        let metadata = repo.load_dataset_config().await?;
        for user_id in repo.active_lease_holders().await?.into_keys() {
            if user_id != actor.user_id {
                users.entry(user_id).or_default().push(PresenceDataset {
                    dataset_id: dataset_id.clone(),
                    name: metadata.name.clone(),
                });
            }
        }
    }
    let users = users
        .into_iter()
        .map(|(user_id, mut datasets)| {
            datasets.sort_by(|a, b| a.dataset_id.cmp(&b.dataset_id));
            PresentUser { user_id, datasets }
        })
        .collect();
    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(ServerPresence { users }),
    ))
}
