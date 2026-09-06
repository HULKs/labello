use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Serialize;

use crate::ApiState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentReadiness {
    ok: bool,
    service: &'static str,
    release_tag: &'static str,
    source_commit: &'static str,
    schema_version: u32,
    persistence: &'static str,
    authentication: &'static str,
}

pub(super) async fn readiness(State(state): State<ApiState>) -> impl IntoResponse {
    let persistence = persistence_ready(state.datasets_root()).await;
    let authentication = state.authentication_ready();
    let ok = persistence && authentication;
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(DeploymentReadiness {
            ok,
            service: "labello",
            release_tag: release_tag(),
            source_commit: source_commit(),
            schema_version: labello_domain::SCHEMA_VERSION,
            persistence: check_name(persistence),
            authentication: check_name(authentication),
        }),
    )
}

async fn persistence_ready(root: &std::path::Path) -> bool {
    let Ok(metadata) = tokio::fs::metadata(root).await else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return false;
    };
    entries.next_entry().await.is_ok()
}

const fn check_name(ready: bool) -> &'static str {
    if ready { "ok" } else { "failed" }
}

const fn release_tag() -> &'static str {
    match option_env!("LABELLO_RELEASE_TAG") {
        Some(value) => value,
        None => "development",
    }
}

const fn source_commit() -> &'static str {
    match option_env!("LABELLO_SOURCE_COMMIT") {
        Some(value) => value,
        None => "development",
    }
}

/// Compiled identity remains available when deployment admission fails.
pub(super) async fn build_information() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(labello_client::BuildIdentity::from_metadata(
            option_env!("LABELLO_RELEASE_TAG"),
            option_env!("LABELLO_SOURCE_COMMIT"),
        )),
    )
}
