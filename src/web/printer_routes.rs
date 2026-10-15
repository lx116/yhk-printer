//! Printer-control HTTP routes, print submission, job management, and SSE events.

use super::{errors::error, WebState};
use crate::{
    imaging,
    service::{PrintJob, PrintMode, Quality, Settings},
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures::StreamExt;
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok":true,"version":env!("CARGO_PKG_VERSION")}))
}
pub(crate) async fn status(State(state): State<WebState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"state":state.service.state().borrow().clone(),"settings":state.service.settings().await.unwrap_or_default(),"queue_length":state.service.jobs().await.len()}),
    )
}
pub(crate) async fn connect(State(state): State<WebState>) -> Response {
    state
        .service
        .connect()
        .await
        .map(|_| StatusCode::ACCEPTED.into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}
pub(crate) async fn disconnect(State(state): State<WebState>) -> Response {
    state
        .service
        .disconnect()
        .await
        .map(|_| StatusCode::NO_CONTENT.into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}

#[derive(Deserialize)]
pub(crate) struct PrintOptions {
    mode: Option<PrintMode>,
    density: Option<u8>,
    quality: Option<WebQuality>,
    content: Option<imaging::ContentKind>,
    definition: Option<imaging::Definition>,
    copies: Option<u16>,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum WebQuality {
    Low,
    Medium,
    High,
}

pub(crate) async fn print(
    State(state): State<WebState>,
    Query(options): Query<PrintOptions>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let bytes = match print_bytes(&headers, body) {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    let mut settings = state.service.settings().await.unwrap_or_default();
    apply_print_options(&mut settings, &options);
    let process_options = imaging::ProcessOptions {
        content: settings.content,
        definition: settings.definition,
    };
    let image = match tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes)?;
        imaging::prepare_image_with_options(
            image,
            imaging::PRINTER_WIDTH_DOTS,
            None,
            process_options,
        )
    })
    .await
    {
        Ok(Ok(image)) => image,
        Ok(Err(cause)) => return error(StatusCode::UNPROCESSABLE_ENTITY, "image", cause),
        Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "image", cause),
    };
    let job = match settings.mode.encode(&image, settings.density) {
        Ok(job) => PrintJob::from(job),
        Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "image", cause),
    };
    let mut job_id = 0;
    for _ in 0..options.copies.unwrap_or(1).max(1) {
        match state.service.print(job.clone()).await {
            Ok(id) => job_id = id,
            Err(cause) => return error(StatusCode::SERVICE_UNAVAILABLE, "service", cause),
        }
    }
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"job_id":job_id})),
    )
        .into_response()
}

