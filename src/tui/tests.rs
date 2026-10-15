//! TUI regression tests.
use super::*;
use super::{
    actions::*,
    app::{CardQueue, BOOK_INSTRUCTION, CALENDAR_INSTRUCTION, CARD_OVERFLOW_ERROR},
    grid::*,
    preview::*,
    update::*,
    view::*,
};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn parses_escaped_spaces_and_quotes() {
    assert_eq!(
        parse_dropped_paths("'/tmp/one image.png' /tmp/two\\ image.png"),
        vec![
            PathBuf::from("/tmp/one image.png"),
            PathBuf::from("/tmp/two image.png")
        ]
    );
}

#[test]
fn parses_file_uris_and_unicode() {
    assert_eq!(
        parse_dropped_paths("file:///tmp/caf%C3%A9%20image.png"),
        vec![PathBuf::from("/tmp/café image.png")]
    );
}

#[test]
fn parses_multiple_newline_and_space_separated_paths() {
    assert_eq!(
        parse_dropped_paths("/tmp/one.png\n\"/tmp/two image.png\""),
        vec![
            PathBuf::from("/tmp/one.png"),
            PathBuf::from("/tmp/two image.png")
        ]
    );
}

#[test]
fn expands_home_paths() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    assert_eq!(
        parse_dropped_paths("~/image.png"),
        vec![PathBuf::from(home).join("image.png")]
    );
}

#[test]
fn paste_appends_loading_items_and_caps_the_queue() {
    let mut app = App::default();
    update(&mut app, Msg::Paste("/tmp/one.png /tmp/two.png".into()));
    assert_eq!(app.queue.len(), 2);
    assert_eq!(app.queue[0].name, "one.png");
    assert_eq!(app.selected, 1);
    assert!(matches!(app.queue[0].state, QueueState::Loading));
    update(&mut app, Msg::Paste("/tmp/three.png".into()));
    assert_eq!(app.queue.len(), 3, "a new drop retains previous items");
    assert_eq!(app.selected, 2, "the last dropped item becomes selected");
    update(
        &mut app,
        Msg::Paste(
            (0..QUEUE_CAPACITY)
                .map(|n| format!("/tmp/{n}.png"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    );
    assert_eq!(app.queue.len(), QUEUE_CAPACITY);
}

#[test]
fn markdown_paste_creates_a_document_item_and_document_settings_clamp() {
    let mut app = App::default();
    update(&mut app, Msg::Paste("/tmp/note.md".into()));
    assert!(app.queue[0].document);
    app.focus = Focus::Settings;
    app.selected_setting = 11;
    for _ in 0..20 {
        update(&mut app, Msg::Key(KeyCode::Left));
    }
    assert_eq!(app.settings.doc_size_pt, 8.0);
    for _ in 0..20 {
        update(&mut app, Msg::Key(KeyCode::Right));
    }
    assert_eq!(app.settings.doc_size_pt, 16.0);
}

#[test]
fn editor_key_and_button_require_a_selected_document() {
    let mut app = App::default();
    assert!(update(&mut app, Msg::Key(KeyCode::Char('e'))).is_empty());
    assert_eq!(app.status, "Select a document to open the editor");
    update(&mut app, Msg::Paste("/tmp/note.md".into()));
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char('e'))),
        vec![Action::Editor]
    );
    assert!(button_rects(Rect::new(0, 0, 120, 3), &app)
        .iter()
        .any(|button| button.action == Action::Editor));
}

#[test]
fn focus_keys_select_reorder_remove_and_clear_queue() {
    let mut app = App::default();
    update(&mut app, Msg::Paste("/tmp/one.png /tmp/two.png".into()));
    update(&mut app, Msg::Key(KeyCode::Down));
    update(&mut app, Msg::Key(KeyCode::Char('[')));
    assert_eq!(app.queue[0].name, "two.png");
    update(&mut app, Msg::Key(KeyCode::Backspace));
    assert_eq!(app.queue.len(), 1);
    update(&mut app, Msg::Key(KeyCode::Char('\u{c}')));
    assert!(app.queue.is_empty());
}

