//! Wires the interactive TUI and HTTP server commands to printer services.

use anyhow::Result;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use yhk_printer::{ble::BleConnector, service::PrinterService, tui, web};

pub(crate) async fn run_tui(
    timeout: u64,
    verbose: bool,
    start_server: bool,
    listen: SocketAddr,
    token: Option<String>,
) -> Result<()> {
    let service = service(timeout, verbose);
    if start_server {
        tokio::spawn(web::serve(service.clone(), listen, token, Vec::new()));
    }
    tui::run(service).await
}

pub(crate) async fn run_server(
    timeout: u64,
    verbose: bool,
    listen: SocketAddr,
    token: Option<String>,
    allow_origin: Vec<String>,
    no_auto_connect: bool,
) -> Result<()> {
    let service = service(timeout, verbose);
    if !no_auto_connect {
        service.connect().await?;
    }
    web::serve(service, listen, token, allow_origin).await
}

fn service(timeout: u64, verbose: bool) -> PrinterService {
    PrinterService::new(Arc::new(BleConnector {
        timeout: Duration::from_secs(timeout),
        verbose,
    }))
}
