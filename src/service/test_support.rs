//! Provides hermetic connector and transport fakes shared by service tests.

use super::{Connector, JobInfo, JobStatus, PrinterService};
use crate::transport::Transport;
use anyhow::Result;
use async_trait::async_trait;
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone)]
pub(super) struct FakeConnector {
    pub(super) writes: Arc<Mutex<Vec<Vec<u8>>>>,
    pub(super) pause: Duration,
}

struct FakeTransport {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    replies: VecDeque<Vec<u8>>,
    pause: Duration,
}

#[async_trait]
impl Transport for FakeTransport {
    async fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writes.lock().await.push(data.to_vec());
        if !self.pause.is_zero() {
            tokio::time::sleep(self.pause).await;
        }
        Ok(())
    }

    async fn read_reply(&mut self, _: Duration) -> Result<Vec<u8>> {
        Ok(self.replies.pop_front().expect("unexpected reply"))
    }
}

#[async_trait]
impl Connector for FakeConnector {
    async fn connect(&self, _: &str, _: bool) -> Result<Box<dyn Transport + Send>> {
        Ok(Box::new(FakeTransport {
            writes: self.writes.clone(),
            replies: VecDeque::from([
                b"HV=H1,SV=V1,VOLT=7640mv,DPI=384,\0".to_vec(),
                b"sn:TEST.\0".to_vec(),
            ]),
            pause: self.pause,
        }))
    }
}

pub(super) fn temp_settings_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "yhkp-service-test-{}-{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos()
    ))
}

pub(super) fn service(pause: Duration) -> (PrinterService, Arc<Mutex<Vec<Vec<u8>>>>, PathBuf) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let path = temp_settings_path();
    let service = PrinterService::new_with_config_path(
        Arc::new(FakeConnector {
            writes: writes.clone(),
            pause,
        }),
        Some(path.clone()),
    );
    (service, writes, path)
}

pub(super) async fn wait_for_job(service: &PrinterService, id: u64) -> JobInfo {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(job) = service.jobs().await.into_iter().find(|job| job.id == id) {
                if !matches!(job.status, JobStatus::Queued | JobStatus::Running) {
                    return job;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("job did not finish")
}
