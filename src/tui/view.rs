//! TUI layout and non-grid widgets.
use super::*;
use super::{
    actions::Action,
    grid::render_queue_grid,
    preview::{half_block_preview, preview_cell_size},
    update::{connect_action, selected_item, selected_ready},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonRect {
    pub rect: Rect,
    pub action: Action,
}
pub fn render(frame: &mut Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .split(frame.area());
    frame.render_widget(header(app), areas[0]);
    action_bar(frame, app, areas[1]);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(areas[2]);
    image_panel(frame, app, main[0]);
    frame.render_widget(settings_panel(app), main[1]);
    frame.render_widget(jobs_panel(app), areas[3]);
    frame.render_widget(
        Paragraph::new(help_text(app)).style(if app.status_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        areas[4],
    );

    if app.show_help {
        let popup = ratatui::layout::Rect::new(
            frame.area().width / 8,
            frame.area().height / 4,
            frame.area().width * 3 / 4,
            frame.area().height / 2,
        );
        frame.render_widget(Paragraph::new("Darkness: Low/Medium/High map to vendor density; Custom preserves a raw override.\nContent: Auto chooses Photo/Text/QR; Photo dithers, Text is solid, QR preserves modules.\nDefinition: Standard is fast; HD sharpens and uses local text thresholding.\nPause: Space/[ Next ] continues, Esc/[ Cancel ] aborts.  ? close help").block(Block::default().title("Help").borders(Borders::ALL)).wrap(Wrap { trim: true }), popup);
    }
}

fn action_bar(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Block::default().title("Actions").borders(Borders::ALL),
        area,
    );
    let print_ready = matches!(app.state, ServiceState::Connected { .. }) && selected_ready(app);
    for button in button_rects(area, app) {
        let label = action_label(button.action, app);
        let style = if button.action == Action::Print && !print_ready {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(label).style(style), button.rect);
    }
}

fn action_label(action: Action, app: &App) -> String {
    match action {
        Action::Connect => "[ Connect ]".into(),
        Action::Disconnect => "[ Disconnect ]".into(),
        Action::Open => "[ Open ]".into(),
        Action::Editor => "[ Editor ]".into(),
        Action::Print => "[ Print ]".into(),
        Action::PrintAll => "[ Print All ]".into(),
        Action::Remove => "[ Remove ]".into(),
        Action::MoveEarlier => "[ Move < ]".into(),
        Action::MoveLater => "[ Move > ]".into(),
        Action::Feed(_) => "[ Feed ]".into(),
        Action::Cancel(_) | Action::CancelBatch => "[ Cancel ]".into(),
        Action::CycleBatchMode => {
            format!("[ Mode: {} ]", batch_mode_label(&app.settings.batch_mode))
        }
        Action::Next if matches!(app.batch_run, BatchRun::WaitingReinsert { .. }) => {
            "[ Continue ]".into()
        }
        Action::Next => "[ Next ]".into(),
        Action::Quit => "[ Quit ]".into(),
        Action::SaveSettings => unreachable!(),
    }
}

pub(super) fn button_rects(area: Rect, app: &App) -> Vec<ButtonRect> {
    let actions = if matches!(
        app.batch_run,
        BatchRun::WaitingConfirm { .. } | BatchRun::WaitingReinsert { .. }
    ) {
        vec![Action::Next, Action::CancelBatch, Action::Quit]
    } else {
        vec![
            connect_action(app),
            Action::Open,
            Action::Editor,
            Action::Print,
            Action::PrintAll,
            Action::CycleBatchMode,
            Action::Remove,
            Action::MoveEarlier,
            Action::MoveLater,
            Action::Feed(4),
            app.jobs
                .iter()
                .find(|job| {
                    matches!(
                        job.status,
                        crate::service::JobStatus::Queued | crate::service::JobStatus::Running
                    )
                })
                .map_or(Action::Cancel(0), |job| Action::Cancel(job.id)),
            Action::Quit,
        ]
    };
    let mut x = area.x.saturating_add(1);
    let mut buttons = Vec::new();
    for action in actions {
        let width = action_label(action, app).len() as u16;
        if x.saturating_add(width) > area.x.saturating_add(area.width) {
            break;
        }
        buttons.push(ButtonRect {
            rect: Rect::new(x, area.y.saturating_add(1), width, 1),
            action,
        });
        x = x.saturating_add(width.saturating_add(1));
    }
    buttons
}

/// Returns the clicked action without touching application state, so coordinates are testable.
pub fn hit_test(column: u16, row: u16, buttons: &[ButtonRect]) -> Option<Action> {
    buttons
        .iter()
        .find(|button| {
            column >= button.rect.x
                && column < button.rect.x.saturating_add(button.rect.width)
                && row >= button.rect.y
                && row < button.rect.y.saturating_add(button.rect.height)
        })
        .map(|button| button.action)
}

fn header(app: &App) -> Paragraph<'static> {
    let state = match &app.state {
        ServiceState::Disconnected => "Disconnected".into(),
        ServiceState::Connecting => "Connecting".into(),
        ServiceState::Connected { info } => format!(
            "Connected  {}mV  {}dpi  {}",
            info.volt_mv, info.dpi, info.serial
        ),
        ServiceState::Printing {
            job_id,
            bytes_sent,
            bytes_total,
        } => format!("Printing #{job_id} {bytes_sent}/{bytes_total}"),
        ServiceState::Error { message } => format!("Error: {message}"),
    };
    Paragraph::new(Line::from(vec![
        Span::styled("YHK Printer  ", Style::default().fg(Color::Cyan)),
        Span::raw(state),
    ]))
    .block(Block::default().borders(Borders::ALL))
}

