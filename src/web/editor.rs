//! Editor asset delivery and the static OpenAPI document.

use axum::{http::header, response::IntoResponse, Json};

pub(crate) async fn editor() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../../assets/editor/index.html"),
    )
}
pub(crate) async fn openapi() -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"openapi":"3.0.0","info":{"title":"yhkp local API","version":env!("CARGO_PKG_VERSION")},"paths":{"/v1/health":{"get":{}},"/v1/status":{"get":{}},"/v1/connect":{"post":{}},"/v1/disconnect":{"post":{}},"/v1/print":{"post":{}},"/v1/jobs":{"get":{}},"/v1/jobs/{id}":{"get":{},"delete":{}},"/v1/feed":{"post":{}},"/v1/settings":{"get":{},"put":{}},"/v1/events":{"get":{}},"/v1/openapi.json":{"get":{}}}}),
    )
}
