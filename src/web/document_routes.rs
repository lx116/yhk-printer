//! Document preview, print, template, font, and expiring-draft HTTP routes.

use super::{
    errors::error,
    types::{
        document_images, parse_document_request, png_base64, DocumentRequest, DOCUMENT_BODY_LIMIT,
        DOTS_PER_MM,
    },
    WebState,
};
use crate::{protocol, service::PrintJob};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::Duration,
};

const DRAFT_TTL: Duration = Duration::from_secs(10 * 60);
static DRAFTS: OnceLock<tokio::sync::Mutex<HashMap<String, (std::time::Instant, String)>>> =
    OnceLock::new();
static NEXT_DRAFT_ID: AtomicU64 = AtomicU64::new(1);
fn drafts() -> &'static tokio::sync::Mutex<HashMap<String, (std::time::Instant, String)>> {
    DRAFTS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}
pub async fn create_draft(text: String) -> String {
    let mut drafts = drafts().lock().await;
    drafts.retain(|_, (created, _)| created.elapsed() < DRAFT_TTL);
    let id = format!(
        "{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_DRAFT_ID.fetch_add(1, Ordering::Relaxed)
    );
    drafts.insert(id.clone(), (std::time::Instant::now(), text));
    id
}

pub(crate) async fn document_preview(State(state): State<WebState>, body: Bytes) -> Response {
    let request = match parse_document_request(body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let images = match document_images_task(request).await {
        Ok(images) => images,
        Err(response) => return *response,
    };
    let rows: u32 = images
        .iter()
        .map(|image| u32::from(image.raster.height))
        .sum();
    let pages = match images.iter().map(|image| png_base64(&image.raster).map(|png_base64| serde_json::json!({"png_base64":png_base64,"width":crate::imaging::PRINTER_WIDTH_DOTS,"height":image.raster.height}))).collect::<Result<Vec<_>, _>>() { Ok(pages) => pages, Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "png", cause) };
    let settings = state.service.settings().await.unwrap_or_default();
    Json(serde_json::json!({"pages":pages,"rows":rows,"mm":rows as f32 / DOTS_PER_MM,"est_seconds":crate::service::estimate_print_seconds(rows, &settings)})).into_response()
}
pub(crate) async fn document_print(State(state): State<WebState>, body: Bytes) -> Response {
    let request = match parse_document_request(body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let copies = request.copies.unwrap_or(1);
    if copies == 0 {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "copies",
            "copies must be at least one",
        );
    }
    let images = match document_images_task(request).await {
        Ok(images) => images,
        Err(response) => return *response,
    };
    if images.is_empty() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "document",
            "Document has no printable content",
        );
    }
    let settings = state.service.settings().await.unwrap_or_default();
    let encoded = match tokio::task::spawn_blocking(move || {
        protocol::encode_batch(&images, settings.mode, settings.density, 0)
    })
    .await
    {
        Ok(Ok(encoded)) => encoded,
        Ok(Err(cause)) => return error(StatusCode::UNPROCESSABLE_ENTITY, "document", cause),
        Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "document", cause),
    };
    let mut job_id = 0;
    for _ in 0..copies {
        match state.service.print(PrintJob::from(encoded.clone())).await {
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
async fn document_images_task(
    request: DocumentRequest,
) -> Result<Vec<crate::imaging::PreparedImage>, Box<Response>> {
    match tokio::task::spawn_blocking(move || document_images(&request)).await {
        Ok(Ok(images)) => Ok(images),
        Ok(Err(message)) => Err(Box::new(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "options",
            message,
        ))),
        Err(cause) => Err(Box::new(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "document",
            cause,
        ))),
    }
}
pub(crate) async fn fonts() -> Json<Vec<&'static str>> {
    Json(vec!["sans", "hand", "mono"])
}
pub(crate) async fn templates() -> Json<Vec<serde_json::Value>> {
    Json(vec![
        serde_json::json!({"name":"Letter","text":"# Dear friend\n\nThank you for your note.\n\nSincerely,"}),
        serde_json::json!({"name":"Note","text":"# Note\n\nRemember to bring the receipt."}),
        serde_json::json!({"name":"List","text":"# Shopping list\n\n- Paper\n- Tea\n- Tape"}),
    ])
}
#[derive(serde::Deserialize)]
struct DraftRequest {
    text: String,
}
pub(crate) async fn document_draft(body: Bytes) -> Response {
    if body.len() > DOCUMENT_BODY_LIMIT {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body",
            "Draft body must not exceed 1 MiB",
        );
    }
    match serde_json::from_slice::<DraftRequest>(&body) {
        Ok(request) => {
            Json(serde_json::json!({"draft_id":create_draft(request.text).await})).into_response()
        }
        Err(cause) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "draft",
            format!("Invalid draft request: {cause}"),
        ),
    }
}
pub(crate) async fn document_draft_get(Path(id): Path<String>) -> Response {
    let mut drafts = drafts().lock().await;
    drafts.retain(|_, (created, _)| created.elapsed() < DRAFT_TTL);
    drafts
        .get(&id)
        .map(|(_, text)| Json(serde_json::json!({"text":text})).into_response())
        .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "draft", "Draft not found or expired"))
}

