//! Queue grid layout, rendering, and selection helpers.
use super::*;
use super::{preview::half_block_preview, view::queue_state_label};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueCell {
    pub index: usize,
    pub rect: Rect,
}

/// Returns visible queue cells; callers can use it for rendering and mouse hit testing.
pub fn queue_grid(area: Rect, count: usize, scroll: usize) -> Vec<QueueCell> {
    let columns = queue_grid_columns(area.width);
    let rows = usize::from((area.height / 8).max(1));
    let start = (scroll / columns) * columns;
    (start..count)
        .take(columns * rows)
        .enumerate()
        .map(|(visible, index)| QueueCell {
            index,
            rect: Rect::new(
                area.x
                    + u16::try_from(visible % columns).unwrap_or(0) * (area.width / columns as u16),
                area.y + u16::try_from(visible / columns).unwrap_or(0) * 8,
                area.width / columns as u16,
                8,
            ),
        })
        .collect()
}

pub fn queue_grid_columns(width: u16) -> usize {
    usize::from((width / 20).max(1))
}

pub fn queue_hit_test(column: u16, row: u16, cells: &[QueueCell]) -> Option<usize> {
    cells
        .iter()
        .find(|cell| {
            column >= cell.rect.x
                && column < cell.rect.x.saturating_add(cell.rect.width)
                && row >= cell.rect.y
                && row < cell.rect.y.saturating_add(cell.rect.height)
        })
        .map(|cell| cell.index)
}

pub(super) fn render_queue_grid(frame: &mut Frame, app: &mut App, area: Rect) {
    let cells = queue_grid(area, app.queue.len(), app.queue_scroll);
    app.queue_columns = queue_grid_columns(area.width);
    app.queue_cells = cells.clone();
    for cell in cells {
        let item = &app.queue[cell.index];
        let style = Style::default().fg(if cell.index == app.selected {
            Color::Yellow
        } else {
            Color::Gray
        });
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .title(format!(
                "{} {}",
                cell.index + 1,
                truncate(&item.name, cell.rect.width.saturating_sub(6))
            ));
        let inner = block.inner(cell.rect);
        frame.render_widget(block, cell.rect);
        if let Some(card) = &item.card {
            frame.render_widget(Paragraph::new("▤"), Rect::new(inner.x, inner.y, 1, 1));
            let preview = card.jobs.first().map(prepared_from_raster);
            if let Some(preview) = preview {
                let lines = half_block_preview(
                    &preview,
                    false,
                    inner.width.saturating_sub(2),
                    inner.height.saturating_sub(2),
                );
                frame.render_widget(
                    Paragraph::new(Text::from(lines)),
                    Rect::new(
                        inner.x.saturating_add(2),
                        inner.y,
                        inner.width.saturating_sub(2),
                        inner.height.saturating_sub(2),
                    ),
                );
            }
            frame.render_widget(
                Paragraph::new(format!("Card ({})", card.layout)).style(style),
                Rect::new(
                    inner.x,
                    inner.y + inner.height.saturating_sub(1),
                    inner.width,
                    1,
                ),
            );
            continue;
        }
        if item.document {
            frame.render_widget(Paragraph::new("[Document]\noverview"), inner);
            continue;
        }
        let preview = item.preview.as_ref().map(|gray| imaging::PreparedImage {
            gray: gray.clone(),
            raster: item.raster.clone().unwrap_or_else(|| imaging::RasterImage {
                width: gray.width,
                height: gray.height,
                data: vec![0; usize::from(gray.width.div_ceil(8)) * usize::from(gray.height)],
            }),
        });
        if let Some(preview) = preview {
            let lines =
                half_block_preview(&preview, false, inner.width, inner.height.saturating_sub(1));
            frame.render_widget(
                Paragraph::new(Text::from(lines)),
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1),
                ),
            );
        }
        frame.render_widget(
            Paragraph::new(truncate(
                &queue_state_label(&item.state, &app.settings.batch_mode),
                inner.width,
            ))
            .style(style),
            Rect::new(
                inner.x,
                inner.y + inner.height.saturating_sub(1),
                inner.width,
                1,
            ),
        );
    }
}

pub(super) fn prepared_from_raster(raster: &imaging::RasterImage) -> imaging::PreparedImage {
    let mut gray = Vec::with_capacity(usize::from(raster.width) * usize::from(raster.height));
    for y in 0..usize::from(raster.height) {
        for x in 0..usize::from(raster.width) {
            let byte = raster.data[y * usize::from(raster.width / 8) + x / 8];
            gray.push(if byte & (0x80 >> (x % 8)) == 0 {
                255
            } else {
                0
            });
        }
    }
    imaging::PreparedImage {
        gray: imaging::GrayImage {
            width: raster.width,
            height: raster.height,
            data: gray,
        },
        raster: raster.clone(),
    }
}

pub(super) fn truncate(value: &str, width: u16) -> String {
    let width = usize::from(width);
    if value.chars().count() <= width {
        value.into()
    } else if width <= 1 {
        "…".into()
    } else {
        format!("{}…", value.chars().take(width - 1).collect::<String>())
    }
}

pub(super) fn file_name(path: Option<&std::path::Path>) -> String {
    path.and_then(|path| path.file_name())
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
