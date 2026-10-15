//! Printer wire encoding and reply parsing.

use crate::imaging::{GrayImage, PreparedImage, RasterImage};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub const STATUS: &[u8] = &[0x1e, 0x47, 0x03];
pub const SERIAL: &[u8] = &[0x1d, 0x67, 0x39];
pub const ERROR: &[u8] = &[0x1d, 0x67, 0x53];
pub const PRODUCT: &[u8] = &[0x1d, 0x67, 0x69];
pub const BATTERY: &[u8] = &[0x1b, 0x23, 0x4e];
pub const STATUS_COMMAND: &[u8] = STATUS;
pub const SERIAL_COMMAND: &[u8] = SERIAL;
pub const ERROR_COMMAND: &[u8] = ERROR;
pub const PRODUCT_COMMAND: &[u8] = PRODUCT;
const FEED: &[u8] = &[0x0a, 0x0a, 0x0a, 0x0a];
const GRAY_END: &[u8] = &[0x1b, 0x23, 0x23, 0x43, 0x4c, 0x45, 0x4e];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub hw: String,
    pub sw: String,
    pub volt_mv: u32,
    pub dpi: u16,
}

impl Status {
    pub fn row_bytes(&self) -> u16 {
        self.dpi / 8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedJob {
    pub bytes: Vec<u8>,
    pub raster_rows: Vec<Range<usize>>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrintMode {
    #[default]
    Rows,
    Raster,
    Gray,
}

impl PrintMode {
    /// Encodes a prepared image with this protocol mode.
    pub fn encode(self, image: &PreparedImage, density: u8) -> Result<EncodedJob> {
        match self {
            Self::Rows => encode_rows(&image.raster, density),
            Self::Raster => encode_raster(&image.raster, density),
            Self::Gray => encode_gray(&image.gray, density),
        }
    }
}

impl<'de> Deserialize<'de> for PrintMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match String::deserialize(deserializer)?.as_str() {
            "raster" => Self::Raster,
            "gray" => Self::Gray,
            _ => Self::Rows,
        })
    }
}

fn reply_text(reply: &[u8]) -> Result<&str> {
    let end = reply
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(reply.len());
    std::str::from_utf8(&reply[..end]).map_err(Into::into)
}

pub fn parse_status(reply: &[u8]) -> Result<Status> {
    let text = reply_text(reply)?;
    let mut hw = None;
    let mut sw = None;
    let mut volt_mv = None;
    let mut dpi = None;
    for part in text.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "HV" => hw = Some(value.to_owned()),
            "SV" => sw = Some(value.to_owned()),
            "VOLT" => volt_mv = value.strip_suffix("mv").and_then(|v| v.parse().ok()),
            "DPI" => dpi = value.parse().ok(),
            _ => {}
        }
    }
    Ok(Status {
        hw: hw.ok_or_else(|| anyhow::anyhow!("status reply has no HV"))?,
        sw: sw.ok_or_else(|| anyhow::anyhow!("status reply has no SV"))?,
        volt_mv: volt_mv.ok_or_else(|| anyhow::anyhow!("status reply has no VOLT"))?,
        dpi: dpi.ok_or_else(|| anyhow::anyhow!("status reply has no DPI"))?,
    })
}

pub fn parse_serial(reply: &[u8]) -> Result<String> {
    reply_text(reply)?
        .strip_prefix("sn:")
        .map(|value| value.trim_end_matches('.').to_owned())
        .ok_or_else(|| anyhow::anyhow!("invalid serial reply"))
}

pub fn parse_error(reply: &[u8]) -> Result<Option<u8>> {
    let body = reply
        .strip_prefix(b"err:")
        .ok_or_else(|| anyhow::anyhow!("invalid error reply"))?;
    Ok(body
        .first()
        .copied()
        .filter(|&code| code != 0 && code != b'.'))
}

pub fn parse_product(reply: &[u8]) -> Result<String> {
    reply_text(reply)?
        .strip_prefix("public id:")
        .map(|value| value.trim_end_matches('.').to_owned())
        .ok_or_else(|| anyhow::anyhow!("invalid product reply"))
}

pub fn feed(lines: usize) -> Vec<u8> {
    vec![0x0a; lines]
}

