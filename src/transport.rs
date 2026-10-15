use anyhow::{bail, Result};
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait Transport: Send {
    async fn write(&mut self, data: &[u8]) -> Result<()>;
    async fn read_reply(&mut self, timeout: Duration) -> Result<Vec<u8>>;
}

#[async_trait]
pub trait Sleeper {
    async fn sleep(&mut self, duration: Duration);
}

pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Pacing {
    pub chunk: usize,
    pub delay: Duration,
    pub row_delay: Duration,
}

pub struct PacingWriter<T, S> {
    transport: T,
    sleeper: S,
    pacing: Pacing,
}

impl<T: Transport, S: Sleeper> PacingWriter<T, S> {
    pub fn new(transport: T, sleeper: S, pacing: Pacing) -> Result<Self> {
        if pacing.chunk == 0 {
            bail!("chunk must be greater than zero");
        }
        Ok(Self {
            transport,
            sleeper,
            pacing,
        })
    }

    pub async fn send(&mut self, data: &[u8], row_ends: &[usize]) -> Result<()> {
        let mut offset = 0;
        while offset < data.len() {
            let chunk_end = (offset + self.pacing.chunk).min(data.len());
            let end = row_ends
                .iter()
                .copied()
                .find(|&row_end| row_end > offset && row_end < chunk_end)
                .unwrap_or(chunk_end);
            self.transport.write(&data[offset..end]).await?;
            if !self.pacing.delay.is_zero() {
                self.sleeper.sleep(self.pacing.delay).await;
            }
            if row_ends.contains(&end) && !self.pacing.row_delay.is_zero() {
                self.sleeper.sleep(self.pacing.row_delay).await;
            }
            offset = end;
        }
        Ok(())
    }

    pub fn into_parts(self) -> (T, S) {
        (self.transport, self.sleeper)
    }
}
