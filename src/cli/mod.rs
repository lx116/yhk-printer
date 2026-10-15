//! Dispatches parsed commands and owns shared BLE connection setup.

pub(crate) mod args;
mod device;
mod encode;
mod print;
mod ui;

use anyhow::Result;
use std::time::Duration;
use yhk_printer::ble::BleTransport;

use args::{Cli, Command};

pub(crate) async fn run(cli: Cli) -> Result<()> {
    let Cli {
        name,
        timeout,
        verbose,
        command,
    } = cli;
    match command {
        Command::Scan => device::scan_printers(&name, timeout).await,
        Command::Info => device::show_info(&name, timeout, verbose).await,
        Command::Print { image, print } => {
            print::print_images(&name, timeout, verbose, image, print).await
        }
        Command::Pattern { print } => print::print_pattern(&name, timeout, verbose, print).await,
        Command::Feed { lines } => device::feed(&name, timeout, verbose, lines).await,
        Command::Encode {
            image,
            doc,
            out,
            encode,
        } => encode::encode_to_file(image, doc, out, encode),
        Command::Doc {
            file,
            document,
            out,
            print,
        } => encode::render_document(&name, timeout, verbose, file, document, out, print).await,
        Command::Tui {
            serve,
            listen,
            token,
        } => ui::run_tui(timeout, verbose, serve, listen, token).await,
        Command::Serve {
            listen,
            token,
            allow_origin,
            no_auto_connect,
        } => {
            ui::run_server(
                timeout,
                verbose,
                listen,
                token,
                allow_origin,
                no_auto_connect,
            )
            .await
        }
    }
}

pub(crate) async fn connect(
    name: &str,
    timeout: u64,
    verbose: bool,
    acked: bool,
) -> Result<BleTransport> {
    BleTransport::connect(name, Duration::from_secs(timeout), acked, verbose).await
}
