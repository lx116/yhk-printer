//! Implements device discovery, inspection, and paper feed commands.

use anyhow::Result;
use std::time::Duration;
use yhk_printer::{ble::scan, protocol, transport::Transport};

use super::connect;

pub(crate) async fn scan_printers(name: &str, timeout: u64) -> Result<()> {
    for printer in scan(name, Duration::from_secs(timeout)).await? {
        println!("{} rssi={:?} id={}", printer.name, printer.rssi, printer.id);
    }
    Ok(())
}

pub(crate) async fn show_info(name: &str, timeout: u64, verbose: bool) -> Result<()> {
    let mut transport = connect(name, timeout, verbose, true).await?;
    for (label, command) in [
        ("status", protocol::STATUS),
        ("serial", protocol::SERIAL),
        ("error", protocol::ERROR),
        ("product", protocol::PRODUCT),
    ] {
        transport.write(command).await?;
        let reply = transport.read_reply(Duration::from_secs(timeout)).await?;
        match label {
            "status" => println!("status: {:?}", protocol::parse_status(&reply)?),
            "serial" => println!("serial: {}", protocol::parse_serial(&reply)?),
            "error" => println!("error: {:?}", protocol::parse_error(&reply)?),
            "product" => println!("product: {}", protocol::parse_product(&reply)?),
            _ => unreachable!(),
        }
    }
    Ok(())
}

pub(crate) async fn feed(name: &str, timeout: u64, verbose: bool, lines: usize) -> Result<()> {
    let mut transport = connect(name, timeout, verbose, true).await?;
    transport.write(&protocol::feed(lines)).await?;
    Ok(())
}
