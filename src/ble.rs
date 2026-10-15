use crate::{service::Connector, transport::Transport};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, Peripheral},
};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

const SERVICE: &str = "49535343-fe7d-4ae5-8fa9-9fafd205e455";
const WRITE: &str = "49535343-8841-43f4-a8d4-ecbe34729bb3";
const REPLY: &str = "49535343-1e4d-4bd9-ba61-23c647249616";
const CONTROL: &str = "49535343-aca3-481c-91ec-d85e28a60318";

#[derive(Debug, Clone)]
pub struct PrinterInfo {
    pub name: String,
    pub rssi: Option<i16>,
    pub id: String,
}

pub struct BleTransport {
    peripheral: Peripheral,
    write: btleplug::api::Characteristic,
    notifications: futures::stream::BoxStream<'static, btleplug::api::ValueNotification>,
    reply_uuid: Uuid,
    acked: bool,
    verbose: bool,
}

pub struct BleConnector {
    pub timeout: Duration,
    pub verbose: bool,
}

#[async_trait]
impl Connector for BleConnector {
    async fn connect(&self, name_prefix: &str, acked: bool) -> Result<Box<dyn Transport + Send>> {
        Ok(Box::new(
            BleTransport::connect(name_prefix, self.timeout, acked, self.verbose).await?,
        ))
    }
}

async fn adapter() -> Result<Adapter> {
    let manager = Manager::new().await?;
    manager
        .adapters()
        .await?
        .into_iter()
        .next()
        .context("no Bluetooth adapter found")
}

pub async fn scan(name_prefix: &str, wait: Duration) -> Result<Vec<PrinterInfo>> {
    let adapter = adapter().await?;
    adapter
        .start_scan(ScanFilter {
            services: vec![SERVICE.parse()?],
        })
        .await?;
    sleep(wait).await;
    let mut printers = Vec::new();
    for peripheral in adapter.peripherals().await? {
        let Some(properties) = peripheral.properties().await? else {
            continue;
        };
        let Some(name) = properties.local_name else {
            continue;
        };
        if name.starts_with(name_prefix) {
            printers.push(PrinterInfo {
                name,
                rssi: properties.rssi,
                id: peripheral.id().to_string(),
            });
        }
    }
    Ok(printers)
}

impl BleTransport {
    pub async fn connect(
        name_prefix: &str,
        wait: Duration,
        acked: bool,
        verbose: bool,
    ) -> Result<Self> {
        let adapter = adapter().await?;
        adapter
            .start_scan(ScanFilter {
                services: vec![SERVICE.parse()?],
            })
            .await?;
        // Advertisements (and the local name) can arrive late on CoreBluetooth, so poll
        // until the printer shows up instead of checking once after a fixed sleep.
        let deadline = tokio::time::Instant::now() + wait;
        let mut peripheral = None;
        'poll: loop {
            for candidate in adapter.peripherals().await? {
                let Some(properties) = candidate.properties().await? else {
                    continue;
                };
                if properties
                    .local_name
                    .as_deref()
                    .is_some_and(|name| name.starts_with(name_prefix))
                {
                    peripheral = Some(candidate);
                    break 'poll;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(250)).await;
        }
        let _ = adapter.stop_scan().await;
        let peripheral = peripheral.with_context(|| {
            format!(
                "no printer named '{name_prefix}*' found in {}s: check that it is on, charged and not connected to another device",
                wait.as_secs()
            )
        })?;
        peripheral.connect().await?;
        peripheral.discover_services().await?;
        let write_id = Uuid::parse_str(WRITE)?;
        let reply_id = Uuid::parse_str(REPLY)?;
        let control_id = Uuid::parse_str(CONTROL)?;
        let write = peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == write_id)
            .context("write characteristic missing")?;
        let characteristics = peripheral.characteristics();
        let reply = characteristics
            .iter()
            .find(|c| c.uuid == reply_id)
            .context("reply characteristic missing")?;
        let control = characteristics
            .iter()
            .find(|c| c.uuid == control_id)
            .context("control characteristic missing")?;
        peripheral.subscribe(reply).await?;
        peripheral.subscribe(control).await?;
        let notifications = peripheral.notifications().await?.boxed();
        Ok(Self {
            peripheral,
            write,
            notifications,
            reply_uuid: reply_id,
            acked,
            verbose,
        })
    }
}

#[async_trait]
impl Transport for BleTransport {
    async fn write(&mut self, data: &[u8]) -> Result<()> {
        if self.verbose {
            eprintln!("write: {}", hex(data));
        }
        self.peripheral
            .write(
                &self.write,
                data,
                if self.acked {
                    WriteType::WithResponse
                } else {
                    WriteType::WithoutResponse
                },
            )
            .await?;
        Ok(())
    }

    async fn read_reply(&mut self, duration: Duration) -> Result<Vec<u8>> {
        let reply = timeout(duration, async {
            let mut reply = Vec::new();
            while let Some(notification) = self.notifications.next().await {
                if self.verbose {
                    eprintln!("notify: {}", hex(&notification.value));
                }
                if notification.uuid != self.reply_uuid {
                    continue;
                }
                reply.extend_from_slice(&notification.value);
                if reply.contains(&0) {
                    return Ok::<_, anyhow::Error>(reply);
                }
            }
            bail!("notification stream ended")
        })
        .await
        .context("timed out waiting for printer reply")??;
        Ok(reply)
    }
}

fn hex(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
