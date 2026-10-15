//! Terminal UI composition and public entry points.

use crate::{
    document, imaging, protocol,
    service::{
        BatchMode, Event as ServiceEvent, Flip, JobInfo, PrintJob, PrintMode, PrinterService,
        Quality, ServiceState, Settings,
    },
    typeset::{self, Align, Font, TypesetOptions},
    web,
};
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use percent_encoding::percent_decode_str;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    collections::{HashMap, HashSet},
    io::{self, stdout},
    panic,
    path::PathBuf,
    process::Command,
    time::Duration,
};

mod actions;
mod app;
mod grid;
mod paste;
mod preview;
mod settings;
mod update;
mod view;

pub use actions::ready_queue_indices;
pub use actions::Action;
pub use app::{App, BatchRun, Focus, PreviewView, QueueItem, QueueState, QUEUE_CAPACITY};
pub use grid::{queue_grid, queue_grid_columns, queue_hit_test, QueueCell};
pub use paste::parse_dropped_paths;
pub use update::{move_grid_index, update, Msg};
pub use view::{hit_test, render, ButtonRect};

#[cfg(test)]
mod tests;

/// Runs the crossterm UI against an existing service; connection policy remains outside the UI.
pub async fn run(service: PrinterService) -> Result<()> {
    enable_raw_mode()?;
    let mut output = stdout();
    execute!(
        output,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    install_panic_hook();
    let mut terminal = match Terminal::new(CrosstermBackend::new(output)) {
        Ok(terminal) => terminal,
        Err(error) => {
            restore_terminal();
            return Err(error.into());
        }
    };
    let result = run_loop(&mut terminal, service).await;
    let _ = terminal.show_cursor();
    restore_terminal();
    result
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    service: PrinterService,
) -> Result<()> {
    let mut app = App {
        settings: service.settings().await.unwrap_or_default(),
        ..App::default()
    };
    let mut state = service.state();
    let mut events = service.subscribe();
    let (load_sender, mut loads) = tokio::sync::mpsc::unbounded_channel();
    let mut loading = HashSet::new();
    let mut preview_options = imaging::ProcessOptions {
        content: app.settings.content,
        definition: app.settings.definition,
    };
    let mut preview_document_options = view::document_options(&app.settings);
    while !app.should_quit {
        terminal.draw(|frame| render(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                CrosstermEvent::Key(KeyEvent {
                    code, modifiers, ..
                }) => {
                    if code == KeyCode::Char('l') && modifiers.contains(KeyModifiers::CONTROL) {
                        update(&mut app, Msg::Key(KeyCode::Char('\u{c}')));
                        continue;
                    }
                    let actions = update(&mut app, Msg::Key(code));
                    actions::apply_actions(&mut app, actions, &service).await;
                }
                CrosstermEvent::Paste(text) => {
                    update(&mut app, Msg::Paste(text));
                    spawn_preview_loads(
                        &app,
                        &load_sender,
                        &mut loading,
                        preview_options,
                        view::document_options(&app.settings),
                        false,
                    );
                }
                CrosstermEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    ..
                }) => {
                    let actions = update(&mut app, Msg::Mouse(column, row));
                    actions::apply_actions(&mut app, actions, &service).await;
                }
                _ => {}
            }
        }
        if state.has_changed().unwrap_or(false) {
            update(&mut app, Msg::State(state.borrow_and_update().clone()));
        }
        while let Ok(event) = events.try_recv() {
            update(&mut app, Msg::Event(event));
        }
        while let Ok(message) = loads.try_recv() {
            if let Msg::Preview { id, .. } | Msg::PreviewError { id, .. } = &message {
                loading.remove(id);
            }
            update(&mut app, message);
        }
        let options = imaging::ProcessOptions {
            content: app.settings.content,
            definition: app.settings.definition,
        };
        if options != preview_options {
            preview_options = options;
            spawn_preview_loads(
                &app,
                &load_sender,
                &mut loading,
                options,
                view::document_options(&app.settings),
                true,
            );
        }
        let new_document_options = view::document_options(&app.settings);
        if new_document_options != preview_document_options {
            preview_document_options = new_document_options.clone();
            spawn_preview_loads(
                &app,
                &load_sender,
                &mut loading,
                options,
                new_document_options,
                true,
            );
        }
        update(&mut app, Msg::Jobs(service.jobs().await));
        actions::sync_cards(&mut app, service.take_cards().await);
    }
    Ok(())
}

fn spawn_preview_loads(
    app: &App,
    sender: &tokio::sync::mpsc::UnboundedSender<Msg>,
    loading: &mut HashSet<u64>,
    options: imaging::ProcessOptions,
    document_options: TypesetOptions,
    force: bool,
) {
    for item in app
        .queue
        .iter()
        .filter(|item| {
            matches!(item.state, QueueState::Loading)
                || (force && matches!(item.state, QueueState::Ready))
        })
        .filter(|item| loading.insert(item.id))
    {
        let id = item.id;
        let path = item.path.clone();
        let sender = sender.clone();
        let document_options = document_options.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking({
                let path = path.clone();
                move || {
                    if view::is_document_path(&path) {
                        let text = std::fs::read_to_string(&path)?;
                        let document = match path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .map(|extension| extension.eq_ignore_ascii_case("txt"))
                        {
                            Some(true) => document::parse_text(&text),
                            _ => document::parse_markdown(&text),
                        };
                        let pages = typeset::typeset(&document, &document_options)
                            .into_iter()
                            .map(typeset::prepared)
                            .collect::<Vec<_>>();
                        let image = pages
                            .first()
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("document is empty"))?;
                        Ok((image, pages))
                    } else {
                        imaging::load_image_with_options(&path, 384, None, options)
                            .map(|image| (image, Vec::new()))
                    }
                }
            })
            .await;
            let message = match result {
                Ok(Ok((image, pages))) => Msg::Preview { id, image, pages },
                Ok(Err(error)) => Msg::PreviewError {
                    id,
                    error: error.to_string(),
                },
                Err(error) => Msg::PreviewError {
                    id,
                    error: error.to_string(),
                },
            };
            let _ = sender.send(message);
        });
    }
}
