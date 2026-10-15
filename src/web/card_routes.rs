//! Card preview and queue-commit HTTP routes with card rendering helpers.

use super::{
    errors::error,
    types::{png_base64, DOTS_PER_MM},
    WebState,
};
use crate::{
    card, document, imaging,
    service::{CardDraft, Flip, Settings},
    typeset::{Align, Font},
};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CardLayout {
    Below,
    Behind,
}
#[derive(Deserialize)]
#[serde(default)]
struct CardRequest {
    photo_base64: String,
    text: String,
    layout: CardLayout,
    flip: Flip,
    margin_mm: f32,
    font: Font,
    size_pt: f32,
    align: Align,
    gap_mm: f32,
    cut_marks: bool,
    frame: bool,
    back_offset_mm: f32,
    min_pt: f32,
}
impl Default for CardRequest {
    fn default() -> Self {
        Self {
            photo_base64: String::new(),
            text: String::new(),
            layout: CardLayout::Below,
            flip: Flip::Book,
            margin_mm: 4.0,
            font: Font::Sans,
            size_pt: 10.0,
            align: Align::Left,
            gap_mm: 5.0,
            cut_marks: true,
            frame: false,
            back_offset_mm: 0.0,
            min_pt: 7.0,
        }
    }
}
fn card_request(body: Bytes) -> Result<CardRequest, Box<Response>> {
    serde_json::from_slice(&body).map_err(|cause| {
        Box::new(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "card",
            format!("Invalid card request: {cause}"),
        ))
    })
}
fn decode_base64(value: &str) -> Result<Vec<u8>, &'static str> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if !value.len().is_multiple_of(4) {
        return Err("photo_base64 must be valid base64");
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let (chunks, []) = value.as_bytes().as_chunks::<4>() else {
        return Err("photo_base64 must be valid base64");
    };
    for (index, chunk) in chunks.iter().enumerate() {
        let padding = usize::from(chunk[2] == b'=') + usize::from(chunk[3] == b'=');
        if padding > 0 && index + 1 != value.len() / 4 || padding == 2 && chunk[2] != b'=' {
            return Err("photo_base64 must be valid base64");
        }
        let a = digit(chunk[0]).ok_or("photo_base64 must be valid base64")?;
        let b = digit(chunk[1]).ok_or("photo_base64 must be valid base64")?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            digit(chunk[2]).ok_or("photo_base64 must be valid base64")?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            digit(chunk[3]).ok_or("photo_base64 must be valid base64")?
        };
        let packed =
            (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((packed >> 16) as u8);
        if padding < 2 {
            output.push((packed >> 8) as u8);
        }
        if padding == 0 {
            output.push(packed as u8);
        }
    }
    Ok(output)
}
struct PreparedCard {
    front: Option<imaging::RasterImage>,
    back: Option<imaging::RasterImage>,
    below: Vec<imaging::RasterImage>,
    instruction: &'static str,
    fit_pt: f32,
}
fn prepare_card(request: &CardRequest) -> Result<PreparedCard, String> {
    let photo_bytes = decode_base64(&request.photo_base64).map_err(str::to_owned)?;
    let photo = imaging::prepare_image_with_options(
        image::load_from_memory(&photo_bytes).map_err(|error| error.to_string())?,
        card::WIDTH_DOTS,
        None,
        imaging::ProcessOptions::default(),
    )
    .map_err(|error| error.to_string())?
    .raster;
    let options = card::CardOptions {
        gap_rows: (request.gap_mm.clamp(0.0, 30.0) * DOTS_PER_MM).round() as u16,
        cut_marks: request.cut_marks,
        margin_dots: (request.margin_mm.clamp(0.0, 24.0) * DOTS_PER_MM).round() as u16,
        back_offset_rows: (request.back_offset_mm.clamp(-5.0, 5.0) * DOTS_PER_MM).round() as i16,
        frame: request.frame,
        min_pt: request.min_pt.max(7.0),
        font: request.font,
        size_pt: request.size_pt.max(request.min_pt.max(7.0)),
        align: request.align,
    };
    let document = document::parse_markdown(&request.text);
    let (front, back, below, fit_pt) = match request.layout {
        CardLayout::Below => (
            None,
            None,
            card::compose_below(&photo, &document, &options),
            0.0,
        ),
        CardLayout::Behind => {
            let front = card::compose_front(&photo, &options);
            let inner_height = photo
                .height
                .saturating_sub(options.margin_dots.saturating_mul(2));
            let (_, fit_pt) = card::fit_text(&document, inner_height, &options)
                .map_err(|error| error.to_string())?;
            let back = card::compose_back(photo.height, &document, &options)
                .map_err(|error| error.to_string())?;
            (Some(front), Some(back), vec![], fit_pt)
        }
    };
    let instruction = match request.flip { Flip::Book => "Cut along the dotted line. Turn the photo over like a page of a book (left to right) so the blank side faces the print head, insert the TOP edge first, then press Space.", Flip::Calendar => "Cut along the dotted line. Turn the photo over like a calendar (top to bottom) so the blank side faces the print head, insert the BOTTOM edge first, then press Space." };
    Ok(PreparedCard {
        front,
        back,
        below,
        instruction,
        fit_pt,
    })
}
fn card_preview_response(card: &PreparedCard, settings: &Settings) -> Result<Response, String> {
    if let Some(strip) = card.below.first() {
        let rows = u32::from(strip.height);
        let strip = serde_json::json!({"png_base64":png_base64(strip)?,"width":strip.width,"height":strip.height});
        return Ok(Json(serde_json::json!({"strip":strip,"mm":rows as f32 / DOTS_PER_MM,"est_seconds":crate::service::estimate_print_seconds(rows, settings),"instruction":card.instruction})).into_response());
    }
    let front = card.front.as_ref().expect("behind cards have a front");
    let back = card.back.as_ref().expect("behind cards have a back");
    let front_json = serde_json::json!({"png_base64":png_base64(front)?,"width":front.width,"height":front.height});
    let back_json =
        serde_json::json!({"png_base64":png_base64(back)?,"width":back.width,"height":back.height});
    Ok(Json(serde_json::json!({"front":front_json,"back":back_json,"mm":(u32::from(front.height) + u32::from(back.height)) as f32 / DOTS_PER_MM,"front_est_seconds":crate::service::estimate_print_seconds(u32::from(front.height), settings),"back_est_seconds":crate::service::estimate_print_seconds(u32::from(back.height), settings),"fit_pt":card.fit_pt,"instruction":card.instruction})).into_response())
}
pub(crate) async fn card_preview(State(state): State<WebState>, body: Bytes) -> Response {
    let request = match card_request(body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let card = match tokio::task::spawn_blocking(move || prepare_card(&request)).await {
        Ok(Ok(card)) => card,
        Ok(Err(message)) => return error(StatusCode::UNPROCESSABLE_ENTITY, "card", message),
        Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "card", cause),
    };
    let settings = state.service.settings().await.unwrap_or_default();
    card_preview_response(&card, &settings)
        .unwrap_or_else(|cause| error(StatusCode::UNPROCESSABLE_ENTITY, "png", cause))
}
pub(crate) async fn card_commit(State(state): State<WebState>, body: Bytes) -> Response {
    let request = match card_request(body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    let layout = request.layout;
    let card = match tokio::task::spawn_blocking(move || prepare_card(&request)).await {
        Ok(Ok(card)) => card,
        Ok(Err(message)) => return error(StatusCode::UNPROCESSABLE_ENTITY, "card", message),
        Err(cause) => return error(StatusCode::UNPROCESSABLE_ENTITY, "card", cause),
    };
    let jobs = match layout {
        CardLayout::Below => card.below,
        CardLayout::Behind => vec![
            card.front.expect("behind cards have a front"),
            card.back.expect("behind cards have a back"),
        ],
    };
    state
        .service
        .queue_card(CardDraft {
            layout: match layout {
                CardLayout::Below => "below",
                CardLayout::Behind => "behind",
            }
            .into(),
            jobs,
            instruction: card.instruction.into(),
        })
        .await
        .map(|item_id| {
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({"item_id":item_id})),
            )
                .into_response()
        })
        .unwrap_or_else(|cause| error(StatusCode::SERVICE_UNAVAILABLE, "service", cause))
}

