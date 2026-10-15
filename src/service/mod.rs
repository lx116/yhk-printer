//! Coordinates settings, printer connection, and the asynchronous FIFO print service.

mod cards;
mod connector;
mod estimate;
mod jobs;
mod settings;
mod state;

#[cfg(test)]
mod test_support;

pub use crate::protocol::PrintMode;
pub use cards::{CardDraft, CardItem};
pub use connector::Connector;
pub use estimate::estimate_print_seconds;
pub use jobs::{JobInfo, PrintJob};
pub use settings::{saved_settings, BatchMode, Flip, Quality, Settings};
pub use state::{Event, JobStatus, PrinterDetails, ServiceState};

use crate::{protocol, transport::Transport};
use anyhow::{Context, Result};
use cards::queue_card;
use jobs::{finish, queue_job, run_job};
use settings::{config_path, load_settings, save_settings};
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, watch};

pub(super) enum Command {
    Connect,
    Disconnect,
    Status,
    Print(PrintJob, tokio::sync::oneshot::Sender<Result<u64>>),
    QueueCard(CardDraft, tokio::sync::oneshot::Sender<Result<u64>>),
    Feed(usize),
    Settings(Option<Settings>, tokio::sync::oneshot::Sender<Settings>),
    Cancel(u64),
}

pub(super) struct ActorDeps<'a> {
    connector: &'a Arc<dyn Connector>,
    state: &'a watch::Sender<ServiceState>,
    events: &'a broadcast::Sender<Event>,
    jobs: &'a Arc<tokio::sync::RwLock<Vec<JobInfo>>>,
    cards: &'a Arc<tokio::sync::RwLock<Vec<CardItem>>>,
    settings_path: &'a Option<PathBuf>,
}

pub(super) struct ActorData {
    transport: Option<Box<dyn Transport + Send>>,
    printer_info: Option<PrinterDetails>,
    settings: Settings,
    queue: VecDeque<(u64, PrintJob)>,
    next_id: u64,
}

#[derive(Clone)]
pub struct PrinterService {
    commands: mpsc::Sender<Command>,
    state: watch::Receiver<ServiceState>,
    events: broadcast::Sender<Event>,
    jobs: Arc<tokio::sync::RwLock<Vec<JobInfo>>>,
    cards: Arc<tokio::sync::RwLock<Vec<CardItem>>>,
}

impl PrinterService {
    pub fn new(connector: Arc<dyn Connector>) -> Self {
        Self::new_with_config_path(connector, config_path())
    }

    pub(crate) fn new_with_config_path(
        connector: Arc<dyn Connector>,
        settings_path: Option<PathBuf>,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(32);
        let (state_tx, state) = watch::channel(ServiceState::Disconnected);
        let (events, _) = broadcast::channel(64);
        let jobs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let cards = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        tokio::spawn(actor(
            receiver,
            connector,
            state_tx,
            events.clone(),
            jobs.clone(),
            cards.clone(),
            settings_path,
        ));
        Self {
            commands,
            state,
            events,
            jobs,
            cards,
        }
    }

    pub fn state(&self) -> watch::Receiver<ServiceState> {
        self.state.clone()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
    pub async fn connect(&self) -> Result<()> {
        self.commands
            .send(Command::Connect)
            .await
            .context("service stopped")
    }
    pub async fn disconnect(&self) -> Result<()> {
        self.commands
            .send(Command::Disconnect)
            .await
            .context("service stopped")
    }
    pub async fn status(&self) -> Result<()> {
        self.commands
            .send(Command::Status)
            .await
            .context("service stopped")
    }
    pub async fn print(&self, job: PrintJob) -> Result<u64> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.commands.send(Command::Print(job, tx)).await?;
        rx.await?
    }
    pub async fn queue_card(&self, card: CardDraft) -> Result<u64> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.commands.send(Command::QueueCard(card, tx)).await?;
        rx.await?
    }
    pub async fn feed(&self, lines: usize) -> Result<()> {
        self.commands.send(Command::Feed(lines)).await?;
        Ok(())
    }
    pub async fn settings(&self) -> Result<Settings> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.commands.send(Command::Settings(None, tx)).await?;
        Ok(rx.await?)
    }
    pub async fn set_settings(&self, settings: Settings) -> Result<Settings> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.commands
            .send(Command::Settings(Some(settings), tx))
            .await?;
        Ok(rx.await?)
    }
    pub async fn cancel(&self, id: u64) -> Result<()> {
        self.commands.send(Command::Cancel(id)).await?;
        Ok(())
    }
    pub async fn jobs(&self) -> Vec<JobInfo> {
        self.jobs.read().await.clone()
    }
    pub async fn cards(&self) -> Vec<CardItem> {
        self.cards.read().await.clone()
    }
    /// Transfers pending cards to the in-process TUI without leaving placeholder jobs active.
    pub async fn take_cards(&self) -> Vec<CardItem> {
        cards::take_cards(&self.cards, &self.jobs).await
    }
}

