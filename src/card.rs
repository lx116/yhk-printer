//! Deterministic photo-and-text card composition.

use crate::{
    document::Document,
    imaging::RasterImage,
    service::Settings,
    typeset::{self, Align, Font, TypesetOptions},
};
use std::{error::Error, fmt};

/// Card width, equal to the printer's printable width.
pub const WIDTH_DOTS: u16 = crate::imaging::PRINTER_WIDTH_DOTS;

#[derive(Clone, Debug, PartialEq)]
pub struct CardOptions {
    pub gap_rows: u16,
    pub cut_marks: bool,
    pub margin_dots: u16,
    pub back_offset_rows: i16,
    pub frame: bool,
    pub min_pt: f32,
    pub font: Font,
    pub size_pt: f32,
    pub align: Align,
}

impl Default for CardOptions {
    fn default() -> Self {
        Self {
            gap_rows: 40,
            cut_marks: true,
            margin_dots: 32,
            back_offset_rows: 0,
            frame: false,
            min_pt: 7.0,
            font: Font::Sans,
            size_pt: 10.0,
            align: Align::Left,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardError {
    TextDoesNotFit,
}

impl fmt::Display for CardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextDoesNotFit => formatter
                .write_str("Text does not fit the photo area: shorten it or use the Below layout"),
        }
    }
}

impl Error for CardError {}

impl From<&Settings> for CardOptions {
    fn from(settings: &Settings) -> Self {
        Self {
            gap_rows: settings.gap_rows(),
            cut_marks: settings.cut_marks,
            margin_dots: (settings.card_margin_mm.clamp(0.0, 24.0) * 8.0).round() as u16,
            back_offset_rows: (settings.back_offset_mm.clamp(-5.0, 5.0) * 8.0).round() as i16,
            frame: settings.frame,
            min_pt: settings.min_pt.max(7.0),
            font: settings.doc_font,
            size_pt: settings.doc_size_pt,
            align: settings.doc_align,
        }
    }
}

pub fn compose_below(
    photo: &RasterImage,
    document: &Document,
    options: &CardOptions,
) -> Vec<RasterImage> {
    let mut rows = compose_front(photo, options).data;
    for raster in render_text(document, options) {
        rows.extend(raster.data);
    }
    split_rows(rows)
}

pub fn compose_front(photo: &RasterImage, options: &CardOptions) -> RasterImage {
    assert_eq!(photo.width, WIDTH_DOTS, "card photos must be 384 dots wide");
    let mut data = photo.data.clone();
    let row_bytes = usize::from(WIDTH_DOTS / 8);
    data.extend(std::iter::repeat_n(
        0,
        usize::from(options.gap_rows) * row_bytes,
    ));
    if options.cut_marks && options.gap_rows > 0 {
        let mark_row = usize::from(photo.height) + usize::from(options.gap_rows) / 2;
        for x in cut_mark_pixels() {
            set_ink(&mut data, mark_row, x);
        }
    }
    RasterImage {
        width: WIDTH_DOTS,
        height: photo.height.saturating_add(options.gap_rows),
        data,
    }
}

pub fn compose_back(
    photo_height: u16,
    document: &Document,
    options: &CardOptions,
) -> Result<RasterImage, CardError> {
    let margin = options.margin_dots.min(WIDTH_DOTS / 2);
    let inner_height = photo_height.saturating_sub(margin.saturating_mul(2));
    let (text, _) = fit_text(document, inner_height, options)?;
    let mut data = vec![0; usize::from(photo_height) * usize::from(WIDTH_DOTS / 8)];
    let top = i32::from(margin) + i32::from(options.back_offset_rows);
    for source_y in 0..usize::from(text.height) {
        let destination_y = top + source_y as i32;
        if (0..i32::from(photo_height)).contains(&destination_y) {
            let source = source_y * 48;
            let destination = destination_y as usize * 48;
            data[destination..destination + 48].copy_from_slice(&text.data[source..source + 48]);
        }
    }
    if options.frame {
        for y in 0..usize::from(photo_height) {
            for x in 0..usize::from(WIDTH_DOTS) {
                if x < 2
                    || x >= usize::from(WIDTH_DOTS) - 2
                    || y < 2
                    || y >= usize::from(photo_height) - 2
                {
                    set_ink(&mut data, y, x);
                }
            }
        }
    }
    Ok(RasterImage {
        width: WIDTH_DOTS,
        height: photo_height,
        data,
    })
}

