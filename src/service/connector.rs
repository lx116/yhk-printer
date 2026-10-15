//! Defines the printer connection boundary used by the service actor.

use crate::transport::Transport;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(&self, name_prefix: &str, acked: bool) -> Result<Box<dyn Transport + Send>>;
}
