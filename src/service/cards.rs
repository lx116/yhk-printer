//! Holds pending card layouts until the in-process TUI takes ownership.

use crate::{imaging::RasterImage, service::JobStatus};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct CardItem {
    pub id: u64,
    pub layout: String,
    pub jobs: Vec<RasterImage>,
    pub instruction: String,
}

#[derive(Clone, Debug)]
pub struct CardDraft {
    pub layout: String,
    pub jobs: Vec<RasterImage>,
    pub instruction: String,
}

pub(super) async fn take_cards(
    cards: &RwLock<Vec<CardItem>>,
    jobs: &RwLock<Vec<crate::service::JobInfo>>,
) -> Vec<CardItem> {
    let cards = std::mem::take(&mut *cards.write().await);
    let ids = cards.iter().map(|card| card.id).collect::<Vec<_>>();
    for job in jobs
        .write()
        .await
        .iter_mut()
        .filter(|job| ids.contains(&job.id))
    {
        job.status = JobStatus::Cancelled;
    }
    cards
}

pub(super) async fn queue_card(
    cards: &Arc<RwLock<Vec<CardItem>>>,
    jobs: &Arc<RwLock<Vec<crate::service::JobInfo>>>,
    id: u64,
    card: CardDraft,
) {
    jobs.write().await.push(crate::service::JobInfo {
        id,
        status: JobStatus::Queued,
        bytes_sent: 0,
        bytes_total: 0,
    });
    cards.write().await.push(CardItem {
        id,
        layout: card.layout,
        jobs: card.jobs,
        instruction: card.instruction,
    });
}