pub fn fit_text(
    document: &Document,
    height_dots: u16,
    options: &CardOptions,
) -> Result<(RasterImage, f32), CardError> {
    let mut size = options.size_pt.max(options.min_pt);
    loop {
        let mut candidate = options.clone();
        candidate.size_pt = size;
        let rendered = render_text(document, &candidate);
        let rendered_height = rendered
            .iter()
            .map(|image| usize::from(image.height))
            .sum::<usize>();
        if rendered_height <= usize::from(height_dots) {
            return Ok((join_rows(rendered), size));
        }
        if size <= options.min_pt {
            return Err(CardError::TextDoesNotFit);
        }
        size = (size - 0.5).max(options.min_pt);
    }
}

fn render_text(document: &Document, options: &CardOptions) -> Vec<RasterImage> {
    let margin = options.margin_dots.min(WIDTH_DOTS / 2);
    let options = TypesetOptions {
        font: options.font,
        size_pt: options.size_pt,
        align: options.align,
        margin_dots: margin,
        width_dots: WIDTH_DOTS,
        ..TypesetOptions::default()
    };
    typeset::typeset_to_bitmap(document, &options)
        .into_iter()
        .map(|image| {
            let mut data = vec![0; usize::from(image.height) * 48];
            for (index, pixel) in image.data.iter().enumerate() {
                if *pixel < 128 {
                    data[index / usize::from(WIDTH_DOTS) * 48
                        + (index % usize::from(WIDTH_DOTS)) / 8] |= 0x80 >> (index % 8);
                }
            }
            RasterImage {
                width: WIDTH_DOTS,
                height: image.height,
                data,
            }
        })
        .collect()
}

fn join_rows(images: Vec<RasterImage>) -> RasterImage {
    let data = images
        .into_iter()
        .flat_map(|image| image.data)
        .collect::<Vec<_>>();
    RasterImage {
        width: WIDTH_DOTS,
        height: u16::try_from(data.len() / 48).expect("fitted card text is at most u16 rows"),
        data,
    }
}

fn split_rows(data: Vec<u8>) -> Vec<RasterImage> {
    data.chunks(usize::from(u16::MAX) * 48)
        .map(|chunk| RasterImage {
            width: WIDTH_DOTS,
            height: (chunk.len() / 48) as u16,
            data: chunk.to_vec(),
        })
        .collect()
}

fn set_ink(data: &mut [u8], row: usize, x: usize) {
    data[row * 48 + x / 8] |= 0x80 >> (x % 8);
}

fn cut_mark_pixels() -> impl Iterator<Item = usize> {
    (0..usize::from(WIDTH_DOTS)).filter(|&x| {
        x < 6
            || x >= usize::from(WIDTH_DOTS) - 6
            || ((8..usize::from(WIDTH_DOTS) - 8).contains(&x) && ((x - 8) / 6).is_multiple_of(2))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::parse_text;

    fn photo(height: u16) -> RasterImage {
        let mut data = vec![0; usize::from(height) * 48];
        data[0] = 0b0010_0000;
        RasterImage {
            width: WIDTH_DOTS,
            height,
            data,
        }
    }

    fn ink(image: &RasterImage, x: usize, y: usize) -> bool {
        image.data[y * 48 + x / 8] & (0x80 >> (x % 8)) != 0
    }

    #[test]
    fn below_preserves_photo_pixels_then_cut_mark_and_document_pixels() {
        let image = compose_below(
            &photo(2),
            &parse_text("Card text"),
            &CardOptions {
                gap_rows: 4,
                ..CardOptions::default()
            },
        )
        .remove(0);

        assert!(ink(&image, 2, 0));
        assert!(ink(&image, 0, 4));
        assert!(ink(&image, 8, 4));
        assert!(!ink(&image, 14, 4));
        assert!(image.data[6 * 48..].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn back_is_photo_height_with_margin_frame_and_offset() {
        let image = compose_back(
            100,
            &parse_text("Card text"),
            &CardOptions {
                margin_dots: 32,
                frame: true,
                back_offset_rows: 5,
                ..CardOptions::default()
            },
        )
        .unwrap();

        assert_eq!((image.width, image.height), (WIDTH_DOTS, 100));
        assert!(ink(&image, 0, 0));
        assert!(ink(&image, 1, 99));
        assert!((32..37).all(|y| (32..352).all(|x| !ink(&image, x, y))));
        assert!((37..100).any(|y| (32..352).any(|x| ink(&image, x, y))));
    }

    #[test]
    fn fit_text_shrinks_in_half_point_steps_and_reports_overflow() {
        let options = CardOptions {
            size_pt: 10.0,
            min_pt: 7.0,
            ..CardOptions::default()
        };
        let (_, size) = fit_text(&parse_text("short text"), 40, &options).unwrap();
        assert_eq!(size, 8.5);
        assert_eq!(
            compose_back(10, &parse_text(&"text ".repeat(200)), &options),
            Err(CardError::TextDoesNotFit)
        );
        assert_eq!(
            CardError::TextDoesNotFit.to_string(),
            "Text does not fit the photo area: shorten it or use the Below layout"
        );
    }
}