fn prefix(density: u8) -> Vec<u8> {
    vec![0x1d, 0x49, 0xf0, density, 0x1b, 0x40]
}

pub fn encode_raster(image: &RasterImage, density: u8) -> Result<EncodedJob> {
    if !image.width.is_multiple_of(8)
        || image.data.len() != usize::from(image.width / 8) * usize::from(image.height)
    {
        bail!("raster requires a complete width divisible by eight");
    }
    let row_bytes = usize::from(image.width / 8);
    let mut bytes = prefix(density);
    bytes.extend_from_slice(&[
        0x1d,
        0x76,
        0x30,
        0x00,
        row_bytes as u8,
        (row_bytes >> 8) as u8,
        image.height as u8,
        (image.height >> 8) as u8,
    ]);
    let mut raster_rows = Vec::with_capacity(usize::from(image.height));
    for row in image.data.chunks_exact(row_bytes) {
        let start = bytes.len();
        bytes.extend_from_slice(row);
        raster_rows.push(start..bytes.len());
    }
    bytes.extend_from_slice(FEED);
    Ok(EncodedJob { bytes, raster_rows })
}

pub fn encode_rows(image: &RasterImage, density: u8) -> Result<EncodedJob> {
    if !image.width.is_multiple_of(8)
        || image.data.len() != usize::from(image.width / 8) * usize::from(image.height)
    {
        bail!("raster requires a complete width divisible by eight");
    }
    let row_bytes = usize::from(image.width / 8);
    let mut bytes = prefix(density);
    let mut raster_rows = Vec::with_capacity(usize::from(image.height));
    for row in image.data.chunks_exact(row_bytes) {
        let start = bytes.len();
        bytes.extend_from_slice(&[
            0x1d,
            0x76,
            0x30,
            0x00,
            row_bytes as u8,
            (row_bytes >> 8) as u8,
            0x01,
            0x00,
        ]);
        bytes.extend_from_slice(row);
        raster_rows.push(start..bytes.len());
    }
    bytes.extend_from_slice(FEED);
    Ok(EncodedJob { bytes, raster_rows })
}

pub fn encode_rows_with_gap(
    image: &RasterImage,
    density: u8,
    gap_rows: u16,
    cut_marks: bool,
    lead_rows: u16,
) -> Result<EncodedJob> {
    if gap_rows == 0 && lead_rows == 0 {
        return encode_rows(image, density);
    }
    encode_rows(
        &spaced_raster(image, lead_rows, gap_rows, cut_marks)?,
        density,
    )
}

pub fn encode_gray(image: &GrayImage, density: u8) -> Result<EncodedJob> {
    if image.data.len() != usize::from(image.width) * usize::from(image.height) {
        bail!("gray encoding requires complete image rows");
    }
    let mut bytes = prefix(density);
    let header = [0x1d, 0x78, image.width as u8, (image.width >> 8) as u8];
    for row in image.data.chunks_exact(usize::from(image.width)) {
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(row);
    }
    for _ in 0..100 {
        bytes.extend_from_slice(&header);
        bytes.extend(std::iter::repeat_n(0xff, usize::from(image.width)));
    }
    bytes.extend_from_slice(GRAY_END);
    Ok(EncodedJob {
        bytes,
        raster_rows: Vec::new(),
    })
}

pub fn encode_raster_with_gap(
    image: &RasterImage,
    density: u8,
    gap_rows: u16,
    cut_marks: bool,
    lead_rows: u16,
) -> Result<EncodedJob> {
    if gap_rows == 0 && lead_rows == 0 {
        return encode_raster(image, density);
    }
    encode_raster(
        &spaced_raster(image, lead_rows, gap_rows, cut_marks)?,
        density,
    )
}

pub fn encode_gray_with_gap(
    image: &GrayImage,
    density: u8,
    gap_rows: u16,
    cut_marks: bool,
    lead_rows: u16,
) -> Result<EncodedJob> {
    if gap_rows == 0 && lead_rows == 0 {
        return encode_gray(image, density);
    }
    encode_gray(
        &spaced_gray(image, lead_rows, gap_rows, cut_marks)?,
        density,
    )
}