#[test]
fn reordering_at_the_ends_clamps_and_selection_follows_the_item() {
    let mut app = App::default();
    update(&mut app, Msg::Paste("/tmp/one.png /tmp/two.png".into()));
    update(&mut app, Msg::Key(KeyCode::Char(']')));
    assert_eq!(app.selected, 1);
    update(&mut app, Msg::Key(KeyCode::Char('[')));
    assert_eq!(app.queue[0].name, "two.png");
    assert_eq!(app.selected, 0);
    update(&mut app, Msg::Key(KeyCode::Char('[')));
    assert_eq!(app.selected, 0);
}

#[test]
fn preview_cells_preserve_aspect_ratio() {
    assert_eq!(preview_cell_size(384, 256, 48, 9), (27, 9));
    assert_eq!(preview_cell_size(384, 256, 48, 32), (48, 16));
    assert_eq!(preview_cell_size(256, 384, 80, 20), (26, 20));
}

#[test]
fn box_filter_averages_source_pixels() {
    let image = imaging::PreparedImage {
        gray: imaging::GrayImage {
            width: 4,
            height: 2,
            data: vec![0, 20, 40, 60, 80, 100, 120, 140],
        },
        raster: imaging::RasterImage {
            width: 4,
            height: 2,
            data: vec![0, 0],
        },
    };
    assert_eq!(box_filter(&image, false, 0, 0, 2, 1), 50);
    assert_eq!(box_filter(&image, false, 1, 0, 2, 1), 90);
}

#[test]
fn v_toggles_preview_view() {
    let mut app = App::default();
    update(&mut app, Msg::Key(KeyCode::Char('v')));
    assert_eq!(app.preview_view, PreviewView::Printed);
    update(&mut app, Msg::Key(KeyCode::Char('v')));
    assert_eq!(app.preview_view, PreviewView::Gray);
}

#[test]
fn m_cycles_rows_raster_gray_rows() {
    let mut app = App::default();
    assert_eq!(app.settings.mode, PrintMode::Rows);
    update(&mut app, Msg::Key(KeyCode::Char('m')));
    assert_eq!(app.settings.mode, PrintMode::Raster);
    update(&mut app, Msg::Key(KeyCode::Char('m')));
    assert_eq!(app.settings.mode, PrintMode::Gray);
    update(&mut app, Msg::Key(KeyCode::Char('m')));
    assert_eq!(app.settings.mode, PrintMode::Rows);
}

#[test]
fn b_cycles_batch_modes_and_saves_settings() {
    let mut app = App::default();
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char('b'))),
        vec![Action::SaveSettings]
    );
    assert_eq!(app.settings.batch_mode, BatchMode::Pause);
    update(&mut app, Msg::Key(KeyCode::Char('b')));
    assert_eq!(app.settings.batch_mode, BatchMode::Roll);
    update(&mut app, Msg::Key(KeyCode::Char('b')));
    assert_eq!(app.settings.batch_mode, BatchMode::Advance);
}

#[test]
fn connect_key_toggles_based_on_state() {
    let mut app = App::default();
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char('c'))),
        vec![Action::Connect]
    );
    app.state = ServiceState::Connecting;
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char('c'))),
        vec![Action::Disconnect]
    );
}

fn connected_state() -> ServiceState {
    ServiceState::Connected {
        info: crate::service::PrinterDetails {
            serial: "TEST".into(),
            volt_mv: 0,
            dpi: 384,
        },
    }
}

fn preview() -> imaging::PreparedImage {
    imaging::PreparedImage {
        gray: imaging::GrayImage {
            width: 8,
            height: 1,
            data: vec![255; 8],
        },
        raster: imaging::RasterImage {
            width: 8,
            height: 1,
            data: vec![0],
        },
    }
}

fn item(id: u64, name: &str, state: QueueState) -> QueueItem {
    let preview = preview();
    QueueItem {
        id,
        path: PathBuf::from(format!("/tmp/{name}")),
        name: name.into(),
        preview: Some(preview.gray),
        document: false,
        document_pages: Vec::new(),
        raster: Some(preview.raster),
        state,
        card: None,
        card_service_id: None,
    }
}

