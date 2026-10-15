//! TUI state transitions and keyboard handling.
use super::*;
use super::{
    actions::{activate, Action},
    grid::{file_name, queue_hit_test},
    settings::{adjust_setting, cycle_print_mode, upsert_job},
    view::{button_rects, is_document_path},
};

#[derive(Clone, Debug)]
pub enum Msg {
    Key(KeyCode),
    Mouse(u16, u16),
    Paste(String),
    Preview {
        id: u64,
        image: imaging::PreparedImage,
        pages: Vec<imaging::PreparedImage>,
    },
    PreviewError {
        id: u64,
        error: String,
    },
    State(ServiceState),
    Event(ServiceEvent),
    Jobs(Vec<JobInfo>),
    Error(String),
}
pub fn update(app: &mut App, msg: Msg) -> Vec<Action> {
    match msg {
        Msg::Key(key) => return update_key(app, key),
        Msg::Mouse(column, row) => {
            if let Some(action) = hit_test(
                column,
                row,
                &button_rects(Rect::new(0, 3, u16::MAX, 3), app),
            ) {
                return activate(app, action);
            }
            if let Some(index) = queue_hit_test(column, row, &app.queue_cells) {
                app.selected = index;
                app.focus = Focus::Queue;
            }
        }
        Msg::Paste(text) => {
            let paths = parse_dropped_paths(&text);
            if paths.is_empty() {
                set_status(app, "No image path found in pasted text", false);
            } else {
                let path_count = paths.len();
                let remaining = QUEUE_CAPACITY.saturating_sub(app.queue.len());
                let added = path_count.min(remaining);
                app.queue
                    .extend(paths.into_iter().take(remaining).map(|path| {
                        let document = is_document_path(&path);
                        QueueItem {
                            id: {
                                let id = app.next_item_id;
                                app.next_item_id += 1;
                                id
                            },
                            name: file_name(Some(&path)),
                            path,
                            preview: None,
                            document,
                            document_pages: Vec::new(),
                            state: QueueState::Loading,
                            raster: None,
                            card: None,
                            card_service_id: None,
                        }
                    }));
                if added > 0 {
                    app.selected = app.queue.len() - 1;
                }
                set_status(
                    app,
                    if added < path_count {
                        format!("Added {added} image(s); queue limit is {QUEUE_CAPACITY}")
                    } else {
                        format!("Loading {added} image(s)")
                    },
                    added < path_count,
                );
            }
        }
        Msg::Preview { id, image, pages } => {
            let name = if let Some(item) = app.queue.iter_mut().find(|item| item.id == id) {
                item.preview = Some(image.gray);
                item.raster = Some(image.raster);
                item.document_pages = pages;
                item.state = QueueState::Ready;
                Some(item.name.clone())
            } else {
                None
            };
            if let Some(name) = name {
                set_status(app, format!("Loaded {name}"), false);
            }
        }
        Msg::PreviewError { id, error } => {
            if let Some(item) = app.queue.iter_mut().find(|item| item.id == id) {
                item.state = QueueState::Failed(error.clone());
            }
            set_status(app, error, true);
        }
        Msg::State(state) => {
            if let ServiceState::Error { message } = &state {
                set_status(app, message.clone(), true);
            }
            app.state = state;
        }
        Msg::Event(event) => match event {
            ServiceEvent::StateChanged { state } => {
                if let ServiceState::Error { message } = &state {
                    set_status(app, message.clone(), true);
                }
                app.state = state;
            }
            ServiceEvent::JobProgress { job } => {
                update_job_status(app, &job);
                update_queue_job(app, &job);
                upsert_job(&mut app.jobs, job);
            }
            ServiceEvent::JobFinished { job } => {
                update_job_status(app, &job);
                update_queue_job(app, &job);
                advance_batch_run(app, &job);
                upsert_job(&mut app.jobs, job);
            }
        },
        Msg::Jobs(jobs) => app.jobs = jobs,
        Msg::Error(error) => set_status(app, error, true),
    }
    Vec::new()
}