#[cfg(test)]
mod tests {
    use super::super::{
        test_support::*,
        types::{bitmap, document_images, DocumentRequest, DOCUMENT_BODY_LIMIT},
    };
    use crate::{
        protocol,
        service::{PrintMode, Settings},
    };
    use axum::{
        body::Body,
        http::{header, StatusCode},
    };
    use tower::ServiceExt;

    fn document_body(text: &str) -> Body {
        Body::from(serde_json::json!({"text": text, "format": "md", "options": {"font": "sans", "size_pt": 10.0, "align": "left", "margin": 12, "line_height": 1.25, "content": "text", "definition": "standard"}}).to_string())
    }

    fn decode_base64(value: &str) -> Vec<u8> {
        let decode = |byte| match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        };
        value
            .as_bytes()
            .chunks(4)
            .flat_map(|chunk| {
                let packed = (u32::from(decode(chunk[0])) << 18)
                    | (u32::from(decode(chunk[1])) << 12)
                    | (u32::from(decode(*chunk.get(2).unwrap_or(&b'A'))) << 6)
                    | u32::from(decode(*chunk.get(3).unwrap_or(&b'A')));
                let count = 3
                    - usize::from(chunk.get(2) == Some(&b'='))
                    - usize::from(chunk.get(3) == Some(&b'='));
                [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8]
                    .into_iter()
                    .take(count)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[tokio::test]
    async fn document_preview_has_typeset_pixels_and_exact_printed_bitmap() {
        let (service, _, path) = test_service().await;
        let source = "# First paragraph\n\nThis paragraph is deliberately long enough to produce visible printed ink.\n\nLast paragraph stays visible.";
        let response = app(service, None)
            .oneshot(request(
                "POST",
                "/v1/documents/preview",
                document_body(source),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let expected_request: DocumentRequest = serde_json::from_value(serde_json::json!({"text":source,"format":"md","options":{"font":"sans","size_pt":10.0,"align":"left","margin":12,"line_height":1.25,"content":"text","definition":"standard"}})).unwrap();
        let expected = document_images(&expected_request).unwrap();
        assert_eq!(
            value["rows"].as_u64(),
            Some(
                expected
                    .iter()
                    .map(|page| u64::from(page.raster.height))
                    .sum()
            )
        );
        let png = decode_base64(value["pages"][0]["png_base64"].as_str().unwrap());
        let decoded = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!(decoded.width(), 384);
        assert!(decoded.pixels().any(|pixel| pixel.0[0] == 0));
        let rows_with_ink = decoded
            .as_raw()
            .chunks(usize::try_from(decoded.width()).unwrap())
            .enumerate()
            .filter(|(_, row)| row.contains(&0))
            .map(|(row, _)| row)
            .collect::<Vec<_>>();
        assert!(rows_with_ink.last().unwrap() * 4 > decoded.height() as usize * 3);
        let expected_bitmap = bitmap(&expected[0].raster);
        assert_eq!(decoded.as_raw(), expected_bitmap.as_raw());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn document_print_queues_the_normal_encoded_job() {
        let (service, writes, path) = test_service().await;
        let mut value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(document_body("Print this paragraph."), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        value["copies"] = serde_json::json!(1);
        let request_data: DocumentRequest = serde_json::from_value(value.clone()).unwrap();
        let expected = protocol::encode_batch(
            &document_images(&request_data).unwrap(),
            PrintMode::Rows,
            Settings::default().density,
            0,
        )
        .unwrap()
        .bytes;
        let response = app(service.clone(), None)
            .oneshot(request(
                "POST",
                "/v1/documents/print",
                Body::from(value.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let id = serde_json::from_slice::<serde_json::Value>(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap()["job_id"]
            .as_u64()
            .unwrap();
        assert_eq!(
            wait_for_job(&service, id).await.status,
            crate::service::JobStatus::Done
        );
        assert_eq!(writes.lock().await[2..].concat(), expected);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn document_routes_validate_size_security_assets_and_drafts() {
        let (service, _, path) = test_service().await;
        let unsecured = app(service.clone(), None);
        let app = app(service, Some("secret"));
        let bad_options = unsecured
            .clone()
            .oneshot(request(
                "POST",
                "/v1/documents/preview",
                Body::from(r#"{"text":"x","format":"txt","options":{"size_pt":0}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(bad_options.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let mut authorized = request("POST", "/v1/documents/preview", document_body("x"));
        authorized
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let preview = app.clone().oneshot(authorized).await.unwrap();
        assert_eq!(preview.status(), StatusCode::OK);
        let mut font_request = request("GET", "/v1/fonts", Body::empty());
        font_request
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let fonts = app.clone().oneshot(font_request).await.unwrap();
        assert!(String::from_utf8(
            axum::body::to_bytes(fonts.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("mono"));
        let mut template_request = request("GET", "/v1/documents/templates", Body::empty());
        template_request
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let templates = app.clone().oneshot(template_request).await.unwrap();
        assert!(String::from_utf8(
            axum::body::to_bytes(templates.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("Letter"));
        let mut hostile = request("GET", "/editor", Body::empty());
        hostile
            .headers_mut()
            .insert(header::HOST, "bad.host".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(hostile).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let mut origin = request("GET", "/v1/fonts", Body::empty());
        origin
            .headers_mut()
            .insert(header::ORIGIN, "bad".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(origin).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let mut too_large = request(
            "POST",
            "/v1/documents/preview",
            Body::from(vec![b'x'; DOCUMENT_BODY_LIMIT + 1]),
        );
        too_large
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(too_large).await.unwrap().status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let mut page = request("GET", "/editor", Body::empty());
        page.headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let html = String::from_utf8(
            axum::body::to_bytes(
                app.clone().oneshot(page).await.unwrap().into_body(),
                usize::MAX,
            )
            .await
            .unwrap()
            .to_vec(),
        )
        .unwrap();
        assert!(!html.contains("http://") && !html.contains("https://"));
        let mut draft = request(
            "POST",
            "/v1/documents/draft",
            Body::from(r#"{"text":"saved"}"#),
        );
        draft
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        draft
            .headers_mut()
            .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        let draft_response = serde_json::from_slice::<serde_json::Value>(
            &axum::body::to_bytes(
                app.clone().oneshot(draft).await.unwrap().into_body(),
                usize::MAX,
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let id = draft_response["draft_id"].as_str().unwrap().to_owned();
        let mut read = request("GET", &format!("/v1/documents/draft/{id}"), Body::empty());
        read.headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        let draft = app.clone().oneshot(read).await.unwrap();
        assert_eq!(draft.status(), StatusCode::OK);
        assert!(String::from_utf8(
            axum::body::to_bytes(draft.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("saved"));
        super::drafts().lock().await.insert(
            "expired".into(),
            (std::time::Instant::now() - super::DRAFT_TTL, "gone".into()),
        );
        let mut expired = request("GET", "/v1/documents/draft/expired", Body::empty());
        expired
            .headers_mut()
            .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert_eq!(
            app.oneshot(expired).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let _ = std::fs::remove_file(path);
    }
}
