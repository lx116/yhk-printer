//! Service effects initiated by TUI actions.
use super::*;
use super::{
    app::{CardQueue, BOOK_INSTRUCTION, CALENDAR_INSTRUCTION, CARD_OVERFLOW_ERROR},
    grid::prepared_from_raster,
    settings::cycle_batch_mode,
    update::{
        advance_selected, ensure_selection_visible, selected_item, selected_ready, set_status,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Connect,
    Disconnect,
    Open,
    Editor,
    Print,
    PrintAll,
    Remove,
    MoveEarlier,
    MoveLater,
    Feed(usize),
    Cancel(u64),
    SaveSettings,
    CycleBatchMode,
    Next,
    CancelBatch,
    Quit,
}

pub(super) fn activate(app: &mut App, action: Action) -> Vec<Action> {
    match action {
        Action::Remove => {
            super::update::remove_selected(app);
            Vec::new()
        }
        Action::MoveEarlier => {
            super::update::move_selected(app, false);
            Vec::new()
        }
        Action::MoveLater => {
            super::update::move_selected(app, true);
            Vec::new()
        }
        Action::CycleBatchMode => {
            cycle_batch_mode(app);
            vec![Action::SaveSettings]
        }
        Action::CancelBatch => {
            app.batch_run = BatchRun::Idle;
            set_status(app, "Pause sequence cancelled", false);
            Vec::new()
        }
        Action::Print if !matches!(app.state, ServiceState::Connected { .. }) => {
            set_status(app, "Connect the printer first", false);
            Vec::new()
        }
        Action::PrintAll if !matches!(app.state, ServiceState::Connected { .. }) => {
            set_status(app, "Connect the printer first", false);
            Vec::new()
        }
        Action::Print if !selected_ready(app) => {
            set_status(app, "Load an image first", false);
            Vec::new()
        }
        Action::PrintAll
            if !app
                .queue
                .iter()
                .any(|item| matches!(item.state, QueueState::Ready)) =>
        {
            set_status(app, "No loaded images to print", false);
            Vec::new()
        }
        Action::Open => {
            set_status(
                app,
                "Paste a path (terminal drag and drop is supported)",
                false,
            );
            Vec::new()
        }
        Action::Editor if !selected_item(app).is_some_and(|item| item.document) => {
            set_status(app, "Select a document to open the editor", false);
            Vec::new()
        }
        Action::Quit => {
            app.should_quit = true;
            Vec::new()
        }
        Action::Cancel(0) => {
            set_status(app, "No queued or running job to cancel", false);
            Vec::new()
        }
        action => vec![action],
    }
}

pub(super) async fn apply_actions(app: &mut App, actions: Vec<Action>, service: &PrinterService) {
    for action in actions {
        let result = match action {
            Action::Connect => service.connect().await,
            Action::Disconnect => service.disconnect().await,
            Action::Editor => open_editor(service, app).await,
            Action::Feed(lines) => service.feed(lines).await,
            Action::Cancel(id) => service.cancel(id).await,
            Action::SaveSettings => service.set_settings(app.settings.clone()).await.map(|_| ()),
            Action::Print => print_selected(service, app).await,
            Action::PrintAll => print_all(service, app).await,
            Action::Next => print_continuous_next(service, app).await,
            Action::Open
            | Action::Quit
            | Action::Remove
            | Action::MoveEarlier
            | Action::MoveLater
            | Action::CycleBatchMode
            | Action::CancelBatch => Ok(()),
        };
        if let Err(error) = result {
            update(app, Msg::Error(error.to_string()));
        }
    }
}

async fn open_editor(service: &PrinterService, app: &mut App) -> Result<()> {
    let item = selected_item(app).ok_or_else(|| anyhow::anyhow!("Select a document first"))?;
    if !item.document {
        anyhow::bail!("Select a document to open the editor");
    }
    let text = tokio::fs::read_to_string(&item.path).await?;
    web::ensure_local_server(service.clone()).await?;
    let draft_id = web::create_draft(text).await;
    let url = format!("http://127.0.0.1:7731/editor?draft={draft_id}");
    let open_url = url.clone();
    match tokio::task::spawn_blocking(move || Command::new("open").arg(open_url).status()).await {
        Ok(Ok(status)) if status.success() => set_status(app, "Opened document editor", false),
        _ => set_status(app, format!("Open this editor URL: {url}"), true),
    }
    Ok(())
}

async fn print_selected(service: &PrinterService, app: &mut App) -> Result<()> {
    let index = app.selected;
    print_item(service, app, index).await?;
    if matches!(app.settings.batch_mode, BatchMode::Advance) && index + 1 < app.queue.len() {
        app.selected = index + 1;
        ensure_selection_visible(app);
    }
    Ok(())
}

async fn print_all(service: &PrinterService, app: &mut App) -> Result<()> {
    if matches!(app.settings.batch_mode, BatchMode::Roll) {
        return print_batch(service, app, ready_queue_indices(&app.queue)).await;
    }
    if matches!(app.settings.batch_mode, BatchMode::Pause) {
        return start_continuous(service, app).await;
    }
    let ready = ready_queue_indices(&app.queue);
    let skipped = app.queue.len() - ready.len();
    for index in ready {
        print_item(service, app, index).await?;
    }
    if skipped > 0 {
        set_status(
            app,
            format!("Skipped {skipped} item(s) that are not ready"),
            false,
        );
    }
    Ok(())
}

pub fn ready_queue_indices(queue: &[QueueItem]) -> Vec<usize> {
    queue
        .iter()
        .enumerate()
        .filter_map(|(index, item)| matches!(item.state, QueueState::Ready).then_some(index))
        .collect()
}

async fn print_item(service: &PrinterService, app: &mut App, index: usize) -> Result<u64> {
    if app.queue[index].card.is_some() {
        return print_card(service, app, index).await;
    }
    let item = &app.queue[index];
    let Some(gray) = item.preview.clone() else {
        return Ok(0);
    };
    let raster = item
        .raster
        .clone()
        .ok_or_else(|| anyhow::anyhow!("missing raster preview"))?;
    let item_id = item.id;
    let settings = app.settings.clone();
    let gap_rows = settings.gap_rows();
    let lead_rows = settings.lead_rows();
    let copies = app.copies;
    let image = imaging::PreparedImage { gray, raster };
    let images = if app.queue[index].document {
        app.queue[index].document_pages.clone()
    } else {
        vec![image]
    };
    let encoded = tokio::task::spawn_blocking(move || {
        protocol::encode_batch_with_gap(
            &images,
            settings.mode,
            settings.density,
            gap_rows,
            settings.cut_marks,
            lead_rows,
        )
    })
    .await??;
    let mut last_id = 0;
    for _ in 0..copies {
        let id = service.print(encoded.clone().into()).await?;
        app.job_items.insert(id, vec![item_id]);
        app.queue[index].state = QueueState::Queued(id);
        last_id = id;
    }
    set_status(
        app,
        format!("Queued {} (copies {copies})", app.queue[index].name),
        false,
    );
    Ok(last_id)
}

pub(super) fn start_card(app: &mut App, index: usize) -> Option<(u64, usize)> {
    let item = app.queue.get(index)?;
    let card = item.card.as_ref()?;
    let valid = match card.layout.as_str() {
        "below" => !card.jobs.is_empty(),
        "behind" => card.jobs.len() == 2,
        _ => false,
    };
    if !valid {
        set_status(app, CARD_OVERFLOW_ERROR, true);
        return None;
    }
    Some((
        item.id,
        if card.layout == "behind" {
            0
        } else {
            card.jobs.len()
        },
    ))
}

async fn print_card(service: &PrinterService, app: &mut App, index: usize) -> Result<u64> {
    let Some((item_id, job_count)) = start_card(app, index) else {
        return Ok(0);
    };
    // start_card verified this queue entry contains a valid card and no await occurred since.
    let card = app.queue[index].card.as_ref().expect("checked card");
    let layout = card.layout.clone();
    let rasters = if job_count == 0 {
        vec![card.jobs[0].clone()]
    } else {
        card.jobs.clone()
    };
    let encoded = rasters
        .iter()
        .map(|raster| protocol::encode_raster(raster, app.settings.density))
        .collect::<Result<Vec<_>, _>>()?;
    let id = service.print(PrintJob::batch(encoded)).await?;
    app.job_items.insert(id, vec![item_id]);
    app.queue[index].state = QueueState::Queued(id);
    app.batch_run = BatchRun::CardPrinting {
        job_id: id,
        item_id,
        job_index: 0,
    };
    set_status(app, format!("Queued Card ({layout})"), false);
    Ok(id)
}

pub(super) fn sync_cards(app: &mut App, cards: Vec<crate::service::CardItem>) {
    for card in cards {
        if app
            .queue
            .iter()
            .any(|item| item.card_service_id == Some(card.id))
        {
            continue;
        }
        let preview = card.jobs.first().map(prepared_from_raster);
        let id = app.next_item_id;
        app.next_item_id += 1;
        let instruction = if card.instruction.is_empty() {
            card_instruction(app.settings.flip.clone()).into()
        } else {
            card.instruction
        };
        app.queue.push(QueueItem {
            id,
            path: PathBuf::new(),
            name: format!("Card ({})", card.layout),
            preview: preview.as_ref().map(|image| image.gray.clone()),
            document: false,
            document_pages: Vec::new(),
            raster: preview.map(|image| image.raster),
            state: QueueState::Ready,
            card: Some(CardQueue {
                layout: card.layout,
                jobs: card.jobs,
                instruction,
            }),
            card_service_id: Some(card.id),
        });
    }
}

pub(super) fn card_instruction(flip: Flip) -> &'static str {
    match flip {
        Flip::Book => BOOK_INSTRUCTION,
        Flip::Calendar => CALENDAR_INSTRUCTION,
    }
}