fn card_item(id: u64, layout: &str) -> QueueItem {
    let raster = imaging::RasterImage {
        width: 8,
        height: 1,
        data: vec![0b1000_0000],
    };
    QueueItem {
        id,
        path: PathBuf::new(),
        name: format!("Card ({layout})"),
        preview: Some(prepared_from_raster(&raster).gray),
        document: false,
        document_pages: Vec::new(),
        raster: Some(raster.clone()),
        state: QueueState::Ready,
        card: Some(CardQueue {
            layout: layout.into(),
            jobs: if layout == "behind" {
                vec![raster.clone(), raster]
            } else {
                vec![raster]
            },
            instruction: BOOK_INSTRUCTION.into(),
        }),
        card_service_id: None,
    }
}

#[test]
fn card_grid_labels_the_layout_and_photo_thumbnail() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App {
        queue: vec![card_item(1, "behind"), card_item(2, "below")],
        ..App::default()
    };
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Card (behind)"));
    assert!(text.contains("▤"));
}

#[test]
fn behind_card_waits_for_reinsert_then_continues_and_cancels() {
    let mut app = App {
        queue: vec![card_item(9, "behind")],
        batch_run: BatchRun::CardPrinting {
            job_id: 7,
            item_id: 9,
            job_index: 0,
        },
        ..App::default()
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobFinished {
            job: JobInfo {
                id: 7,
                status: crate::service::JobStatus::Done,
                bytes_sent: 1,
                bytes_total: 1,
            },
        }),
    );
    assert!(matches!(app.batch_run, BatchRun::WaitingReinsert { .. }));
    assert_eq!(app.status, BOOK_INSTRUCTION);
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char(' '))),
        vec![Action::Next]
    );
    app.batch_run = BatchRun::CardPrinting {
        job_id: 8,
        item_id: 9,
        job_index: 1,
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobFinished {
            job: JobInfo {
                id: 8,
                status: crate::service::JobStatus::Done,
                bytes_sent: 1,
                bytes_total: 1,
            },
        }),
    );
    assert!(matches!(app.batch_run, BatchRun::Idle));
    app.batch_run = BatchRun::WaitingReinsert { item_id: 9 };
    assert!(update(&mut app, Msg::Key(KeyCode::Esc)).is_empty());
    assert!(matches!(app.batch_run, BatchRun::Idle));
    assert_eq!(card_instruction(Flip::Calendar), CALENDAR_INSTRUCTION);
}

#[test]
fn card_failure_stops_the_sequence_and_overflow_starts_nothing() {
    let mut app = App {
        queue: vec![card_item(9, "behind")],
        batch_run: BatchRun::CardPrinting {
            job_id: 7,
            item_id: 9,
            job_index: 0,
        },
        ..App::default()
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobFinished {
            job: JobInfo {
                id: 7,
                status: crate::service::JobStatus::Failed("paper jam".into()),
                bytes_sent: 0,
                bytes_total: 1,
            },
        }),
    );
    assert!(matches!(app.batch_run, BatchRun::Idle));
    assert_eq!(app.status, "paper jam");
    app.queue[0].card.as_mut().unwrap().jobs.pop();
    assert!(start_card(&mut app, 0).is_none());
    assert_eq!(app.status, CARD_OVERFLOW_ERROR);
}

#[test]
fn below_card_starts_one_job() {
    let mut app = App {
        queue: vec![card_item(9, "below")],
        ..App::default()
    };
    assert_eq!(start_card(&mut app, 0), Some((9, 1)));
}

#[test]
fn grid_columns_and_navigation_clamp_at_edges() {
    assert_eq!(queue_grid_columns(1), 1);
    assert_eq!(queue_grid_columns(39), 1);
    assert_eq!(queue_grid_columns(40), 2);
    assert_eq!(queue_grid_columns(81), 4);
    assert_eq!(move_grid_index(0, 5, 2, -1, 0), 0);
    assert_eq!(move_grid_index(0, 5, 2, 0, -1), 0);
    assert_eq!(move_grid_index(1, 5, 2, 1, 0), 2);
    assert_eq!(move_grid_index(4, 5, 2, 0, 1), 4);
}

#[test]
fn scroll_moves_to_keep_selected_cell_visible() {
    let mut app = App {
        queue: (0..8)
            .map(|id| item(id as u64, &format!("{id}.png"), QueueState::Ready))
            .collect(),
        selected: 6,
        queue_cells: queue_grid(Rect::new(0, 0, 40, 8), 8, 0),
        ..App::default()
    };
    ensure_selection_visible(&mut app);
    assert_eq!(app.queue_scroll, 6);
}

