//! Terminal image preview rendering.
use super::*;
/// Fits square source pixels into terminal cells containing two half-block pixels.
pub(super) fn preview_cell_size(
    width: u16,
    height: u16,
    max_columns: u16,
    max_rows: u16,
) -> (u16, u16) {
    if width == 0 || height == 0 || max_columns == 0 || max_rows == 0 {
        return (0, 0);
    }
    let columns_from_rows = u32::from(max_rows) * u32::from(width) * 2 / u32::from(height);
    let columns = u16::try_from(columns_from_rows)
        .unwrap_or(u16::MAX)
        .min(max_columns)
        .max(1);
    let rows = (u32::from(columns) * u32::from(height)).div_ceil(u32::from(width) * 2);
    (
        columns,
        u16::try_from(rows).unwrap_or(max_rows).min(max_rows).max(1),
    )
}

pub(super) fn half_block_preview(
    image: &imaging::PreparedImage,
    printed: bool,
    columns: u16,
    rows: u16,
) -> Vec<Line<'static>> {
    if columns == 0 || rows == 0 {
        return Vec::new();
    }
    (0..rows)
        .map(|row| {
            let spans: Vec<_> = (0..columns)
                .map(|column| {
                    let top = box_filter(image, printed, column, row * 2, columns, rows * 2);
                    let bottom = box_filter(image, printed, column, row * 2 + 1, columns, rows * 2);
                    Span::styled("▀", Style::default().fg(gray(top)).bg(gray(bottom)))
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

pub(super) fn box_filter(
    image: &imaging::PreparedImage,
    printed: bool,
    column: u16,
    row: u16,
    columns: u16,
    rows: u16,
) -> u8 {
    let width = usize::from(image.gray.width);
    let height = usize::from(image.gray.height);
    let x_start = usize::from(column) * width / usize::from(columns);
    let x_end =
        ((usize::from(column + 1) * width / usize::from(columns)).max(x_start + 1)).min(width);
    let y_start = usize::from(row) * height / usize::from(rows);
    let y_end = ((usize::from(row + 1) * height / usize::from(rows)).max(y_start + 1)).min(height);
    let mut total = 0u32;
    let mut pixels = 0u32;
    for y in y_start..y_end {
        for x in x_start..x_end {
            total += u32::from(preview_pixel(image, printed, x, y));
            pixels += 1;
        }
    }
    (total / pixels) as u8
}

fn preview_pixel(image: &imaging::PreparedImage, printed: bool, x: usize, y: usize) -> u8 {
    if !printed {
        return image.gray.data[y * usize::from(image.gray.width) + x];
    }
    let byte = image.raster.data[y * usize::from(image.raster.width / 8) + x / 8];
    if byte & (0x80 >> (x % 8)) == 0 {
        255
    } else {
        0
    }
}

fn gray(value: u8) -> Color {
    Color::Rgb(value, value, value)
}
