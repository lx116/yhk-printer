//! Implements file-only encoding, document loading, and document previews.

use anyhow::{Context, Result};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use yhk_printer::{
    document, imaging, protocol,
    transport::{PacingWriter, TokioSleeper, Transport},
    typeset::{self, validate_document_size, TypesetOptions},
};

use super::{
    args::{DocArgs, EncodeArgs, PrintArgs},
    connect,
    print::{encode_images, process_options},
};

pub(crate) fn encode_to_file(
    image_paths: Vec<PathBuf>,
    document_path: Option<PathBuf>,
    output_path: PathBuf,
    options: EncodeArgs,
) -> Result<()> {
    if image_paths.is_empty() && document_path.is_none() {
        anyhow::bail!("no input: pass at least one IMAGE or --doc");
    }
    let mut images = load_images(&image_paths, &options)?;
    if let Some(path) = document_path {
        images.extend(load_document(
            &path,
            &DocArgs {
                font: super::args::CliFont::Sans,
                size: 10.0,
                align: super::args::CliAlign::Left,
                margin: 12,
                line_height: 1.25,
            },
        )?);
    }
    std::fs::write(output_path, encode_images(&images, &options)?.bytes)?;
    Ok(())
}

pub(crate) async fn render_document(
    name: &str,
    timeout: u64,
    verbose: bool,
    file: PathBuf,
    document: DocArgs,
    output: Option<PathBuf>,
    print: PrintArgs,
) -> Result<()> {
    let pages = load_document(&file, &document)?;
    if let Some(path) = output {
        return write_preview(&path, pages);
    }
    print_document(name, timeout, verbose, pages, print).await
}

fn load_images(paths: &[PathBuf], options: &EncodeArgs) -> Result<Vec<imaging::PreparedImage>> {
    paths
        .iter()
        .map(|path| {
            imaging::load_image_with_options(
                path,
                imaging::PRINTER_WIDTH_DOTS,
                options.gamma,
                process_options(options),
            )
        })
        .collect()
}

pub(crate) fn load_document(path: &Path, options: &DocArgs) -> Result<Vec<imaging::PreparedImage>> {
    validate_document_size(options.size).map_err(anyhow::Error::msg)?;
    let text = read_document(path)?;
    let document = match path.extension().and_then(|extension| extension.to_str()) {
        Some("md" | "markdown" | "MD" | "MARKDOWN") => document::parse_markdown(&text),
        _ => document::parse_text(&text),
    };
    let options = TypesetOptions {
        font: options.font.into(),
        size_pt: options.size,
        align: options.align.into(),
        margin_dots: options.margin,
        line_height: options.line_height,
        ..TypesetOptions::default()
    };
    Ok(typeset::typeset(&document, &options)
        .into_iter()
        .map(typeset::prepared)
        .collect())
}

fn read_document(path: &Path) -> Result<String> {
    if path.as_os_str() != "-" {
        return Ok(std::fs::read_to_string(path)?);
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text)
}

fn write_preview(path: &Path, pages: Vec<imaging::PreparedImage>) -> Result<()> {
    let height = pages.iter().map(|page| u32::from(page.gray.height)).sum();
    let mut output = image::GrayImage::from_pixel(
        u32::from(imaging::PRINTER_WIDTH_DOTS),
        height,
        image::Luma([255]),
    );
    let mut y = 0;
    for page in pages {
        let image = image::GrayImage::from_raw(
            u32::from(page.gray.width),
            u32::from(page.gray.height),
            page.gray.data,
        )
        .context("typeset image dimensions are valid")?;
        image::imageops::replace(&mut output, &image, 0, i64::from(y));
        y += image.height();
    }
    output.save(path)?;
    Ok(())
}

async fn print_document(
    name: &str,
    timeout: u64,
    verbose: bool,
    pages: Vec<imaging::PreparedImage>,
    print: PrintArgs,
) -> Result<()> {
    let mut transport = connect(name, timeout, verbose, print.pacing.acked()).await?;
    transport.write(protocol::STATUS).await?;
    let _ = protocol::parse_status(&transport.read_reply(Duration::from_secs(timeout)).await?)?;
    let mut writer = PacingWriter::new(transport, TokioSleeper, print.pacing.pacing())?;
    for _ in 0..print.copies {
        let job = encode_images(&pages, &print.encode)?;
        let row_ends = job
            .raster_rows
            .iter()
            .map(|range| range.end)
            .collect::<Vec<_>>();
        writer.send(&job.bytes, &row_ends).await?;
    }
    Ok(())
}
