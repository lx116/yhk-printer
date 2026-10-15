use super::{router, WebConfig};
use crate::{
    service::{Connector, JobStatus, PrinterService},
    transport::Transport,
};
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header, Request},
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
use std::{
    collections::VecDeque,
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

#[derive(Clone)]
struct FakeConnector {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

struct FakeTransport {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    replies: VecDeque<Vec<u8>>,
}

#[async_trait]
impl Transport for FakeTransport {
    async fn write(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.writes.lock().await.push(data.to_vec());
        Ok(())
    }

    async fn read_reply(&mut self, _: Duration) -> anyhow::Result<Vec<u8>> {
        Ok(self.replies.pop_front().expect("unexpected reply"))
    }
}

#[async_trait]
impl Connector for FakeConnector {
    async fn connect(&self, _: &str, _: bool) -> anyhow::Result<Box<dyn Transport + Send>> {
        Ok(Box::new(FakeTransport {
            writes: self.writes.clone(),
            replies: VecDeque::from([
                b"HV=H1,SV=V1,VOLT=7640mv,DPI=384,\0".to_vec(),
                b"sn:TEST.\0".to_vec(),
            ]),
        }))
    }
}

pub(super) fn config_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "yhkp-web-test-{}-{}.toml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos()
    ))
}

pub(super) async fn test_service() -> (PrinterService, Arc<Mutex<Vec<Vec<u8>>>>, PathBuf) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let path = config_path();
    let service = PrinterService::new_with_config_path(
        Arc::new(FakeConnector {
            writes: writes.clone(),
        }),
        Some(path.clone()),
    );
    let mut settings = service.settings().await.unwrap();
    settings.chunk = usize::MAX;
    settings.delay_ms = 0;
    service.set_settings(settings).await.unwrap();
    (service, writes, path)
}

pub(super) fn config(token: Option<&str>) -> WebConfig {
    WebConfig {
        token: token.map(str::to_owned),
        allow_origins: vec![],
        listen_host: "127.0.0.1".into(),
    }
}

pub(super) fn app(service: PrinterService, token: Option<&str>) -> axum::Router {
    router(service, config(token))
}

pub(super) fn request(method: &str, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1")
        .body(body)
        .unwrap()
}

pub(super) fn png() -> Vec<u8> {
    let image = DynamicImage::ImageLuma8(ImageBuffer::from_pixel(1, 1, Luma([0])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

pub(super) fn card_png() -> Vec<u8> {
    let image = DynamicImage::ImageLuma8(ImageBuffer::from_fn(384, 200, |x, y| {
        Luma([if x == y { 0 } else { 255 }])
    }));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

pub(super) async fn wait_for_job(service: &PrinterService, id: u64) -> crate::service::JobInfo {
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
