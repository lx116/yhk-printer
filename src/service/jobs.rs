//! Defines encoded print jobs and job-record updates for the FIFO service queue.

use crate::{
    protocol::EncodedJob,
    service::{ActorData, ActorDeps, Command, Event, JobStatus, ServiceState},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, RwLock};

#[derive(Clone, Debug, Serialize)]
pub struct JobInfo {
    pub id: u64,
    pub status: JobStatus,
    pub bytes_sent: usize,
    pub bytes_total: usize,
}

use serde::Serialize;

#[derive(Clone, Debug)]
pub struct PrintJob {
    pub bytes: Vec<u8>,
    pub row_ends: Vec<usize>,
}

impl From<EncodedJob> for PrintJob {
    fn from(job: EncodedJob) -> Self {
        Self {
            bytes: job.bytes,
            row_ends: job.raster_rows.into_iter().map(|row| row.end).collect(),
        }
    }
}

impl PrintJob {
    pub fn batch(jobs: impl IntoIterator<Item = EncodedJob>) -> Self {
        jobs.into_iter().fold(
            Self {
                bytes: Vec::new(),
                row_ends: Vec::new(),
            },
            |mut batch, job| {
                let offset = batch.bytes.len();
                batch.bytes.extend(job.bytes);
                batch
                    .row_ends
                    .extend(job.raster_rows.into_iter().map(|row| offset + row.end));
                batch
            },
        )
    }
}

impl From<Vec<EncodedJob>> for PrintJob {
    fn from(jobs: Vec<EncodedJob>) -> Self {
        Self::batch(jobs)
    }
}

pub(super) async fn queue_job(jobs: &Arc<RwLock<Vec<JobInfo>>>, id: u64, job: &PrintJob) {
    jobs.write().await.push(JobInfo {
        id,
        status: JobStatus::Queued,
        bytes_sent: 0,
        bytes_total: job.bytes.len(),
    });
}

pub(super) async fn set_job(
    jobs: &RwLock<Vec<JobInfo>>,
    id: u64,
    status: JobStatus,
    bytes_sent: usize,
) {
    if let Some(job) = jobs.write().await.iter_mut().find(|job| job.id == id) {
        job.status = status;
        job.bytes_sent = bytes_sent;
    }
}

pub(super) async fn finish(
    jobs: &RwLock<Vec<JobInfo>>,
    events: &broadcast::Sender<Event>,
    id: u64,
    status: JobStatus,
) {
    set_job(jobs, id, status, usize::MAX).await;
    if let Some(job) = jobs.read().await.iter().find(|job| job.id == id).cloned() {
        let _ = events.send(Event::JobFinished { job });
    }
}

pub(super) async fn run_job(
    id: u64,
    job: PrintJob,
    transport: &mut Box<dyn crate::transport::Transport + Send>,
    commands: &mut mpsc::Receiver<Command>,
    deps: &ActorDeps<'_>,
    data: &mut ActorData,
) {
    set_job(deps.jobs, id, JobStatus::Running, 0).await;
    let total = job.bytes.len();
    super::publish_state(
        deps.state,
        deps.events,
        ServiceState::Printing {
            job_id: id,
            bytes_sent: 0,
            bytes_total: total,
        },
    );
    let mut sent = 0;
    let mut cancelled = false;
    while sent < total {
        cancelled |= handle_running_commands(id, transport, commands, deps, data).await;
        if cancelled {
            break;
        }
        let end = (sent + data.settings.chunk.max(1)).min(total);
        if let Err(error) = transport.write(&job.bytes[sent..end]).await {
            finish(
                deps.jobs,
                deps.events,
                id,
                JobStatus::Failed(error.to_string()),
            )
            .await;
            super::publish_state(
                deps.state,
                deps.events,
                ServiceState::Error {
                    message: error.to_string(),
                },
            );
            break;
        }
        sent = end;
        set_job(deps.jobs, id, JobStatus::Running, sent).await;
        if let Some(job) = deps
            .jobs
            .read()
            .await
            .iter()
            .find(|job| job.id == id)
            .cloned()
        {
            let _ = deps.events.send(Event::JobProgress { job });
        }
        if data.settings.delay_ms != 0 {
            tokio::time::sleep(Duration::from_millis(data.settings.delay_ms)).await;
        }
        if job.row_ends.contains(&sent) && data.settings.row_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(data.settings.row_delay_ms)).await;
        }
    }
    if cancelled {
        finish(deps.jobs, deps.events, id, JobStatus::Cancelled).await;
    } else if sent == total {
        finish(deps.jobs, deps.events, id, JobStatus::Done).await;
    }
}