pub fn encode_batch_with_gap(
    images: &[PreparedImage],
    mode: PrintMode,
    density: u8,
    gap_rows: u16,
    cut_marks: bool,
    lead_rows: u16,
) -> Result<EncodedJob> {
    if gap_rows == 0 && lead_rows == 0 {
        return encode_batch(images, mode, density, 0);
    }
    let mut spaced = Vec::with_capacity(images.len());
    for (index, image) in images.iter().enumerate() {
        spaced.push(PreparedImage {
            raster: spaced_raster(
                &image.raster,
                if index == 0 { lead_rows } else { 0 },
                gap_rows,
                cut_marks,
            )?,
            gray: spaced_gray(
                &image.gray,
                if index == 0 { lead_rows } else { 0 },
                gap_rows,
                cut_marks,
            )?,
        });
    }
    encode_batch(&spaced, mode, density, 0)
}

fn spaced_raster(
    image: &RasterImage,
    lead_rows: u16,
    gap_rows: u16,
    cut_marks: bool,
) -> Result<RasterImage> {
    validate_raster(image)?;
    let height = usize::from(image.height) + usize::from(lead_rows) + usize::from(gap_rows);
    if height > usize::from(u16::MAX) {
        bail!("spaced raster height exceeds protocol limit");
    }
    let row_bytes = usize::from(image.width / 8);
    let mut data = vec![0; height * row_bytes];
    let start = usize::from(lead_rows) * row_bytes;
    data[start..start + image.data.len()].copy_from_slice(&image.data);
    if cut_marks && gap_rows > 0 {
        let row = usize::from(lead_rows) + usize::from(image.height) + usize::from(gap_rows) / 2;
        for x in cut_mark_pixels(image.width) {
            data[row * row_bytes + x / 8] |= 0x80 >> (x % 8);
        }
    }
    Ok(RasterImage {
        width: image.width,
        height: height as u16,
        data,
    })
}

fn spaced_gray(
    image: &GrayImage,
    lead_rows: u16,
    gap_rows: u16,
    cut_marks: bool,
) -> Result<GrayImage> {
    if image.data.len() != usize::from(image.width) * usize::from(image.height) {
        bail!("gray encoding requires complete image rows");
    }
    let height = usize::from(image.height) + usize::from(lead_rows) + usize::from(gap_rows);
    if height > usize::from(u16::MAX) {
        bail!("spaced gray height exceeds protocol limit");
    }
    let width = usize::from(image.width);
    let mut data = vec![255; height * width];
    let start = usize::from(lead_rows) * width;
    data[start..start + image.data.len()].copy_from_slice(&image.data);
    if cut_marks && gap_rows > 0 {
        let row = usize::from(lead_rows) + usize::from(image.height) + usize::from(gap_rows) / 2;
        for x in cut_mark_pixels(image.width) {
            data[row * width + x] = 0;
        }
    }
    Ok(GrayImage {
        width: image.width,
        height: height as u16,
        data,
    })
}

fn cut_mark_pixels(width: u16) -> impl Iterator<Item = usize> {
    let width = usize::from(width);
    (0..width).filter(move |&x| {
        x < 6
            || x >= width.saturating_sub(6)
            || ((8..width.saturating_sub(8)).contains(&x) && ((x - 8) / 6).is_multiple_of(2))
    })
}

/// Encodes images as one uninterrupted printer stream.
pub fn encode_batch(
    images: &[PreparedImage],
    mode: PrintMode,
    density: u8,
    gap_rows: u16,
) -> Result<EncodedJob> {
    let Some((first, rest)) = images.split_first() else {
        bail!("batch requires at least one image");
    };
    if rest.is_empty() {
        return match mode {
            PrintMode::Rows => encode_rows(&first.raster, density),
            PrintMode::Raster => encode_raster(&first.raster, density),
            PrintMode::Gray => encode_gray(&first.gray, density),
        };
    }

    match mode {
        PrintMode::Rows => encode_rows_batch(images, density, gap_rows),
        PrintMode::Raster => encode_raster_batch(images, density, gap_rows),
        PrintMode::Gray => encode_gray_batch(images, density, gap_rows),
    }
}