async fn print_batch(service: &PrinterService, app: &mut App, indices: Vec<usize>) -> Result<()> {
    let items: Vec<_> = indices
        .iter()
        .map(|&index| &app.queue[index])
        .flat_map(|item| {
            if item.document {
                item.document_pages.clone()
            } else {
                item.preview
                    .clone()
                    .zip(item.raster.clone())
                    .map_or_else(Vec::new, |(gray, raster)| {
                        vec![imaging::PreparedImage { gray, raster }]
                    })
            }
        })
        .collect();
    if items.is_empty() {
        return Ok(());
    }
    let ids = indices
        .iter()
        .map(|&index| app.queue[index].id)
        .collect::<Vec<_>>();
    let settings = app.settings.clone();
    let gap_rows = settings.gap_rows();
    let lead_rows = settings.lead_rows();
    let copies = app.copies;
    let encoded = tokio::task::spawn_blocking(move || {
        let mut images = Vec::with_capacity(items.len() * usize::from(copies));
        for _ in 0..copies {
            images.extend(items.iter().cloned());
        }
        protocol::encode_batch_with_gap(
            &images,
            settings.mode,
            settings.density,
            gap_rows,
            settings.cut_marks,
            lead_rows,
        )
    })
    .await??;
    let id = service.print(encoded.into()).await?;
    app.job_items.insert(id, ids);
    for &index in &indices {
        app.queue[index].state = QueueState::Queued(id);
    }
    set_status(
        app,
        format!("Queued roll #{id} ({} images)", indices.len()),
        false,
    );
    Ok(())
}