#[test]
fn failed_load_only_marks_its_matching_item() {
    let mut app = App::default();
    update(&mut app, Msg::Paste("/tmp/one.png /tmp/two.png".into()));
    let first = app.queue[0].id;
    update(
        &mut app,
        Msg::PreviewError {
            id: first,
            error: "bad image".into(),
        },
    );
    assert!(matches!(app.queue[0].state, QueueState::Failed(_)));
    assert!(matches!(app.queue[1].state, QueueState::Loading));
}

#[test]
fn print_all_keeps_queue_order_and_skips_non_ready_items() {
    let queue = vec![
        item(1, "one.png", QueueState::Ready),
        item(2, "two.png", QueueState::Loading),
        item(3, "three.png", QueueState::Ready),
        item(4, "four.png", QueueState::Failed("bad".into())),
    ];
    assert_eq!(ready_queue_indices(&queue), vec![0, 2]);
}

#[test]
fn job_events_update_the_queued_item_state() {
    let mut app = App {
        queue: vec![item(9, "one.png", QueueState::Queued(7))],
        job_items: HashMap::from([(7, vec![9])]),
        ..App::default()
    };
    let mut job = JobInfo {
        id: 7,
        status: crate::service::JobStatus::Running,
        bytes_sent: 2,
        bytes_total: 4,
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobProgress { job: job.clone() }),
    );
    assert_eq!(app.queue[0].state, QueueState::Printing(7));
    job.status = crate::service::JobStatus::Done;
    update(&mut app, Msg::Event(ServiceEvent::JobFinished { job }));
    assert_eq!(app.queue[0].state, QueueState::Printed(7));
}

#[test]
fn roll_events_update_every_item_in_the_group() {
    let mut app = App {
        settings: Settings {
            batch_mode: BatchMode::Roll,
            gap_mm: 1.0,
            ..Settings::default()
        },
        queue: vec![
            item(1, "one.png", QueueState::Queued(7)),
            item(2, "two.png", QueueState::Queued(7)),
        ],
        job_items: HashMap::from([(7, vec![1, 2])]),
        ..App::default()
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobProgress {
            job: JobInfo {
                id: 7,
                status: crate::service::JobStatus::Running,
                bytes_sent: 2,
                bytes_total: 4,
            },
        }),
    );
    assert!(app
        .queue
        .iter()
        .all(|item| matches!(item.state, QueueState::Printing(7))));
    assert_eq!(app.status, "Printing roll #7 2/4 bytes");
}

#[test]
fn continuous_roll_waits_for_confirmation_and_advances_selection() {
    let mut app = App {
        queue: vec![
            item(1, "one.png", QueueState::Printing(7)),
            item(2, "two.png", QueueState::Ready),
        ],
        batch_run: BatchRun::Printing {
            job_id: 7,
            next: vec![2],
        },
        ..App::default()
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobFinished {
            job: JobInfo {
                id: 7,
                status: crate::service::JobStatus::Done,
                bytes_sent: 4,
                bytes_total: 4,
            },
        }),
    );
    assert_eq!(app.selected, 1);
    assert!(matches!(app.batch_run, BatchRun::WaitingConfirm { .. }));
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Char(' '))),
        vec![Action::Next]
    );
    app.batch_run = BatchRun::WaitingConfirm { next: vec![2] };
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Esc)),
        Vec::<Action>::new()
    );
    assert!(matches!(app.batch_run, BatchRun::Idle));
}

#[test]
fn failed_pause_job_stops_sequence_and_keeps_the_error() {
    let mut app = App {
        batch_run: BatchRun::Printing {
            job_id: 7,
            next: vec![2],
        },
        ..App::default()
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobFinished {
            job: JobInfo {
                id: 7,
                status: crate::service::JobStatus::Failed("paper jam".into()),
                bytes_sent: 0,
                bytes_total: 4,
            },
        }),
    );
    assert!(matches!(app.batch_run, BatchRun::Idle));
    assert_eq!(app.status, "paper jam");
    assert!(app.status_is_error);
}