fn update_key(app: &mut App, key: KeyCode) -> Vec<Action> {
    if matches!(
        app.batch_run,
        BatchRun::WaitingConfirm { .. } | BatchRun::WaitingReinsert { .. }
    ) {
        return match key {
            KeyCode::Char(' ') | KeyCode::Char('n') | KeyCode::Enter => activate(app, Action::Next),
            KeyCode::Char('x') | KeyCode::Esc => activate(app, Action::CancelBatch),
            KeyCode::Char('?') => {
                app.show_help = !app.show_help;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return activate(app, Action::Quit),
        KeyCode::Char('?') => app.show_help = !app.show_help,
        KeyCode::Char('c') => return activate(app, connect_action(app)),
        KeyCode::Char('p') | KeyCode::Enter => return activate(app, Action::Print),
        KeyCode::Char('a') => return activate(app, Action::PrintAll),
        KeyCode::Char('g') => app.grid_view = !app.grid_view,
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Queue => Focus::Settings,
                Focus::Settings => Focus::Queue,
            }
        }
        KeyCode::Delete | KeyCode::Backspace if app.focus == Focus::Queue => remove_selected(app),
        KeyCode::Char('\u{c}') if app.focus == Focus::Queue => {
            app.queue.clear();
            app.selected = 0;
            app.queue_scroll = 0;
            set_status(app, "Cleared image queue", false);
        }
        KeyCode::Char('[') if app.focus == Focus::Queue => move_selected(app, false),
        KeyCode::Char(']') if app.focus == Focus::Queue => move_selected(app, true),
        KeyCode::Char('f') => return vec![Action::Feed(4)],
        KeyCode::Char('x') => {
            if let Some(job) = app.jobs.iter().find(|job| {
                matches!(
                    job.status,
                    crate::service::JobStatus::Queued | crate::service::JobStatus::Running
                )
            }) {
                return vec![Action::Cancel(job.id)];
            }
            set_status(app, "No queued or running job to cancel", false);
        }
        KeyCode::Char('m') => {
            cycle_print_mode(app);
            return vec![Action::SaveSettings];
        }
        KeyCode::Char('b') => return activate(app, Action::CycleBatchMode),
        KeyCode::Char('v') => {
            app.preview_view = match app.preview_view {
                PreviewView::Gray => PreviewView::Printed,
                PreviewView::Printed => PreviewView::Gray,
            };
        }
        KeyCode::Char('+') => {
            app.copies = app.copies.saturating_add(1);
        }
        KeyCode::Char('-') => {
            app.copies = app.copies.saturating_sub(1).max(1);
        }
        KeyCode::Up if app.focus == Focus::Queue => move_grid_selection(app, 0, -1),
        KeyCode::Down if app.focus == Focus::Queue => move_grid_selection(app, 0, 1),
        KeyCode::Left if app.focus == Focus::Queue => move_grid_selection(app, -1, 0),
        KeyCode::Right if app.focus == Focus::Queue => move_grid_selection(app, 1, 0),
        KeyCode::Up => app.selected_setting = app.selected_setting.saturating_sub(1),
        KeyCode::Char('e') => return activate(app, Action::Editor),
        KeyCode::Down => app.selected_setting = (app.selected_setting + 1).min(18),
        KeyCode::Left => {
            adjust_setting(app, false);
            return vec![Action::SaveSettings];
        }
        KeyCode::Right => {
            adjust_setting(app, true);
            return vec![Action::SaveSettings];
        }
        KeyCode::Char('o') => return activate(app, Action::Open),
        _ => {}
    }
    Vec::new()
}

pub(super) fn selected_item(app: &App) -> Option<&QueueItem> {
    app.queue.get(app.selected)
}

pub(super) fn selected_ready(app: &App) -> bool {
    selected_item(app).is_some_and(|item| matches!(item.state, QueueState::Ready))
}

fn move_grid_selection(app: &mut App, horizontal: isize, vertical: isize) {
    if app.queue.is_empty() {
        return;
    }
    app.selected = move_grid_index(
        app.selected,
        app.queue.len(),
        app.queue_columns,
        horizontal,
        vertical,
    );
    ensure_selection_visible(app);
}

pub fn move_grid_index(
    selected: usize,
    count: usize,
    columns: usize,
    horizontal: isize,
    vertical: isize,
) -> usize {
    if count == 0 {
        return 0;
    }
    let columns = columns.max(1);
    let target = selected as isize + horizontal + vertical * columns as isize;
    target.clamp(0, count.saturating_sub(1) as isize) as usize
}

pub(super) fn move_selected(app: &mut App, later: bool) {
    if app.queue.len() < 2 {
        return;
    }
    let target = if later {
        (app.selected + 1).min(app.queue.len() - 1)
    } else {
        app.selected.saturating_sub(1)
    };
    app.queue.swap(app.selected, target);
    app.selected = target;
    ensure_selection_visible(app);
}