async fn handle_running_commands(
    id: u64,
    transport: &mut Box<dyn crate::transport::Transport + Send>,
    commands: &mut mpsc::Receiver<Command>,
    deps: &ActorDeps<'_>,
    data: &mut ActorData,
) -> bool {
    let mut cancelled = false;
    while let Ok(command) = commands.try_recv() {
        match command {
            Command::Cancel(cancel_id) if cancel_id == id => cancelled = true,
            Command::Cancel(cancel_id) => {
                if let Some(position) = data
                    .queue
                    .iter()
                    .position(|(queued_id, _)| *queued_id == cancel_id)
                {
                    data.queue.remove(position);
                    finish(deps.jobs, deps.events, cancel_id, JobStatus::Cancelled).await;
                }
            }
            Command::Print(job, reply) => {
                super::enqueue_job(job, reply, deps.jobs, &mut data.queue, &mut data.next_id).await
            }
            Command::QueueCard(card, reply) => {
                super::enqueue_card(card, reply, deps.cards, deps.jobs, &mut data.next_id).await
            }
            Command::Settings(value, reply) => {
                super::update_settings(value, reply, deps.settings_path, &mut data.settings)
            }
            Command::Feed(lines) => {
                super::write_feed(Some(transport), lines, deps.state, deps.events).await
            }
            Command::Disconnect => cancelled = true,
            Command::Connect | Command::Status => {}
        }
    }
    cancelled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::{service, wait_for_job};
    use std::time::Duration;

    #[test]
    fn batch_job_preserves_row_boundaries() {
        let job = PrintJob::batch([
            EncodedJob {
                bytes: vec![1, 2],
                raster_rows: vec![0..1, 1..2],
            },
            EncodedJob {
                bytes: vec![3, 4, 5],
                raster_rows: vec![0..1, 1..3],
            },
        ]);
        assert_eq!(job.bytes, vec![1, 2, 3, 4, 5]);
        assert_eq!(job.row_ends, vec![1, 2, 3, 5]);
    }

    #[tokio::test]
    async fn jobs_are_written_in_fifo_order() {
        let (service, writes, path) = service(Duration::from_millis(1));
        let first = service
            .print(PrintJob {
                bytes: vec![1; 32],
                row_ends: vec![],
            })
            .await
            .unwrap();
        let second = service
            .print(PrintJob {
                bytes: vec![2; 16],
                row_ends: vec![],
            })
            .await
            .unwrap();
        assert_eq!(wait_for_job(&service, first).await.status, JobStatus::Done);
        assert_eq!(wait_for_job(&service, second).await.status, JobStatus::Done);
        let bytes = writes.lock().await[2..].concat();
        assert_eq!(&bytes[..32], vec![1; 32]);
        assert_eq!(&bytes[32..], vec![2; 16]);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn cancelling_a_queued_job_marks_it_cancelled() {
        let (service, _, path) = service(Duration::from_millis(2));
        let first = service
            .print(PrintJob {
                bytes: vec![1; 128],
                row_ends: vec![],
            })
            .await
            .unwrap();
        let second = service
            .print(PrintJob {
                bytes: vec![2; 16],
                row_ends: vec![],
            })
            .await
            .unwrap();
        service.cancel(second).await.unwrap();
        assert_eq!(wait_for_job(&service, first).await.status, JobStatus::Done);
        assert_eq!(
            wait_for_job(&service, second).await.status,
            JobStatus::Cancelled
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn batch_is_one_cancellable_job() {
        let (service, _, path) = service(Duration::from_millis(2));
        let mut settings = service.settings().await.unwrap();
        settings.chunk = 1;
        settings.delay_ms = 0;
        service.set_settings(settings).await.unwrap();
        let job = PrintJob::batch([
            EncodedJob {
                bytes: vec![1; 64],
                raster_rows: vec![],
            },
            EncodedJob {
                bytes: vec![2; 64],
                raster_rows: vec![],
            },
        ]);
        let total = job.bytes.len();
        let id = service.print(job).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        service.cancel(id).await.unwrap();
        let finished = wait_for_job(&service, id).await;
        assert_eq!(finished.status, JobStatus::Cancelled);
        assert_eq!(finished.bytes_total, total);
        assert_eq!(service.jobs().await.len(), 1);
        let _ = std::fs::remove_file(path);
    }
}