#[test]
fn render_shows_grid_items_and_single_large_preview() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App {
        queue: vec![
            item(1, "one.png", QueueState::Ready),
            item(2, "two.png", QueueState::Loading),
            item(3, "three.png", QueueState::Printed(3)),
        ],
        ..App::default()
    };
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("1 one.png"));
    assert!(text.contains("2 two.png"));
    assert!(text.contains("3 three.png"));

    app.queue.truncate(1);
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("one.png"));
    assert!(text.contains("▀"));
}

#[test]
fn button_hit_testing_includes_edges_and_excludes_gaps() {
    let buttons = [
        ButtonRect {
            rect: Rect::new(2, 4, 9, 1),
            action: Action::Connect,
        },
        ButtonRect {
            rect: Rect::new(12, 4, 9, 1),
            action: Action::Print,
        },
    ];
    assert_eq!(hit_test(2, 4, &buttons), Some(Action::Connect));
    assert_eq!(hit_test(10, 4, &buttons), Some(Action::Connect));
    assert_eq!(hit_test(11, 4, &buttons), None);
    assert_eq!(hit_test(12, 4, &buttons), Some(Action::Print));
    assert_eq!(hit_test(21, 4, &buttons), None);
    assert_eq!(hit_test(12, 5, &buttons), None);
}

#[test]
fn queue_grid_hit_testing_uses_visible_cells() {
    let cells = queue_grid(Rect::new(4, 7, 40, 16), 8, 3);
    assert_eq!(cells.first().map(|cell| cell.index), Some(2));
    assert_eq!(queue_hit_test(5, 7, &cells), Some(2));
    assert_eq!(queue_hit_test(4, 23, &cells), None);
}

#[test]
fn disabled_print_explains_what_is_missing() {
    let mut app = App::default();
    assert!(update(&mut app, Msg::Key(KeyCode::Char('p'))).is_empty());
    assert_eq!(app.status, "Connect the printer first");

    app.state = connected_state();
    assert!(update(&mut app, Msg::Key(KeyCode::Char('p'))).is_empty());
    assert_eq!(app.status, "Load an image first");
}

#[test]
fn enter_prints_when_ready() {
    let mut app = App {
        state: connected_state(),
        queue: vec![QueueItem {
            id: 1,
            path: PathBuf::from("/tmp/image.png"),
            name: "image.png".into(),
            preview: Some(preview().gray),
            document: false,
            document_pages: Vec::new(),
            raster: Some(preview().raster),
            state: QueueState::Ready,
            card: None,
            card_service_id: None,
        }],
        ..App::default()
    };
    assert_eq!(
        update(&mut app, Msg::Key(KeyCode::Enter)),
        vec![Action::Print]
    );
}

#[test]
fn settings_density_changes_in_steps_of_five_and_clamps() {
    let mut app = App {
        selected_setting: 1,
        ..App::default()
    };
    app.settings.density = 0;
    update(&mut app, Msg::Key(KeyCode::Left));
    assert_eq!(app.settings.density, 0);
    app.settings.density = 255;
    update(&mut app, Msg::Key(KeyCode::Right));
    assert_eq!(app.settings.density, 255);
}

#[test]
fn job_events_update_queued_then_done_status() {
    let mut app = App::default();
    let mut job = JobInfo {
        id: 7,
        status: crate::service::JobStatus::Queued,
        bytes_sent: 0,
        bytes_total: 20,
    };
    update(
        &mut app,
        Msg::Event(ServiceEvent::JobProgress { job: job.clone() }),
    );
    assert_eq!(app.status, "Queued job #7");
    job.status = crate::service::JobStatus::Done;
    update(&mut app, Msg::Event(ServiceEvent::JobFinished { job }));
    assert_eq!(app.status, "Done job #7");
}

#[test]
fn render_shows_key_strings() {
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::default();
    terminal.draw(|frame| render(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("YHK Printer"));
    assert!(text.contains("Disconnected"));
    assert!(text.contains("Settings"));
    assert!(text.contains("[ Print ]"));
    assert!(text.contains("[ Mode: Advance ]"));
    assert!(text.contains("c connect"));
    assert!(text.contains("Drop image files here"));
}