async fn actor(
    mut commands: mpsc::Receiver<Command>,
    connector: Arc<dyn Connector>,
    state_tx: watch::Sender<ServiceState>,
    events: broadcast::Sender<Event>,
    jobs: Arc<tokio::sync::RwLock<Vec<JobInfo>>>,
    cards: Arc<tokio::sync::RwLock<Vec<CardItem>>>,
    settings_path: Option<PathBuf>,
) {
    let deps = ActorDeps {
        connector: &connector,
        state: &state_tx,
        events: &events,
        jobs: &jobs,
        cards: &cards,
        settings_path: &settings_path,
    };
    let mut data = ActorData {
        transport: None,
        printer_info: None,
        settings: load_settings(settings_path.as_deref()),
        queue: VecDeque::new(),
        next_id: 1,
    };
    loop {
        tokio::select! {
            Some(command) = commands.recv(), if data.queue.is_empty() => handle_idle(command, &deps, &mut data).await,
            else => break,
        }
        while let Some((id, job)) = data.queue.pop_front() {
            if data.transport.is_none() {
                data.printer_info = connect(
                    deps.connector,
                    &data.settings,
                    &mut data.transport,
                    deps.state,
                    deps.events,
                )
                .await;
            }
            let Some(mut transport) = data.transport.take() else {
                finish(
                    deps.jobs,
                    deps.events,
                    id,
                    JobStatus::Failed("not connected".into()),
                )
                .await;
                continue;
            };
            run_job(id, job, &mut transport, &mut commands, &deps, &mut data).await;
            data.transport = Some(transport);
            publish_state(
                deps.state,
                deps.events,
                data.printer_info
                    .clone()
                    .map(|info| ServiceState::Connected { info })
                    .unwrap_or(ServiceState::Disconnected),
            );
        }
    }
}

async fn handle_idle(command: Command, deps: &ActorDeps<'_>, data: &mut ActorData) {
    match command {
        Command::Connect | Command::Status => {
            data.printer_info = connect(
                deps.connector,
                &data.settings,
                &mut data.transport,
                deps.state,
                deps.events,
            )
            .await
            .or(data.printer_info.clone())
        }
        Command::Disconnect => {
            data.transport = None;
            data.printer_info = None;
            publish_state(deps.state, deps.events, ServiceState::Disconnected);
        }
        Command::Print(job, reply) => {
            enqueue_job(job, reply, deps.jobs, &mut data.queue, &mut data.next_id).await
        }
        Command::QueueCard(card, reply) => {
            enqueue_card(card, reply, deps.cards, deps.jobs, &mut data.next_id).await
        }
        Command::Feed(lines) => {
            write_feed(data.transport.as_mut(), lines, deps.state, deps.events).await
        }
        Command::Settings(value, reply) => {
            update_settings(value, reply, deps.settings_path, &mut data.settings)
        }
        Command::Cancel(_) => {}
    }
}

pub(super) async fn enqueue_job(
    job: PrintJob,
    reply: tokio::sync::oneshot::Sender<Result<u64>>,
    jobs: &Arc<tokio::sync::RwLock<Vec<JobInfo>>>,
    queue: &mut VecDeque<(u64, PrintJob)>,
    next_id: &mut u64,
) {
    let id = *next_id;
    *next_id += 1;
    queue_job(jobs, id, &job).await;
    let _ = reply.send(Ok(id));
    queue.push_back((id, job));
}

