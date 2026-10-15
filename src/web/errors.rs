//! JSON error response construction for web routes and middleware.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    message: String,
}

pub(crate) fn error(status: StatusCode, code: &str, message: impl ToString) -> Response {
    (
        status,
        Json(ErrorBody {
            error: code.into(),
            message: message.to_string(),
        }),
    )
        .into_response()
}
