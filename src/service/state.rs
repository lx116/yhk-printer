//! Defines observable printer state and service events.

use crate::service::JobInfo;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct PrinterDetails {
    pub serial: String,
    pub volt_mv: u32,
    pub dpi: u16,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ServiceState {
    Disconnected,
    Connecting,
    Connected {
        info: PrinterDetails,
    },
    Printing {
        job_id: u64,
        bytes_sent: usize,
        bytes_total: usize,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", tag = "status", content = "message")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    StateChanged { state: ServiceState },
    JobProgress { job: JobInfo },
    JobFinished { job: JobInfo },
}
