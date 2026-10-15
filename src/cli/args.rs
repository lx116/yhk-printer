//! Defines the command-line interface without performing any command work.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use yhk_printer::{
    imaging,
    service::BatchMode,
    transport::Pacing,
    typeset::{Align, Font},
};

#[derive(Parser)]
#[command(name = "yhkp", about = "Print images on YHK BLE thermal printers")]
pub(crate) struct Cli {
    #[arg(long, global = true, default_value = "YHK-")]
    pub(crate) name: String,
    #[arg(long, global = true, default_value_t = 20)]
    pub(crate) timeout: u64,
    #[arg(short, long, global = true)]
    pub(crate) verbose: bool,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    Scan,
    Info,
    Print {
        image: Vec<PathBuf>,
        #[command(flatten)]
        print: PrintArgs,
    },
    Pattern {
        #[command(flatten)]
        print: PrintArgs,
    },
    Feed {
        #[arg(long, default_value_t = 4)]
        lines: usize,
    },
    Encode {
        image: Vec<PathBuf>,
        #[arg(long)]
        doc: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        encode: EncodeArgs,
    },
    Doc {
        file: PathBuf,
        #[command(flatten)]
        document: DocArgs,
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        print: PrintArgs,
    },
    Tui {
        #[arg(long)]
        serve: bool,
        #[arg(long, default_value = "127.0.0.1:7731")]
        listen: SocketAddr,
        #[arg(long)]
        token: Option<String>,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:7731")]
        listen: SocketAddr,
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        allow_origin: Vec<String>,
        #[arg(long)]
        no_auto_connect: bool,
    },
}

#[derive(Args, Clone)]
pub(crate) struct DocArgs {
    #[arg(long, value_enum, default_value_t = CliFont::Sans)]
    pub(crate) font: CliFont,
    #[arg(long, default_value_t = 10.0)]
    pub(crate) size: f32,
    #[arg(long, value_enum, default_value_t = CliAlign::Left)]
    pub(crate) align: CliAlign,
    #[arg(long, default_value_t = 12)]
    pub(crate) margin: u16,
    #[arg(long, default_value_t = 1.25)]
    pub(crate) line_height: f32,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliFont {
    Sans,
    Hand,
    Mono,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliAlign {
    Left,
    Center,
    Right,
    Justify,
}

impl From<CliFont> for Font {
    fn from(value: CliFont) -> Self {
        match value {
            CliFont::Sans => Self::Sans,
            CliFont::Hand => Self::Hand,
            CliFont::Mono => Self::Mono,
        }
    }
}

impl From<CliAlign> for Align {
    fn from(value: CliAlign) -> Self {
        match value {
            CliAlign::Left => Self::Left,
            CliAlign::Center => Self::Center,
            CliAlign::Right => Self::Right,
            CliAlign::Justify => Self::Justify,
        }
    }
}

#[derive(Args)]
pub(crate) struct EncodeArgs {
    #[arg(long, value_enum, default_value_t = Mode::Rows)]
    pub(crate) mode: Mode,
    #[arg(long)]
    pub(crate) density: Option<u8>,
    #[arg(long, value_enum)]
    pub(crate) quality: Option<CliQuality>,
    #[arg(long, value_enum, default_value_t = CliContent::Auto)]
    pub(crate) content: CliContent,
    #[arg(long, value_enum, default_value_t = CliDefinition::Standard)]
    pub(crate) definition: CliDefinition,
    #[arg(long)]
    pub(crate) gamma: Option<f32>,
    #[command(flatten)]
    pub(crate) batch: BatchArgs,
}

#[derive(Args)]
pub(crate) struct BatchArgs {
    #[arg(long, value_enum)]
    pub(crate) batch: Option<CliBatchMode>,
    #[arg(long)]
    pub(crate) gap_mm: Option<f32>,
    #[arg(long, hide = true)]
    pub(crate) gap_rows: Option<u16>,
}

#[derive(Args)]
pub(crate) struct PrintArgs {
    #[command(flatten)]
    pub(crate) encode: EncodeArgs,
    #[arg(long, default_value_t = 1)]
    pub(crate) copies: u16,
    #[command(flatten)]
    pub(crate) pacing: PacingArgs,
}

#[derive(Args)]
pub(crate) struct PacingArgs {
    #[arg(long, default_value_t = 20)]
    chunk: usize,
    #[arg(long, default_value_t = 20)]
    delay_ms: u64,
    #[arg(long, default_value_t = 0)]
    row_delay_ms: u64,
    #[arg(long, action = clap::ArgAction::SetTrue, conflicts_with = "no_acked")]
    acked: bool,
    #[arg(long, action = clap::ArgAction::SetTrue, conflicts_with = "acked")]
    no_acked: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Mode {
    Rows,
    Raster,
    Gray,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliBatchMode {
    Advance,
    Pause,
    Roll,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliQuality {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliContent {
    Auto,
    Photo,
    Text,
    Qr,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CliDefinition {
    Standard,
    Hd,
}

impl From<CliContent> for imaging::ContentKind {
    fn from(value: CliContent) -> Self {
        match value {
            CliContent::Auto => Self::Auto,
            CliContent::Photo => Self::Photo,
            CliContent::Text => Self::Text,
            CliContent::Qr => Self::Qr,
        }
    }
}

impl From<CliDefinition> for imaging::Definition {
    fn from(value: CliDefinition) -> Self {
        match value {
            CliDefinition::Standard => Self::Standard,
            CliDefinition::Hd => Self::Hd,
        }
    }
}

impl From<CliBatchMode> for BatchMode {
    fn from(value: CliBatchMode) -> Self {
        match value {
            CliBatchMode::Advance => Self::Advance,
            CliBatchMode::Pause => Self::Pause,
            CliBatchMode::Roll => Self::Roll,
        }
    }
}

impl PacingArgs {
    pub(crate) fn pacing(&self) -> Pacing {
        Pacing {
            chunk: self.chunk,
            delay: Duration::from_millis(self.delay_ms),
            row_delay: Duration::from_millis(self.row_delay_ms),
        }
    }

    pub(crate) fn acked(&self) -> bool {
        self.acked || !self.no_acked
    }
}