fn print_bytes(headers: &HeaderMap, body: Bytes) -> Result<Vec<u8>, Box<Response>> {
    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    else {
        return Err(Box::new(media_error()));
    };
    if content_type.starts_with("image/") {
        return Ok(body.to_vec());
    }
    if content_type.starts_with("multipart/form-data") {
        return multipart_image(content_type, &body).ok_or_else(|| {
            Box::new(error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "media",
                "multipart body needs an image field",
            ))
        });
    }
    Err(Box::new(media_error()))
}
fn media_error() -> Response {
    error(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "media",
        "Content-Type must be image/* or multipart/form-data",
    )
}
fn apply_print_options(settings: &mut Settings, options: &PrintOptions) {
    if let Some(mode) = &options.mode {
        settings.mode = mode.clone();
    }
    if let Some(density) = options.density {
        settings.density = density;
    } else if let Some(quality) = &options.quality {
        settings.density = match quality {
            WebQuality::Low => Quality::Low.density(),
            WebQuality::Medium => Quality::Medium.density(),
            WebQuality::High => Quality::High.density(),
        };
    }
    if let Some(content) = options.content {
        settings.content = content;
    }
    if let Some(definition) = options.definition {
        settings.definition = definition;
    }
}
fn multipart_image(content_type: &str, body: &[u8]) -> Option<Vec<u8>> {
    let boundary = content_type.split("boundary=").nth(1)?.trim_matches('"');
    let marker = format!("--{boundary}").into_bytes();
    let mut next_marker = b"\r\n".to_vec();
    next_marker.extend_from_slice(&marker);
    let mut offset = 0;
    while let Some(start) = body[offset..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let start = offset + start + marker.len();
        let part = &body[start..];
        let header_end = part.windows(4).position(|window| window == b"\r\n\r\n")?;
        let headers = std::str::from_utf8(&part[..header_end]).ok()?;
        let data_start = start + header_end + 4;
        let next = body[data_start..]
            .windows(next_marker.len())
            .position(|window| window == next_marker)?;
        if headers.contains("name=\"image\"") {
            return Some(body[data_start..data_start + next].to_vec());
        }
        offset = data_start + next;
    }
    None
}
pub(crate) async fn jobs(State(state): State<WebState>) -> Json<Vec<crate::service::JobInfo>> {
    Json(state.service.jobs().await)
}
pub(crate) async fn job(State(state): State<WebState>, Path(id): Path<u64>) -> Response {
    state
        .service
        .jobs()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .map(|job| Json(job).into_response())
        .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "job", "Job not found"))
}
pub(crate) async fn cancel(State(state): State<WebState>, Path(id): Path<u64>) -> Response {
    state
        .service
        .cancel(id)
        .await
        .map(|_| StatusCode::NO_CONTENT.into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}
#[derive(Deserialize)]
pub(crate) struct Feed {
    lines: usize,
}
pub(crate) async fn feed(State(state): State<WebState>, Json(request): Json<Feed>) -> Response {
    state
        .service
        .feed(request.lines)
        .await
        .map(|_| StatusCode::ACCEPTED.into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}
pub(crate) async fn settings(State(state): State<WebState>) -> Response {
    state
        .service
        .settings()
        .await
        .map(|s| Json(s).into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}
pub(crate) async fn set_settings(
    State(state): State<WebState>,
    Json(settings): Json<Settings>,
) -> Response {
    state
        .service
        .set_settings(settings)
        .await
        .map(|s| Json(s).into_response())
        .unwrap_or_else(|e| error(StatusCode::SERVICE_UNAVAILABLE, "service", e))
}
pub(crate) async fn events(
    State(state): State<WebState>,
) -> Sse<impl futures::Stream<Item = Result<SseEvent, Infallible>>> {
    Sse::new(
        BroadcastStream::new(state.service.subscribe()).filter_map(|result| async move {
            result
                .ok()
                .and_then(|event| serde_json::to_string(&event).ok())
                .map(|data| Ok(SseEvent::default().event("event").data(data)))
        }),
    )
    .keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use crate::service::JobStatus;
    use axum::{body::Body, http::StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_and_status_are_available() {
        let (service, _, path) = test_service().await;
        let app = app(service, None);
        let health = app
            .clone()
            .oneshot(request("GET", "/v1/health", Body::empty()))
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let status = app
            .oneshot(request("GET", "/v1/status", Body::empty()))
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn print_defaults_to_rows_encoding_and_finishes() {
        let (service, writes, path) = test_service().await;
        let image = png();
        let expected = crate::protocol::encode_rows(
            &crate::imaging::prepare_image_with_options(
                image::load_from_memory(&image).unwrap(),
                384,
                None,
                crate::imaging::ProcessOptions::default(),
            )
            .unwrap()
            .raster,
            crate::service::Settings::default().density,
        )
        .unwrap()
        .bytes;
        let mut print = request("POST", "/v1/print", Body::from(image));
        print.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            "image/png".parse().unwrap(),
        );
        let response = app(service.clone(), None).oneshot(print).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["job_id"]
            .as_u64()
            .unwrap();
        let job = wait_for_job(&service, id).await;
        assert_eq!(job.status, JobStatus::Done);
        assert_eq!(job.bytes_total, expected.len());
        assert_eq!(writes.lock().await[2..].concat(), expected);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn print_rejects_oversized_wrong_type_and_undecodable_bodies() {
        let (service, _, path) = test_service().await;
        let app = app(service, None);
        let too_large = app
            .clone()
            .oneshot(request(
                "POST",
                "/v1/print",
                Body::from(vec![0; 10 * 1024 * 1024 + 1]),
            ))
            .await
            .unwrap();
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let mut wrong_type = request("POST", "/v1/print", Body::from("not an image"));
        wrong_type.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            "text/plain".parse().unwrap(),
        );
        assert_eq!(
            app.clone().oneshot(wrong_type).await.unwrap().status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        let mut bad_image = request("POST", "/v1/print", Body::from("not an image"));
        bad_image.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            "image/png".parse().unwrap(),
        );
        assert_eq!(
            app.oneshot(bad_image).await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sse_streams_a_job_finished_event() {
        use futures::StreamExt;
        let (service, _, path) = test_service().await;
        let response = app(service.clone(), None)
            .oneshot(request("GET", "/v1/events", Body::empty()))
            .await
            .unwrap();
        let mut stream = response.into_body().into_data_stream();
        let id = service
            .print(crate::service::PrintJob {
                bytes: vec![1],
                row_ends: vec![],
            })
            .await
            .unwrap();
        assert_eq!(wait_for_job(&service, id).await.status, JobStatus::Done);
        let mut saw_finished = false;
        for _ in 0..8 {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if std::str::from_utf8(&chunk)
                .unwrap()
                .contains("job_finished")
            {
                saw_finished = true;
                break;
            }
        }
        assert!(saw_finished);
        let _ = std::fs::remove_file(path);
    }
}
