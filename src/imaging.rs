//! Image preparation for the printer's fixed raster geometry.

use anyhow::{bail, Result};
use image::{imageops::FilterType, DynamicImage, GrayImage as ImageBuffer, Luma};
use serde::{Deserialize, Serialize};

/// The printer's fixed printable width in dots.
pub const PRINTER_WIDTH_DOTS: u16 = 384;
/// Physical printer resolution used for millimetre conversions.
pub const DOTS_PER_MM: f32 = 8.0;
/// Number of dots represented by one packed raster byte.
pub const DOTS_PER_BYTE: u16 = 8;
pub const MAX_HEIGHT: u32 = 16_000;
pub const MAX_ROWS: u32 = MAX_HEIGHT;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    #[default]
    Auto,
    Photo,
    Text,
    Qr,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Definition {
    #[default]
    Standard,
    Hd,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessOptions {
    pub content: ContentKind,
    pub definition: Definition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterImage {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub gray: GrayImage,
    pub raster: RasterImage,
}

pub fn load_image(path: &std::path::Path, width: u16, gamma: Option<f32>) -> Result<PreparedImage> {
    prepare_image(image::open(path)?, width, gamma)
}

pub fn load_image_with_options(
    path: &std::path::Path,
    width: u16,
    gamma: Option<f32>,
    options: ProcessOptions,
) -> Result<PreparedImage> {
    prepare_image_with_options(image::open(path)?, width, gamma, options)
}

/// Legacy entry point: the photo path remains byte-for-byte compatible.
pub fn prepare_image(image: DynamicImage, width: u16, gamma: Option<f32>) -> Result<PreparedImage> {
    prepare_image_with_options(
        image,
        width,
        gamma,
        ProcessOptions {
            content: ContentKind::Photo,
            definition: Definition::Standard,
        },
    )
}

pub fn prepare_image_with_options(
    image: DynamicImage,
    width: u16,
    gamma: Option<f32>,
    options: ProcessOptions,
) -> Result<PreparedImage> {
    let gray = image.to_luma8();
    validate_dimensions(&gray, width)?;
    let content = match options.content {
        ContentKind::Auto => classify(&gray),
        kind => kind,
    };
    match content {
        ContentKind::Photo => prepare_photo(gray, width, gamma, options.definition),
        ContentKind::Text => prepare_text(gray, width, options.definition),
        ContentKind::Qr => prepare_qr(gray, width),
        ContentKind::Auto => unreachable!(),
    }
}

/// Classifies only image data; no dimensions or printer state are involved.
pub fn classify(image: &ImageBuffer) -> ContentKind {
    let bilevel = image
        .pixels()
        .filter(|pixel| pixel.0[0] <= 40 || pixel.0[0] >= 215)
        .count();
    if bilevel * 100 < image.as_raw().len() * 85 {
        return ContentKind::Photo;
    }
    let square = image.width().abs_diff(image.height()) <= image.width().max(image.height()) / 10;
    if square && module_size(image).is_some() {
        ContentKind::Qr
    } else {
        ContentKind::Text
    }
}

fn validate_dimensions(image: &ImageBuffer, width: u16) -> Result<()> {
    if image.width() == 0 || image.height() == 0 {
        bail!("image dimensions must be non-zero");
    }
    if width == 0 || !width.is_multiple_of(8) {
        bail!("printer width must be a non-zero multiple of eight");
    }
    Ok(())
}

fn resized(image: &ImageBuffer, width: u16) -> Result<ImageBuffer> {
    let height = ((u64::from(image.height()) * u64::from(width) + u64::from(image.width()) / 2)
        / u64::from(image.width()))
    .max(1) as u32;
    if height > MAX_HEIGHT {
        bail!("image height after resizing must be between 1 and {MAX_HEIGHT} rows");
    }
    Ok(image::imageops::resize(
        image,
        u32::from(width),
        height,
        FilterType::Lanczos3,
    ))
}

fn prepare_photo(
    image: ImageBuffer,
    width: u16,
    gamma: Option<f32>,
    definition: Definition,
) -> Result<PreparedImage> {
    let mut image = resized(&image, width)?;
    auto_contrast(&mut image);
    if let Some(gamma) = gamma {
        if gamma <= 0.0 {
            bail!("gamma must be greater than zero");
        }
        for pixel in image.pixels_mut() {
            pixel.0[0] = ((f32::from(pixel.0[0]) / 255.0).powf(1.0 / gamma) * 255.0).round() as u8;
        }
    }
    if definition == Definition::Hd {
        unsharp(&mut image);
    }
    pack(image)
}

fn prepare_text(image: ImageBuffer, width: u16, definition: Definition) -> Result<PreparedImage> {
    let mut image = resized(&image, width)?;
    unsharp(&mut image);
    let threshold = otsu_threshold(&image);
    if definition == Definition::Hd {
        sauvola(&mut image, 15);
    } else {
        threshold_image(&mut image, threshold);
    }
    pack(image)
}

fn prepare_qr(image: ImageBuffer, width: u16) -> Result<PreparedImage> {
    let module =
        module_size(&image).ok_or_else(|| anyhow::anyhow!("QR input has no module grid"))?;
    let modules = image.width().min(image.height()) / module;
    if modules == 0 {
        bail!("QR module grid is empty");
    }
    let quiet = 4u32;
    let scale = (u32::from(width) / (modules + quiet * 2)).max(1);
    let output = modules * scale + quiet * 2 * scale;
    let mut canvas = ImageBuffer::from_pixel(u32::from(width), output, Luma([255]));
    let x_offset = (u32::from(width) - output) / 2 + quiet * scale;
    let y_offset = quiet * scale;
    for my in 0..modules {
        for mx in 0..modules {
            let x = (mx * module + module / 2).min(image.width() - 1);
            let y = (my * module + module / 2).min(image.height() - 1);
            let value = if image.get_pixel(x, y).0[0] < 128 {
                0
            } else {
                255
            };
            for y in y_offset + my * scale..y_offset + (my + 1) * scale {
                for x in x_offset + mx * scale..x_offset + (mx + 1) * scale {
                    canvas.put_pixel(x, y, Luma([value]));
                }
            }
        }
    }
    pack(canvas)
}

fn module_size(image: &ImageBuffer) -> Option<u32> {
    let binary = |value: u8| value < 128;
    let mut smallest = u32::MAX;
    for y in 0..image.height() {
        let mut start = 0;
        while start < image.width() {
            let value = binary(image.get_pixel(start, y).0[0]);
            let mut end = start + 1;
            while end < image.width() && binary(image.get_pixel(end, y).0[0]) == value {
                end += 1;
            }
            smallest = smallest.min(end - start);
            start = end;
        }
    }
    (smallest != u32::MAX && smallest > 0).then_some(smallest)
}

pub fn otsu_threshold(image: &ImageBuffer) -> u8 {
    let mut histogram = [0u32; 256];
    for pixel in image.pixels() {
        histogram[usize::from(pixel.0[0])] += 1;
    }
    let total = image.width() * image.height();
    let sum = histogram
        .iter()
        .enumerate()
        .map(|(value, count)| value as u64 * u64::from(*count))
        .sum::<u64>();
    let (mut background, mut background_sum, mut best, mut best_variance) = (0u32, 0u64, 0u8, 0f64);
    for (value, count) in histogram.iter().enumerate() {
        background += count;
        if background == 0 {
            continue;
        }
        let foreground = total - background;
        if foreground == 0 {
            break;
        }
        background_sum += value as u64 * u64::from(*count);
        let difference = background_sum as f64 / background as f64
            - (sum - background_sum) as f64 / foreground as f64;
        let variance = background as f64 * foreground as f64 * difference * difference;
        if variance > best_variance {
            best_variance = variance;
            best = value as u8;
        }
    }
    best
}

fn threshold_image(image: &mut ImageBuffer, threshold: u8) {
    for pixel in image.pixels_mut() {
        pixel.0[0] = if pixel.0[0] <= threshold { 0 } else { 255 };
    }
}

fn sauvola(image: &mut ImageBuffer, window: u32) {
    let source = image.clone();
    let radius = window / 2;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let left = x.saturating_sub(radius);
            let right = (x + radius + 1).min(image.width());
            let top = y.saturating_sub(radius);
            let bottom = (y + radius + 1).min(image.height());
            let mut sum = 0f64;
            let mut squares = 0f64;
            let mut count = 0f64;
            for yy in top..bottom {
                for xx in left..right {
                    let value = f64::from(source.get_pixel(xx, yy).0[0]);
                    sum += value;
                    squares += value * value;
                    count += 1.0;
                }
            }
            let mean = sum / count;
            let deviation = (squares / count - mean * mean).max(0.0).sqrt();
            let threshold = mean * (1.0 + 0.2 * (deviation / 128.0 - 1.0));
            image.get_pixel_mut(x, y).0[0] = if f64::from(source.get_pixel(x, y).0[0]) <= threshold
            {
                0
            } else {
                255
            };
        }
    }
}

