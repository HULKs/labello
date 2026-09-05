use std::time::Instant;

use axum::{
    body::Body,
    extract::MatchedPath,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use labello_domain::{Actor, DatasetId};
use tower_http::request_id::RequestId;
use tracing::Instrument;

#[derive(Clone)]
pub(crate) struct FailureDiagnostic {
    pub error_kind: &'static str,
    pub warn: bool,
}

pub(crate) fn record_actor(actor: &Actor) {
    if actor.user_id.validate_path_segment().is_ok() {
        tracing::Span::current().record("user_id", actor.user_id.as_str());
    }
}

pub(crate) fn record_dataset(dataset_id: &DatasetId) {
    if dataset_id.validate_path_segment().is_ok() {
        tracing::Span::current().record("dataset_id", dataset_id.as_str());
    }
}

// Normalize before SetRequestIdLayer so logs and the response always agree.
pub(crate) async fn normalize_request_id(mut request: Request<Body>, next: Next) -> Response {
    let ids = request.headers().get_all("x-request-id");
    let mut values = ids.iter();
    let valid = values.next().is_some_and(|value| {
        let bytes = value.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 128
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(byte))
    }) && values.next().is_none();
    if !valid {
        request.headers_mut().remove("x-request-id");
    }
    next.run(request).await
}

pub(crate) async fn observe_response(request: Request<Body>, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("<unmatched>");
    let discovery = request.method() == Method::GET && route == "/me";
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("<missing>");
    let method = match request.method().as_str() {
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "CONNECT" | "TRACE" => {
            request.method().as_str()
        }
        _ => "<other>",
    };
    let span = tracing::info_span!(
        "http.request",
        request_id,
        method,
        route,
        user_id = tracing::field::Empty,
        dataset_id = tracing::field::Empty,
    );
    async move {
        let started = Instant::now();
        let response = next.run(request).await;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let diagnostic = response.extensions().get::<FailureDiagnostic>();
            let error_kind = diagnostic.map_or_else(|| status_kind(status), |item| item.error_kind);
            let status_code = status.as_u16();
            if status.is_server_error() && !diagnostic.is_some_and(|item| item.warn) {
                tracing::error!(
                    event = "api.error",
                    error_kind,
                    status = status_code,
                    "request failed"
                );
            } else if discovery && status == StatusCode::UNAUTHORIZED {
                tracing::debug!(
                    event = "api.request.rejected",
                    error_kind,
                    status = status_code,
                    "request rejected"
                );
            } else if matches!(
                status,
                StatusCode::UNAUTHORIZED
                    | StatusCode::FORBIDDEN
                    | StatusCode::PAYLOAD_TOO_LARGE
                    | StatusCode::TOO_MANY_REQUESTS
            ) || diagnostic.is_some_and(|item| item.warn)
            {
                tracing::warn!(
                    event = "api.request.rejected",
                    error_kind,
                    status = status_code,
                    "request rejected"
                );
            } else {
                tracing::info!(
                    event = "api.request.rejected",
                    error_kind,
                    status = status_code,
                    "request rejected"
                );
            }
        }
        tracing::info!(
            event = "http.request.completed",
            status = status.as_u16(),
            latency_ms = started.elapsed().as_millis() as u64,
            "request completed"
        );
        response
    }
    .instrument(span)
    .await
}

fn status_kind(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::REQUEST_TIMEOUT => "request_timeout",
        StatusCode::CONFLICT => "conflict",
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::UNPROCESSABLE_ENTITY => "unprocessable_entity",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit",
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            "dependency_unavailable"
        }
        _ if status.is_server_error() => "internal",
        _ => "request_rejected",
    }
}