#[cfg(test)]
mod tests {
    use super::super::{test_support::*, types::base64};
    use crate::{
        card::{self, CardOptions},
        document, imaging,
        typeset::{self, Align, Font, TypesetOptions},
    };
    use axum::{
        body::Body,
        http::{header, StatusCode},
    };
    use tower::ServiceExt;

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

    fn card_body(text: &str, layout: &str) -> Body {
        Body::from(serde_json::json!({"photo_base64": base64(&card_png()), "text": text, "layout": layout, "flip": "book", "margin_mm": 4.0, "font": "sans", "size_pt": 10.0, "gap_mm": 5.0, "cut_marks": true, "frame": false, "back_offset_mm": 0.0}).to_string())
    }

    #[tokio::test]
    async fn card_preview_returns_decodable_bitmaps_with_a_photo_sized_back() {
        let (service, _, path) = test_service().await;
        let response = app(service, None)
            .oneshot(request(
                "POST",
                "/v1/cards/preview",
                card_body("A short card", "behind"),
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
        let front = image::load_from_memory(&decode_base64(
            value["front"]["png_base64"].as_str().unwrap(),
        ))
        .unwrap()
        .to_luma8();
        let back = image::load_from_memory(&decode_base64(
            value["back"]["png_base64"].as_str().unwrap(),
        ))
        .unwrap()
        .to_luma8();
        assert_eq!(front.width(), 384);
        assert_eq!(
            back.height(),
            200,
            "back must match the decoded photo height"
        );
        assert_eq!(back.width(), 384);
        assert!(front.pixels().any(|pixel| pixel.0[0] == 0));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn card_preview_below_returns_the_exact_single_job_strip() {
        let (service, _, path) = test_service().await;
        let text = "A card preview must match its printed strip.";
        let app = app(service.clone(), None);
        let below: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(
                app.clone()
                    .oneshot(request(
                        "POST",
                        "/v1/cards/preview",
                        card_body(text, "below"),
                    ))
                    .await
                    .unwrap()
                    .into_body(),
                usize::MAX,
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let strip = image::load_from_memory(&decode_base64(
            below["strip"]["png_base64"].as_str().unwrap(),
        ))
        .unwrap()
        .to_luma8();
        assert!(below["front"].is_null());
        assert!(below["back"].is_null());

        let behind: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(
                app.clone()
                    .oneshot(request(
                        "POST",
                        "/v1/cards/preview",
                        card_body(text, "behind"),
                    ))
                    .await
                    .unwrap()
                    .into_body(),
                usize::MAX,
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let front = image::load_from_memory(&decode_base64(
            behind["front"]["png_base64"].as_str().unwrap(),
        ))
        .unwrap()
        .to_luma8();
        assert_eq!(&strip.as_raw()[..200 * 384], &front.as_raw()[..200 * 384]);
        let cut_row = 200 + 40 / 2;
        assert_eq!(strip.get_pixel(0, cut_row).0, [0]);
        assert_eq!(strip.get_pixel(8, cut_row).0, [0]);
        assert_eq!(strip.get_pixel(14, cut_row).0, [255]);
        assert!(strip.as_raw()[240 * 384..].contains(&0));

        let photo = imaging::prepare_image_with_options(
            image::load_from_memory(&card_png()).unwrap(),
            card::WIDTH_DOTS,
            None,
            imaging::ProcessOptions::default(),
        )
        .unwrap()
        .raster;
        let expected = card::compose_below(
            &photo,
            &document::parse_markdown(text),
            &CardOptions {
                gap_rows: 40,
                cut_marks: true,
                margin_dots: 32,
                back_offset_rows: 0,
                frame: false,
                min_pt: 7.0,
                font: Font::Sans,
                size_pt: 10.0,
                align: Align::Left,
            },
        )
        .remove(0);
        let text_rows: u16 = typeset::typeset_to_bitmap(
            &document::parse_markdown(text),
            &TypesetOptions {
                font: Font::Sans,
                size_pt: 10.0,
                align: Align::Left,
                margin_dots: 32,
                width_dots: card::WIDTH_DOTS,
                ..TypesetOptions::default()
            },
        )
        .iter()
        .map(|image| image.height)
        .sum();
        assert_eq!(expected.height, photo.height + 40 + text_rows);
        assert_eq!(strip.height(), u32::from(expected.height));

        let response = app
            .oneshot(request(
                "POST",
                "/v1/cards/commit",
                card_body(text, "below"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let committed = service.cards().await.pop().unwrap().jobs.remove(0);
        assert_eq!(committed, expected);
        assert_eq!(
            strip.as_raw(),
            super::super::types::bitmap(&committed).as_raw()
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn card_preview_below_accepts_text_that_overflows_behind() {
        let (service, _, path) = test_service().await;
        let response = app(service, None)
            .oneshot(request(
                "POST",
                "/v1/cards/preview",
                card_body(&"too much text ".repeat(100), "below"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn card_preview_reports_the_c2_overflow_error() {
        let (service, _, path) = test_service().await;
        let response = app(service, None)
            .oneshot(request(
                "POST",
                "/v1/cards/preview",
                card_body(&"too much text ".repeat(100), "behind"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["message"],
            "Text does not fit the photo area: shorten it or use the Below layout"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn card_commit_queues_one_item_without_printing() {
        let (service, writes, path) = test_service().await;
        let response = app(service.clone(), None)
            .oneshot(request(
                "POST",
                "/v1/cards/commit",
                card_body("Queue me", "below"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let value: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(value["item_id"].as_u64().is_some());
        assert_eq!(service.jobs().await.len(), 1);
        assert!(writes.lock().await.is_empty(), "committing must not print");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn card_routes_use_the_existing_host_origin_and_token_security() {
        let (service, _, path) = test_service().await;
        let app = app(service, Some("secret"));
        for route in ["/v1/cards/preview", "/v1/cards/commit"] {
            let mut bad_host = request("POST", route, card_body("x", "below"));
            bad_host
                .headers_mut()
                .insert(header::HOST, "attacker.example".parse().unwrap());
            assert_eq!(
                app.clone().oneshot(bad_host).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
            let mut bad_origin = request("POST", route, card_body("x", "below"));
            bad_origin
                .headers_mut()
                .insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
            assert_eq!(
                app.clone().oneshot(bad_origin).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                app.clone()
                    .oneshot(request("POST", route, card_body("x", "below")))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            let mut authorized = request("POST", route, card_body("x", "below"));
            authorized
                .headers_mut()
                .insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
            assert!(app
                .clone()
                .oneshot(authorized)
                .await
                .unwrap()
                .status()
                .is_success());
        }
        let _ = std::fs::remove_file(path);
    }
}
