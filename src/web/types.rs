//! Web request and response types, validation, size limits, and image helpers.

use crate::{
    document, imaging,
    typeset::{self, validate_document_size, Align, Font, TypesetOptions},
};
use axum::{body::Bytes, http::StatusCode, response::Response};
use serde::Deserialize;

pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const DOCUMENT_BODY_LIMIT: usize = 1024 * 1024;
pub(crate) const DOTS_PER_MM: f32 = imaging::DOTS_PER_MM;
pub(crate) const MAX_DOCUMENT_MARGIN_DOTS: u16 = 80;
pub(crate) const MIN_DOCUMENT_LINE_HEIGHT: f32 = 0.5;
pub(crate) const MAX_DOCUMENT_LINE_HEIGHT: f32 = 3.0;

#[derive(Clone)]
pub struct WebConfig {
    pub token: Option<String>,
    pub allow_origins: Vec<String>,
    pub listen_host: String,
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub(crate) struct DocumentOptions {
    pub(crate) font: Font,
    pub(crate) size_pt: f32,
    pub(crate) align: Align,
    pub(crate) margin: u16,
    pub(crate) line_height: f32,
    pub(crate) content: imaging::ContentKind,
    pub(crate) definition: imaging::Definition,
}
impl Default for DocumentOptions {
    fn default() -> Self {
        Self {
            font: Font::Sans,
            size_pt: 10.0,
            align: Align::Left,
            margin: 12,
            line_height: 1.25,
            content: imaging::ContentKind::Auto,
            definition: imaging::Definition::Standard,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct DocumentRequest {
    pub(crate) text: String,
    pub(crate) format: DocumentFormat,
    #[serde(default)]
    pub(crate) options: DocumentOptions,
    pub(crate) copies: Option<u16>,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DocumentFormat {
    Txt,
    Md,
}
impl DocumentRequest {
    pub(crate) fn options(&self) -> Result<TypesetOptions, &'static str> {
        validate_document_size(self.options.size_pt)?;
        if self.options.margin > MAX_DOCUMENT_MARGIN_DOTS {
            return Err("margin must be between 0 and 80 dots");
        }
        if !(MIN_DOCUMENT_LINE_HEIGHT..=MAX_DOCUMENT_LINE_HEIGHT)
            .contains(&self.options.line_height)
        {
            return Err("line_height must be between 0.5 and 3.0");
        }
        Ok(TypesetOptions {
            font: self.options.font,
            size_pt: self.options.size_pt,
            align: self.options.align,
            margin_dots: self.options.margin,
            line_height: self.options.line_height,
            ..TypesetOptions::default()
        })
    }
    pub(crate) fn document(&self) -> document::Document {
        match self.format {
            DocumentFormat::Txt => document::parse_text(&self.text),
            DocumentFormat::Md => document::parse_markdown(&self.text),
        }
    }
}

pub(crate) fn document_images(
    request: &DocumentRequest,
) -> Result<Vec<imaging::PreparedImage>, &'static str> {
    let options = request.options()?;
    let process = imaging::ProcessOptions {
        content: request.options.content,
        definition: request.options.definition,
    };
    typeset::typeset(&request.document(), &options)
        .into_iter()
        .map(|image| {
            let source = image::GrayImage::from_raw(
                u32::from(image.width),
                u32::from(image.height),
                image.data,
            )
            .ok_or("typesetter returned invalid dimensions")?;
            imaging::prepare_image_with_options(
                image::DynamicImage::ImageLuma8(source),
                imaging::PRINTER_WIDTH_DOTS,
                None,
                process,
            )
            .map_err(|_| "document image processing failed")
        })
        .collect()
}

pub(crate) fn bitmap(image: &imaging::RasterImage) -> image::GrayImage {
    image::GrayImage::from_fn(u32::from(image.width), u32::from(image.height), |x, y| {
        let index = y as usize * usize::from(image.width / 8) + x as usize / 8;
        image::Luma([if image.data[index] & (0x80 >> (x % 8)) == 0 {
            255
        } else {
            0
        }])
    })
}
pub(crate) fn png_base64(image: &imaging::RasterImage) -> Result<String, String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(bitmap(image))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(base64(&bytes.into_inner()))
}
pub(crate) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[(value >> 18) as usize & 63] as char);
        output.push(TABLE[(value >> 12) as usize & 63] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(value >> 6) as usize & 63] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[value as usize & 63] as char
        } else {
            '='
        });
    }
    output
}
pub(crate) fn parse_document_request(body: Bytes) -> Result<DocumentRequest, Box<Response>> {
    if body.len() > DOCUMENT_BODY_LIMIT {
        return Err(Box::new(super::errors::error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body",
            "Document body must not exceed 1 MiB",
        )));
    }
    serde_json::from_slice(&body).map_err(|cause| {
        Box::new(super::errors::error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "document",
            format!("Invalid document request: {cause}"),
        ))
    })
}