fn encode_rows_batch(images: &[PreparedImage], density: u8, gap_rows: u16) -> Result<EncodedJob> {
    let mut bytes = prefix(density);
    let mut raster_rows = Vec::new();
    for (index, image) in images.iter().enumerate() {
        validate_raster(&image.raster)?;
        let row_bytes = usize::from(image.raster.width / 8);
        for row in image.raster.data.chunks_exact(row_bytes) {
            let start = bytes.len();
            bytes.extend_from_slice(&[
                0x1d,
                0x76,
                0x30,
                0x00,
                row_bytes as u8,
                (row_bytes >> 8) as u8,
                0x01,
                0x00,
            ]);
            bytes.extend_from_slice(row);
            raster_rows.push(start..bytes.len());
        }
        if index + 1 < images.len() {
            for _ in 0..gap_rows {
                let start = bytes.len();
                bytes.extend_from_slice(&[
                    0x1d,
                    0x76,
                    0x30,
                    0x00,
                    row_bytes as u8,
                    (row_bytes >> 8) as u8,
                    0x01,
                    0x00,
                ]);
                bytes.extend(std::iter::repeat_n(0, row_bytes));
                raster_rows.push(start..bytes.len());
            }
        }
    }
    bytes.extend_from_slice(FEED);
    Ok(EncodedJob { bytes, raster_rows })
}

fn encode_raster_batch(images: &[PreparedImage], density: u8, gap_rows: u16) -> Result<EncodedJob> {
    let width = images[0].raster.width;
    if images.iter().any(|image| image.raster.width != width) {
        bail!("batch raster images must have the same width");
    }
    for image in images {
        validate_raster(&image.raster)?;
    }
    let row_bytes = usize::from(width / 8);
    let height = images
        .iter()
        .map(|image| u32::from(image.raster.height))
        .sum::<u32>()
        + u32::from(gap_rows) * u32::try_from(images.len().saturating_sub(1)).unwrap_or(u32::MAX);
    if height > u32::from(u16::MAX) {
        bail!("batch raster height exceeds protocol limit");
    }
    let mut bytes = prefix(density);
    let mut raster_rows = Vec::new();
    bytes.extend_from_slice(&[
        0x1d,
        0x76,
        0x30,
        0x00,
        row_bytes as u8,
        (row_bytes >> 8) as u8,
        height as u8,
        (height >> 8) as u8,
    ]);
    for (index, image) in images.iter().enumerate() {
        for row in image.raster.data.chunks_exact(row_bytes) {
            let start = bytes.len();
            bytes.extend_from_slice(row);
            raster_rows.push(start..bytes.len());
        }
        if index + 1 < images.len() {
            for _ in 0..gap_rows {
                let start = bytes.len();
                bytes.extend(std::iter::repeat_n(0, row_bytes));
                raster_rows.push(start..bytes.len());
            }
        }
    }
    bytes.extend_from_slice(FEED);
    Ok(EncodedJob { bytes, raster_rows })
}

fn encode_gray_batch(images: &[PreparedImage], density: u8, gap_rows: u16) -> Result<EncodedJob> {
    let mut bytes = prefix(density);
    let width = images[0].gray.width;
    if images.iter().any(|image| image.gray.width != width) {
        bail!("batch gray images must have the same width");
    }
    let header = [0x1d, 0x78, width as u8, (width >> 8) as u8];
    for (index, image) in images.iter().enumerate() {
        if image.gray.data.len() != usize::from(image.gray.width) * usize::from(image.gray.height) {
            bail!("gray encoding requires complete image rows");
        }
        for row in image.gray.data.chunks_exact(usize::from(image.gray.width)) {
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(row);
        }
        if index + 1 < images.len() {
            for _ in 0..gap_rows {
                bytes.extend_from_slice(&header);
                bytes.extend(std::iter::repeat_n(0xff, usize::from(width)));
            }
        }
    }
    for _ in 0..100 {
        bytes.extend_from_slice(&header);
        bytes.extend(std::iter::repeat_n(0xff, usize::from(width)));
    }
    bytes.extend_from_slice(GRAY_END);
    Ok(EncodedJob {
        bytes,
        raster_rows: Vec::new(),
    })
}