pub(super) async fn enqueue_card(
    card: CardDraft,
    reply: tokio::sync::oneshot::Sender<Result<u64>>,
    cards: &Arc<tokio::sync::RwLock<Vec<CardItem>>>,
    jobs: &Arc<tokio::sync::RwLock<Vec<JobInfo>>>,
    next_id: &mut u64,
) {
    let id = *next_id;
    *next_id += 1;
    queue_card(cards, jobs, id, card).await;
    let _ = reply.send(Ok(id));
}

pub(super) fn update_settings(
    value: Option<Settings>,
    reply: tokio::sync::oneshot::Sender<Settings>,
    path: &Option<PathBuf>,
    settings: &mut Settings,
) {
    if let Some(value) = value {
        *settings = value;
        save_settings(path.as_deref(), settings);
    }
    let _ = reply.send(settings.clone());
}

pub(super) async fn write_feed(
    transport: Option<&mut Box<dyn Transport + Send>>,
    lines: usize,
    state: &watch::Sender<ServiceState>,
    events: &broadcast::Sender<Event>,
) {
    if let Some(transport) = transport {
        if let Err(error) = transport.write(&protocol::feed(lines)).await {
            publish_state(
                state,
                events,
                ServiceState::Error {
                    message: error.to_string(),
                },
            );
        }
    }
}

async fn connect(
    connector: &Arc<dyn Connector>,
    settings: &Settings,
    transport: &mut Option<Box<dyn Transport + Send>>,
    state: &watch::Sender<ServiceState>,
    events: &broadcast::Sender<Event>,
) -> Option<PrinterDetails> {
    if transport.is_some() {
        return None;
    }
    publish_state(state, events, ServiceState::Connecting);
    match connector
        .connect(&settings.name_prefix, settings.acked)
        .await
    {
        Ok(mut transport_value) => match read_info(&mut *transport_value).await {
            Ok(info) => {
                *transport = Some(transport_value);
                publish_state(
                    state,
                    events,
                    ServiceState::Connected { info: info.clone() },
                );
                Some(info)
            }
            Err(error) => {
                publish_state(
                    state,
                    events,
                    ServiceState::Error {
                        message: error.to_string(),
                    },
                );
                None
            }
        },
        Err(error) => {
            publish_state(
                state,
                events,
                ServiceState::Error {
                    message: error.to_string(),
                },
            );
            None
        }
    }
}

async fn read_info(transport: &mut (dyn Transport + Send)) -> Result<PrinterDetails> {
    transport.write(protocol::STATUS).await?;
    let protocol::Status { volt_mv, dpi, .. } =
        protocol::parse_status(&transport.read_reply(Duration::from_secs(10)).await?)?;
    transport.write(protocol::SERIAL).await?;
    Ok(PrinterDetails {
        serial: protocol::parse_serial(&transport.read_reply(Duration::from_secs(10)).await?)?,
        volt_mv,
        dpi,
    })
}

pub(super) fn publish_state(
    state: &watch::Sender<ServiceState>,
    events: &broadcast::Sender<Event>,
    value: ServiceState,
) {
    let _ = state.send(value.clone());
    let _ = events.send(Event::StateChanged { state: value });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::{service, wait_for_job};

    #[tokio::test]
    async fn state_returns_to_connected_after_printing() {
        let (service, _, path) = service(Duration::ZERO);
        let mut events = service.subscribe();
        assert!(matches!(
            *service.state().borrow(),
            ServiceState::Disconnected
        ));
        service.connect().await.unwrap();
        let id = service
            .print(PrintJob {
                bytes: vec![1],
                row_ends: vec![],
            })
            .await
            .unwrap();
        assert_eq!(wait_for_job(&service, id).await.status, JobStatus::Done);
        let mut states = Vec::new();
        while let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(20), events.recv()).await
        {
            if let Event::StateChanged { state } = event {
                states.push(state);
            }
        }
        assert!(matches!(states[0], ServiceState::Connecting));
        assert!(matches!(states[1], ServiceState::Connected { .. }));
        assert!(states
            .iter()
            .any(|state| matches!(state, ServiceState::Printing { .. })));
        assert!(matches!(
            states.last(),
            Some(ServiceState::Connected { .. })
        ));
        let _ = std::fs::remove_file(path);
    }
}