fn image_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let view = match app.preview_view {
        PreviewView::Gray => "Gray",
        PreviewView::Printed => "Printed",
    };
    let block = Block::default()
        .title(format!("Image ({view})"))
        .borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let show_grid = app.queue.len() >= 2 && app.grid_view;
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if show_grid {
            [Constraint::Min(4), Constraint::Length(1)]
        } else {
            [Constraint::Length(1), Constraint::Min(1)]
        })
        .split(inner);
    if show_grid {
        render_queue_grid(frame, app, content[0]);
        if let Some(item) = selected_item(app) {
            let dimensions = item.preview.as_ref().map_or_else(
                || "loading".into(),
                |preview| {
                    format!(
                        "{}x{} px  {:.1} mm paper",
                        preview.width,
                        preview.height,
                        f32::from(preview.height) / 8.0
                    )
                },
            );
            frame.render_widget(
                Paragraph::new(format!("{}  {dimensions}", item.name)),
                content[1],
            );
        }
        return;
    }
    app.queue_cells.clear();
    let Some(item) = selected_item(app) else {
        frame.render_widget(
            Paragraph::new("Drop image files here (up to 50)\nPNG, JPEG, GIF, BMP, TIFF, WebP")
                .wrap(Wrap { trim: true }),
            content[1],
        );
        return;
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{}  {}  [{} of {}]",
            item.name,
            queue_state_label(&item.state, &app.settings.batch_mode),
            app.selected + 1,
            app.queue.len(),
        )),
        content[0],
    );
    let Some(gray_image) = &item.preview else {
        frame.render_widget(
            Paragraph::new("Loading image preview...").wrap(Wrap { trim: true }),
            content[1],
        );
        return;
    };
    if item.document {
        frame.render_widget(Paragraph::new(format!("Document overview: {} lines, {:.1} mm paper\nOpen the editor with e for a real preview", gray_image.height, f32::from(gray_image.height) / 8.0)).wrap(Wrap { trim: true }), content[1]);
        return;
    }
    let preview = imaging::PreparedImage {
        gray: gray_image.clone(),
        raster: item.raster.clone().unwrap_or_else(|| imaging::RasterImage {
            width: gray_image.width,
            height: gray_image.height,
            data: vec![
                0;
                usize::from(gray_image.width.div_ceil(8)) * usize::from(gray_image.height)
            ],
        }),
    };
    let (columns, rows) = preview_cell_size(
        preview.gray.width,
        preview.gray.height,
        content[1].width,
        content[1].height,
    );
    let lines = half_block_preview(
        &preview,
        app.preview_view == PreviewView::Printed
            && matches!(app.settings.mode, PrintMode::Rows | PrintMode::Raster),
        columns,
        rows,
    );
    let preview_area = Rect::new(
        content[1].x + content[1].width.saturating_sub(columns) / 2,
        content[1].y + content[1].height.saturating_sub(rows) / 2,
        columns,
        rows,
    );
    frame.render_widget(Paragraph::new(Text::from(lines)), preview_area);
}

pub(super) fn queue_state_label(state: &QueueState, batch_mode: &BatchMode) -> String {
    let roll = matches!(batch_mode, BatchMode::Roll);
    match state {
        QueueState::Loading => "Loading".into(),
        QueueState::Ready => "Ready".into(),
        QueueState::Failed(error) => format!("Load failed: {error}"),
        QueueState::Queued(id) if roll => format!("Roll #{id} queued"),
        QueueState::Printing(id) if roll => format!("Roll #{id} printing"),
        QueueState::Printed(id) if roll => format!("Roll #{id} printed"),
        QueueState::Queued(id) => format!("Queued #{id}"),
        QueueState::Printing(id) => format!("Printing #{id}"),
        QueueState::Printed(id) => format!("Printed #{id}"),
        QueueState::PrintFailed(error) => format!("Print failed: {error}"),
    }
}