pub(super) fn advance_selected(app: &mut App, id: u64) {
    if let Some(index) = app.queue.iter().position(|item| item.id == id) {
        app.selected = index;
        ensure_selection_visible(app);
    }
}

pub(super) fn ensure_selection_visible(app: &mut App) {
    let Some(first) = app.queue_cells.first() else {
        return;
    };
    let last = app
        .queue_cells
        .last()
        .map(|cell| cell.index)
        .unwrap_or(first.index);
    if app.selected < first.index || app.selected > last {
        app.queue_scroll = app.selected;
    }
}

pub(super) fn remove_selected(app: &mut App) {
    if app.selected < app.queue.len() {
        let item = app.queue.remove(app.selected);
        app.job_items.retain(|_, ids| {
            ids.retain(|id| *id != item.id);
            !ids.is_empty()
        });
        app.selected = app.selected.min(app.queue.len().saturating_sub(1));
        set_status(app, format!("Removed {}", item.name), false);
    }
}

pub(super) fn connect_action(app: &App) -> Action {
    if matches!(
        app.state,
        ServiceState::Disconnected | ServiceState::Error { .. }
    ) {
        Action::Connect
    } else {
        Action::Disconnect
    }
}

pub(super) fn set_status(app: &mut App, status: impl Into<String>, is_error: bool) {
    app.status = status.into();
    app.status_is_error = is_error;
}

fn update_job_status(app: &mut App, job: &JobInfo) {
    let name = if matches!(app.settings.batch_mode, BatchMode::Roll) {
        "roll"
    } else {
        "job"
    };
    match &job.status {
        crate::service::JobStatus::Queued => {
            set_status(app, format!("Queued {name} #{}", job.id), false)
        }
        crate::service::JobStatus::Running => set_status(
            app,
            format!(
                "Printing {name} #{} {}/{} bytes",
                job.id, job.bytes_sent, job.bytes_total
            ),
            false,
        ),
        crate::service::JobStatus::Done => {
            set_status(app, format!("Done {name} #{}", job.id), false)
        }
        crate::service::JobStatus::Failed(error) => set_status(app, error.clone(), true),
        crate::service::JobStatus::Cancelled => {
            set_status(app, format!("Cancelled {name} #{}", job.id), false)
        }
    }
}

fn update_queue_job(app: &mut App, job: &JobInfo) {
    let Some(ids) = app.job_items.get(&job.id) else {
        return;
    };
    for item in app.queue.iter_mut().filter(|item| ids.contains(&item.id)) {
        item.state = match &job.status {
            crate::service::JobStatus::Queued => QueueState::Queued(job.id),
            crate::service::JobStatus::Running => QueueState::Printing(job.id),
            crate::service::JobStatus::Done => QueueState::Printed(job.id),
            crate::service::JobStatus::Failed(error) => QueueState::PrintFailed(error.clone()),
            crate::service::JobStatus::Cancelled => QueueState::PrintFailed("Cancelled".into()),
        };
    }
}

fn advance_batch_run(app: &mut App, job: &JobInfo) {
    if let BatchRun::CardPrinting {
        job_id,
        item_id,
        job_index,
    } = app.batch_run.clone()
    {
        if job_id != job.id {
            return;
        }
        if !matches!(job.status, crate::service::JobStatus::Done) {
            app.batch_run = BatchRun::Idle;
            return;
        }
        let card = app
            .queue
            .iter()
            .find(|item| item.id == item_id)
            .and_then(|item| item.card.as_ref());
        if job_index == 0 && card.is_some_and(|card| card.layout == "behind") {
            app.batch_run = BatchRun::WaitingReinsert { item_id };
            set_status(
                app,
                card.map(|card| card.instruction.clone())
                    .unwrap_or_default(),
                false,
            );
        } else {
            app.batch_run = BatchRun::Idle;
        }
        return;
    }
    let BatchRun::Printing { job_id, next } = &app.batch_run else {
        return;
    };
    if *job_id != job.id {
        return;
    }
    let next = next.clone();
    if matches!(job.status, crate::service::JobStatus::Done) && !next.is_empty() {
        advance_selected(app, next[0]);
        app.batch_run = BatchRun::WaitingConfirm { next };
        set_status(
            app,
            "Tear the paper, then press Space or click [ Next ]",
            false,
        );
    } else {
        app.batch_run = BatchRun::Idle;
    }
}