fn unsharp(image: &mut ImageBuffer) {
    let source = image.clone();
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mut sum = 0u32;
            let mut count = 0u32;
            for yy in y.saturating_sub(1)..=(y + 1).min(image.height() - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(image.width() - 1) {
                    sum += u32::from(source.get_pixel(xx, yy).0[0]);
                    count += 1;
                }
            }
            let original = i32::from(source.get_pixel(x, y).0[0]);
            image.get_pixel_mut(x, y).0[0] =
                (original * 2 - (sum / count) as i32).clamp(0, 255) as u8;
        }
    }
}

pub fn pattern(width: u16) -> PreparedImage {
    let height = 160;
    let image = ImageBuffer::from_fn(u32::from(width), height, |x, y| {
        let border = x < 3 || y < 3 || x >= u32::from(width) - 3 || y >= height - 3;
        let stripe = (y / 16) % 2 == 0 && x < u32::from(width) / 3;
        let block = x > u32::from(width) * 2 / 3 && y > 80;
        let gradient = 255 - (x * 255 / u32::from(width)) as u8;
        Luma([if border || stripe || block {
            0
        } else {
            gradient
        }])
    });
    pack(image).expect("pattern dimensions are valid")
}

fn pack(image: ImageBuffer) -> Result<PreparedImage> {
    let width = u16::try_from(image.width())?;
    let height = u16::try_from(image.height())?;
    let mut work = image
        .as_raw()
        .iter()
        .map(|&value| f32::from(value))
        .collect::<Vec<_>>();
    let mut data = vec![0; usize::from(width / 8) * usize::from(height)];
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            let index = y * usize::from(width) + x;
            let old = work[index];
            let black = old < 128.0;
            let error = old - if black { 0.0 } else { 255.0 };
            if black {
                data[y * usize::from(width / 8) + x / 8] |= 0x80 >> (x % 8);
            }
            diffuse(&mut work, width, height, x + 1, y, error * 7.0 / 16.0);
            if y + 1 < usize::from(height) {
                if x > 0 {
                    diffuse(&mut work, width, height, x - 1, y + 1, error * 3.0 / 16.0);
                }
                diffuse(&mut work, width, height, x, y + 1, error * 5.0 / 16.0);
                diffuse(&mut work, width, height, x + 1, y + 1, error / 16.0);
            }
        }
    }
    Ok(PreparedImage {
        gray: GrayImage {
            width,
            height,
            data: image.into_raw(),
        },
        raster: RasterImage {
            width,
            height,
            data,
        },
    })
}