fn settings_panel(app: &App) -> List<'static> {
    let content = if app.settings.content == imaging::ContentKind::Auto {
        selected_item(app)
            .and_then(|item| item.preview.as_ref())
            .map_or("Auto".into(), |image| {
                format!(
                    "Auto ({})",
                    content_label(imaging::classify(&image_buffer(image)))
                )
            })
    } else {
        content_label(app.settings.content).into()
    };
    let darkness = match Quality::from_density(app.settings.density) {
        Quality::Custom(value) => format!("Custom ({value})"),
        quality => quality.label().into(),
    };
    let rows = [
        format!("Print darkness: {darkness}"),
        format!("Content: {content}"),
        format!("Definition: {}", definition_label(app.settings.definition)),
        format!("Mode: {:?}", app.settings.mode),
        format!("Batch mode: {}", batch_mode_label(&app.settings.batch_mode)),
        format!("Gap between prints: {:.0} mm", app.settings.gap_mm),
        format!(
            "Cut marks: {}",
            if app.settings.cut_marks { "on" } else { "off" }
        ),
        format!("Chunk: {}", app.settings.chunk),
        format!("Delay: {} ms", app.settings.delay_ms),
        format!("Acknowledged: {}", app.settings.acked),
        format!("Document font: {}", font_label(app.settings.doc_font)),
        format!("Text size: {:.0} pt", app.settings.doc_size_pt),
        format!("Align: {}", align_label(app.settings.doc_align)),
        format!("Margin: {}", app.settings.doc_margin),
        format!(
            "Flip: {}",
            match app.settings.flip {
                Flip::Book => "Book",
                Flip::Calendar => "Calendar",
            }
        ),
        format!("Card margin: {:.1} mm", app.settings.card_margin_mm),
        format!("Back offset: {:.1} mm", app.settings.back_offset_mm),
        format!("Frame: {}", if app.settings.frame { "on" } else { "off" }),
        format!("Min size: {:.1} pt", app.settings.min_pt),
    ];
    List::new(rows.into_iter().enumerate().map(|(index, text)| {
        ListItem::new(text).style(if index == app.selected_setting {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        })
    }))
    .block(Block::default().title("Settings").borders(Borders::ALL))
}

fn image_buffer(image: &imaging::GrayImage) -> image::GrayImage {
    // Preview images are created from width-by-height pixel buffers before entering App state.
    image::GrayImage::from_raw(
        u32::from(image.width),
        u32::from(image.height),
        image.data.clone(),
    )
    .expect("valid preview dimensions")
}
fn content_label(content: imaging::ContentKind) -> &'static str {
    match content {
        imaging::ContentKind::Auto => "Auto",
        imaging::ContentKind::Photo => "Photo",
        imaging::ContentKind::Text => "Text",
        imaging::ContentKind::Qr => "QR",
    }
}
fn definition_label(definition: imaging::Definition) -> &'static str {
    match definition {
        imaging::Definition::Standard => "Standard",
        imaging::Definition::Hd => "HD",
    }
}

fn batch_mode_label(mode: &BatchMode) -> &'static str {
    match mode {
        BatchMode::Advance => "Advance",
        BatchMode::Pause => "Pause",
        BatchMode::Roll => "Roll",
    }
}
fn font_label(font: Font) -> &'static str {
    match font {
        Font::Sans => "Sans",
        Font::Hand => "Hand",
        Font::Mono => "Mono",
    }
}
fn align_label(align: Align) -> &'static str {
    match align {
        Align::Left => "Left",
        Align::Center => "Center",
        Align::Right => "Right",
        Align::Justify => "Justify",
    }
}
pub(super) fn is_document_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("txt" | "md" | "markdown")
    )
}
pub(super) fn document_options(settings: &Settings) -> TypesetOptions {
    TypesetOptions {
        font: settings.doc_font,
        size_pt: settings.doc_size_pt,
        align: settings.doc_align,
        margin_dots: settings.doc_margin,
        ..TypesetOptions::default()
    }
}

fn jobs_panel(app: &App) -> Gauge<'static> {
    let active = app.jobs.iter().find(|job| {
        matches!(
            job.status,
            crate::service::JobStatus::Queued | crate::service::JobStatus::Running
        )
    });
    let (label, ratio) = active.map_or_else(
        || ("No active job".into(), 0.0),
        |job| {
            (
                format!(
                    "{} #{}: {}/{} bytes",
                    if matches!(app.settings.batch_mode, BatchMode::Roll) {
                        "Roll"
                    } else {
                        "Job"
                    },
                    job.id,
                    job.bytes_sent,
                    job.bytes_total
                ),
                if job.bytes_total == 0 {
                    0.0
                } else {
                    job.bytes_sent.min(job.bytes_total) as f64 / job.bytes_total as f64
                },
            )
        },
    );
    Gauge::default()
        .block(Block::default().title("Jobs").borders(Borders::ALL))
        .label(label)
        .ratio(ratio)
}

fn help_text(app: &App) -> String {
    format!(
        "{}  |  c connect  Tab focus  arrows select/edit  p print  a print all  b batch mode  e editor  [/] reorder  Delete remove  Ctrl+L clear  g grid",
        app.status
    )
}