async fn start_continuous(service: &PrinterService, app: &mut App) -> Result<()> {
    let mut ids = app
        .queue
        .iter()
        .filter(|item| matches!(item.state, QueueState::Ready))
        .map(|item| item.id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let current = ids.remove(0);
    advance_selected(app, current);
    let job_id = print_item(service, app, app.selected).await?;
    app.batch_run = BatchRun::Printing { job_id, next: ids };
    Ok(())
}

async fn print_continuous_next(service: &PrinterService, app: &mut App) -> Result<()> {
    if let BatchRun::WaitingReinsert { item_id } = app.batch_run.clone() {
        let Some(index) = app.queue.iter().position(|item| item.id == item_id) else {
            app.batch_run = BatchRun::Idle;
            return Ok(());
        };
        let Some(card) = app.queue[index].card.as_ref() else {
            app.batch_run = BatchRun::Idle;
            return Ok(());
        };
        let encoded = protocol::encode_raster(&card.jobs[1], app.settings.density)?;
        let id = service.print(encoded.into()).await?;
        app.job_items.insert(id, vec![item_id]);
        app.queue[index].state = QueueState::Queued(id);
        app.batch_run = BatchRun::CardPrinting {
            job_id: id,
            item_id,
            job_index: 1,
        };
        return Ok(());
    }
    let BatchRun::WaitingConfirm { mut next } = std::mem::take(&mut app.batch_run) else {
        return Ok(());
    };
    let Some(current) = next.first().copied() else {
        set_status(app, "Pause sequence complete", false);
        return Ok(());
    };
    next.remove(0);
    advance_selected(app, current);
    let job_id = print_item(service, app, app.selected).await?;
    app.batch_run = BatchRun::Printing { job_id, next };
    Ok(())
}