fn validate_raster(image: &RasterImage) -> Result<()> {
    if !image.width.is_multiple_of(8)
        || image.data.len() != usize::from(image.width / 8) * usize::from(image.height)
    {
        bail!("raster requires a complete width divisible by eight");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded_rows(job: &EncodedJob) -> Vec<Vec<u8>> {
        job.raster_rows
            .iter()
            .map(|range| job.bytes[range.start + 8..range.end].to_vec())
            .collect()
    }

    fn image(value: u8) -> PreparedImage {
        PreparedImage {
            raster: RasterImage {
                width: 8,
                height: 1,
                data: vec![value],
            },
            gray: GrayImage {
                width: 8,
                height: 1,
                data: vec![value; 8],
            },
        }
    }

    #[test]
    fn batch_singleton_preserves_each_mode_output() {
        let image = image(0xaa);
        for (mode, expected) in [
            (PrintMode::Rows, encode_rows(&image.raster, 10)),
            (PrintMode::Raster, encode_raster(&image.raster, 10)),
            (PrintMode::Gray, encode_gray(&image.gray, 10)),
        ] {
            assert_eq!(
                encode_batch(std::slice::from_ref(&image), mode, 10, 8).unwrap(),
                expected.unwrap()
            );
        }
    }

    #[test]
    fn batch_bytes_are_exact_for_every_mode() {
        let images = [image(0xaa), image(0x55)];
        let rows = encode_batch(&images, PrintMode::Rows, 10, 2).unwrap();
        let raster = encode_batch(&images, PrintMode::Raster, 10, 2).unwrap();
        let gray = encode_batch(&images, PrintMode::Gray, 10, 2).unwrap();
        let mut expected_rows = prefix(10);
        for row in [0xaa, 0, 0, 0x55] {
            expected_rows.extend_from_slice(&[0x1d, 0x76, 0x30, 0x00, 1, 0, 1, 0, row]);
        }
        expected_rows.extend_from_slice(FEED);
        assert_eq!(rows.bytes, expected_rows);

        let mut expected_raster = prefix(10);
        expected_raster.extend_from_slice(&[0x1d, 0x76, 0x30, 0x00, 1, 0, 4, 0, 0xaa, 0, 0, 0x55]);
        expected_raster.extend_from_slice(FEED);
        assert_eq!(raster.bytes, expected_raster);

        let mut expected_gray = prefix(10);
        let header = [0x1d, 0x78, 8, 0];
        for value in [0xaa, 0xff, 0xff, 0x55] {
            expected_gray.extend_from_slice(&header);
            expected_gray.extend(std::iter::repeat_n(value, 8));
        }
        for _ in 0..100 {
            expected_gray.extend_from_slice(&header);
            expected_gray.extend(std::iter::repeat_n(0xff, 8));
        }
        expected_gray.extend_from_slice(GRAY_END);
        assert_eq!(gray.bytes, expected_gray);
        assert_eq!(rows.raster_rows.len(), 4);
        assert_eq!(raster.raster_rows.len(), 4);
    }

    #[test]
    fn batch_allows_zero_gap_rows() {
        let images = [image(0xaa), image(0x55)];
        let encoded = encode_batch(&images, PrintMode::Raster, 10, 0).unwrap();
        assert_eq!(
            encoded.bytes,
            [
                prefix(10),
                vec![0x1d, 0x76, 0x30, 0x00, 1, 0, 2, 0, 0xaa, 0x55],
                FEED.to_vec()
            ]
            .concat()
        );
    }

    #[test]
    fn trailing_gap_has_exact_cut_mark_pixels() {
        let image = RasterImage {
            width: 32,
            height: 1,
            data: vec![0x80, 0, 0, 0],
        };
        let job = encode_rows_with_gap(&image, 10, 5, true, 0).unwrap();
        let rows = decoded_rows(&job);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0], image.data);
        assert!(rows[1].iter().all(|&byte| byte == 0));
        assert_eq!(rows[3], vec![0xfc, 0xfc, 0x0f, 0x3f]);
        assert!(rows[5].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn trailing_gap_without_marks_is_blank_pixels() {
        let image = RasterImage {
            width: 32,
            height: 1,
            data: vec![0x80, 0, 0, 0],
        };
        let job = encode_rows_with_gap(&image, 10, 5, false, 0).unwrap();
        let rows = decoded_rows(&job);
        assert!(rows[1..]
            .iter()
            .all(|row| row.iter().all(|&byte| byte == 0)));
    }
}