fn diffuse(data: &mut [f32], width: u16, height: u16, x: usize, y: usize, error: f32) {
    if x < usize::from(width) && y < usize::from(height) {
        data[y * usize::from(width) + x] += error;
    }
}

fn auto_contrast(image: &mut ImageBuffer) {
    let mut histogram = [0u32; 256];
    for pixel in image.pixels() {
        histogram[usize::from(pixel.0[0])] += 1;
    }
    let clip = image.width() * image.height() / 100;
    let mut count = 0;
    let low = histogram
        .iter()
        .position(|value| {
            count += *value;
            count > clip
        })
        .unwrap_or(0) as u8;
    count = 0;
    let high = histogram
        .iter()
        .rposition(|value| {
            count += *value;
            count > clip
        })
        .unwrap_or(255) as u8;
    if low >= high {
        return;
    }
    for pixel in image.pixels_mut() {
        pixel.0[0] = ((i32::from(pixel.0[0]) - i32::from(low)) * 255 / i32::from(high - low))
            .clamp(0, 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn qr(modules: u32, scale: u32) -> ImageBuffer {
        ImageBuffer::from_fn(modules * scale, modules * scale, |x, y| {
            Luma([if (x / scale + y / scale).is_multiple_of(3) {
                0
            } else {
                255
            }])
        })
    }
    #[test]
    fn otsu_splits_bimodal_image() {
        let image = ImageBuffer::from_fn(20, 1, |x, _| Luma([if x < 10 { 20 } else { 220 }]));
        assert!((20..220).contains(&otsu_threshold(&image)));
    }
    #[test]
    fn text_is_strictly_bilevel() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(30, 10, |x, _| {
            Luma([if x % 4 < 2 { 80 } else { 200 }])
        }));
        let result = prepare_image_with_options(
            image,
            32,
            None,
            ProcessOptions {
                content: ContentKind::Text,
                definition: Definition::Standard,
            },
        )
        .unwrap();
        assert!(result
            .gray
            .data
            .iter()
            .all(|&value| matches!(value, 0 | 255)));
    }
    #[test]
    fn qr_has_integer_modules_and_quiet_zone() {
        let result = prepare_qr(qr(21, 7), 384).unwrap();
        let values = result.gray.data;
        assert!(values.iter().all(|&value| matches!(value, 0 | 255)));
        let first = values.iter().position(|&value| value == 0).unwrap();
        assert!(first >= 4 * 13);
        assert_eq!(first % 384, 3 + 4 * 13);
    }
    #[test]
    fn qr_non_integer_source_scale_is_still_integer() {
        let image = image::imageops::resize(&qr(21, 7), 151, 151, FilterType::Triangle);
        let result = prepare_qr(image, 384).unwrap();
        assert!(result
            .gray
            .data
            .iter()
            .all(|&value| matches!(value, 0 | 255)));
    }
    #[test]
    fn classifier_selects_content() {
        assert_eq!(
            classify(&ImageBuffer::from_pixel(20, 10, Luma([0]))),
            ContentKind::Text
        );
        assert_eq!(classify(&qr(21, 7)), ContentKind::Qr);
        assert_eq!(
            classify(&ImageBuffer::from_fn(64, 32, |x, _| Luma([(x * 4) as u8]))),
            ContentKind::Photo
        );
    }
    #[test]
    fn legacy_photo_is_identical() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(20, 10, |x, y| {
            Luma([((x * 11 + y * 17) % 255) as u8])
        }));
        let old = prepare_image(image.clone(), 32, None).unwrap();
        let new = prepare_image_with_options(
            image,
            32,
            None,
            ProcessOptions {
                content: ContentKind::Photo,
                definition: Definition::Standard,
            },
        )
        .unwrap();
        assert_eq!(old.raster, new.raster);
    }
    #[test]
    fn hd_changes_soft_text() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(32, 16, |x, _| {
            Luma([if (10..22).contains(&x) { 130 } else { 180 }])
        }));
        let standard = prepare_image_with_options(
            image.clone(),
            32,
            None,
            ProcessOptions {
                content: ContentKind::Text,
                definition: Definition::Standard,
            },
        )
        .unwrap();
        let hd = prepare_image_with_options(
            image,
            32,
            None,
            ProcessOptions {
                content: ContentKind::Text,
                definition: Definition::Hd,
            },
        )
        .unwrap();
        assert_ne!(standard.raster, hd.raster);
    }

    #[test]
    fn sauvola_reads_text_on_an_uneven_background() {
        let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(32, 16, |x, y| {
            Luma([
                if ((6..10).contains(&x) || (22..26).contains(&x)) && (3..13).contains(&y) {
                    20
                } else if x < 16 {
                    100
                } else {
                    220
                },
            ])
        }));
        let result = prepare_image_with_options(
            image,
            32,
            None,
            ProcessOptions {
                content: ContentKind::Text,
                definition: Definition::Hd,
            },
        )
        .unwrap();
        assert_eq!(result.gray.data[5 * 32 + 7], 0);
        assert_eq!(result.gray.data[5 * 32 + 23], 0);
    }
}
