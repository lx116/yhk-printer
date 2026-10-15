//! TUI application state.
use super::*;
#[derive(Clone, Debug)]
pub struct App {
    pub state: ServiceState,
    pub settings: Settings,
    pub selected_setting: usize,
    pub queue: Vec<QueueItem>,
    pub selected: usize,
    pub queue_scroll: usize,
    pub queue_columns: usize,
    pub focus: Focus,
    pub job_items: HashMap<u64, Vec<u64>>,
    pub batch_run: BatchRun,
    pub next_item_id: u64,
    pub queue_cells: Vec<QueueCell>,
    pub preview_view: PreviewView,
    pub grid_view: bool,
    pub jobs: Vec<JobInfo>,
    pub status: String,
    pub status_is_error: bool,
    pub copies: u16,
    pub show_help: bool,
    pub should_quit: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewView {
    #[default]
    Gray,
    Printed,
}

pub const QUEUE_CAPACITY: usize = 50;
pub(super) const CARD_OVERFLOW_ERROR: &str =
    "Text does not fit the photo area: shorten it or use the Below layout";
pub(super) const BOOK_INSTRUCTION: &str = "Cut along the dotted line. Turn the photo over like a page of a book (left to right) so the blank side faces the print head, insert the TOP edge first, then press Space.";
pub(super) const CALENDAR_INSTRUCTION: &str = "Cut along the dotted line. Turn the photo over like a calendar (top to bottom) so the blank side faces the print head, insert the BOTTOM edge first, then press Space.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueState {
    Loading,
    Ready,
    Failed(String),
    Queued(u64),
    Printing(u64),
    Printed(u64),
    PrintFailed(String),
}

#[derive(Clone, Debug)]
pub struct QueueItem {
    pub path: PathBuf,
    pub(super) id: u64,
    pub name: String,
    pub preview: Option<imaging::GrayImage>,
    pub document: bool,
    pub(super) document_pages: Vec<imaging::PreparedImage>,
    pub state: QueueState,
    // Keep the printer-width bitmap only so Printed preview remains available.
    pub(super) raster: Option<imaging::RasterImage>,
    pub(super) card: Option<CardQueue>,
    pub(super) card_service_id: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct CardQueue {
    pub(super) layout: String,
    pub(super) jobs: Vec<imaging::RasterImage>,
    pub(super) instruction: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Queue,
    Settings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BatchRun {
    #[default]
    Idle,
    Printing {
        job_id: u64,
        next: Vec<u64>,
    },
    WaitingConfirm {
        next: Vec<u64>,
    },
    CardPrinting {
        job_id: u64,
        item_id: u64,
        job_index: usize,
    },
    WaitingReinsert {
        item_id: u64,
    },
}

impl Default for App {
    fn default() -> Self {
        Self {
            state: ServiceState::Disconnected,
            settings: Settings::default(),
            selected_setting: 0,
            queue: Vec::new(),
            selected: 0,
            queue_scroll: 0,
            queue_columns: 1,
            focus: Focus::Queue,
            job_items: HashMap::new(),
            batch_run: BatchRun::default(),
            next_item_id: 1,
            queue_cells: Vec::new(),
            preview_view: PreviewView::default(),
            grid_view: true,
            jobs: Vec::new(),
            status: "Drop image paths or press o to open one".into(),
            status_is_error: false,
            copies: 1,
            show_help: false,
            should_quit: false,
        }
    }
}
