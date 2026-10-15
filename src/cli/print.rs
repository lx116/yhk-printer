//! Prepares, encodes, and sends image print jobs.

use anyhow::{Context, Result};
use std::{path::PathBuf, time::Duration};
use yhk_printer::{
    imaging,
    protocol::{self, EncodedJob, PrintMode},
    service::{BatchMode, Quality},
    transport::{PacingWriter, Sleeper, TokioSleeper, Transport},
};

use super::{
    args::{CliQuality, EncodeArgs, Mode, PrintArgs},
    connect,
};

const DEFAULT_DENSITY: u8 = 10;
const ROWS_PER_MILLIMETRE: f32 = 8.0;
const MAX_GAP_MM: f32 = 30.0;

pub(crate) async fn print_images(
    name: &str,
    timeout: u64,
    verbose: bool,
    paths: Vec<PathBuf>,
    print: PrintArgs,
) -> Result<()> {
    let mut transport = connect(name, timeout, verbose, print.pacing.acked()).await?;
    transport.write(protocol::STATUS).await?;
    let status =
        protocol::parse_status(&transport.read_reply(Duration::from_secs(timeout)).await?)?;
    let images = paths
        .iter()
        .map(|path| {
            imaging::load_image_with_options(
                path,
                status.dpi,
                print.encode.gamma,
                process_options(&print.encode),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut writer = PacingWriter::new(transport, TokioSleeper, print.pacing.pacing())?;
    let (batch, _, _, _) = batch_options(&print.encode);
    for _ in 0..print.copies {
        send_images(&mut writer, &images, &print.encode, &batch).await?;
    }
    Ok(())
}

pub(crate) async fn print_pattern(
    name: &str,
    timeout: u64,
    verbose: bool,
    print: PrintArgs,
) -> Result<()> {
    let mut transport = connect(name, timeout, verbose, print.pacing.acked()).await?;
    transport.write(protocol::STATUS).await?;
    let status =
        protocol::parse_status(&transport.read_reply(Duration::from_secs(timeout)).await?)?;
    let pattern = imaging::pattern(status.dpi);
    let gray = image::GrayImage::from_raw(
        u32::from(pattern.gray.width),
        u32::from(pattern.gray.height),
        pattern.gray.data,
    )
    .context("pattern dimensions are valid")?;
    let image = imaging::prepare_image_with_options(
        image::DynamicImage::ImageLuma8(gray),
        status.dpi,
        print.encode.gamma,
        process_options(&print.encode),
    )?;
    let job = encode(&image, &print.encode)?;
    let mut writer = PacingWriter::new(transport, TokioSleeper, print.pacing.pacing())?;
    send_job(&mut writer, &job).await
}

async fn send_images<T: Transport, S: Sleeper>(
    writer: &mut PacingWriter<T, S>,
    images: &[imaging::PreparedImage],
    options: &EncodeArgs,
    batch: &BatchMode,
) -> Result<()> {
    match batch {
        BatchMode::Roll => send_job(writer, &encode_images(images, options)?).await?,
        BatchMode::Advance | BatchMode::Pause => {
            for (index, image) in images.iter().enumerate() {
                send_job(
                    writer,
                    &encode_images(std::slice::from_ref(image), options)?,
                )
                .await?;
                if matches!(batch, BatchMode::Pause) && index + 1 < images.len() {
                    println!("Tear the paper, then press Enter for the next image.");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn encode(image: &imaging::PreparedImage, options: &EncodeArgs) -> Result<EncodedJob> {
    print_mode(options.mode).encode(image, density(options))
}

pub(crate) fn process_options(options: &EncodeArgs) -> imaging::ProcessOptions {
    imaging::ProcessOptions {
        content: options.content.into(),
        definition: options.definition.into(),
    }
}

pub(crate) fn encode_images(
    images: &[imaging::PreparedImage],
    options: &EncodeArgs,
) -> Result<EncodedJob> {
    let (batch, gap_rows, cut_marks, lead_rows) = batch_options(options);
    if matches!(batch, BatchMode::Roll) {
        return protocol::encode_batch_with_gap(
            images,
            print_mode(options.mode),
            density(options),
            gap_rows,
            cut_marks,
            lead_rows,
        );
    }
    let jobs = images
        .iter()
        .map(|image| match options.mode {
            Mode::Rows => protocol::encode_rows_with_gap(
                &image.raster,
                density(options),
                gap_rows,
                cut_marks,
                lead_rows,
            ),
            Mode::Raster => protocol::encode_raster_with_gap(
                &image.raster,
                density(options),
                gap_rows,
                cut_marks,
                lead_rows,
            ),
            Mode::Gray => protocol::encode_gray_with_gap(
                &image.gray,
                density(options),
                gap_rows,
                cut_marks,
                lead_rows,
            ),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(EncodedJob {
        bytes: jobs
            .iter()
            .flat_map(|job| job.bytes.iter().copied())
            .collect(),
        raster_rows: Vec::new(),
    })
}

async fn send_job<T: Transport, S: Sleeper>(
    writer: &mut PacingWriter<T, S>,
    job: &EncodedJob,
) -> Result<()> {
    let row_ends = job
        .raster_rows
        .iter()
        .map(|range| range.end)
        .collect::<Vec<_>>();
    writer.send(&job.bytes, &row_ends).await
}

fn density(options: &EncodeArgs) -> u8 {
    options.density.unwrap_or_else(|| match options.quality {
        Some(CliQuality::Low) => Quality::Low.density(),
        Some(CliQuality::Medium) => Quality::Medium.density(),
        Some(CliQuality::High) => Quality::High.density(),
        None => DEFAULT_DENSITY,
    })
}

fn print_mode(mode: Mode) -> PrintMode {
    match mode {
        Mode::Rows => PrintMode::Rows,
        Mode::Raster => PrintMode::Raster,
        Mode::Gray => PrintMode::Gray,
    }
}

fn batch_options(options: &EncodeArgs) -> (BatchMode, u16, bool, u16) {
    let settings = yhk_printer::service::saved_settings();
    (
        options
            .batch
            .batch
            .map(Into::into)
            .unwrap_or(settings.batch_mode.clone()),
        gap_rows(
            options.batch.gap_mm,
            options.batch.gap_rows,
            settings.gap_rows(),
        ),
        settings.cut_marks,
        settings.lead_rows(),
    )
}

fn gap_rows(gap_mm: Option<f32>, legacy_gap_rows: Option<u16>, configured_gap_rows: u16) -> u16 {
    gap_mm
        .map(|gap| (gap.clamp(0.0, MAX_GAP_MM) * ROWS_PER_MILLIMETRE).round() as u16)
        .or(legacy_gap_rows)
        .unwrap_or(configured_gap_rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{BatchArgs, CliContent, CliDefinition};

    fn options(density: Option<u8>, quality: Option<CliQuality>) -> EncodeArgs {
        EncodeArgs {
            mode: Mode::Rows,
            density,
            quality,
            content: CliContent::Auto,
            definition: CliDefinition::Standard,
            gamma: None,
            batch: BatchArgs {
                batch: None,
                gap_mm: None,
                gap_rows: None,
            },
        }
    }

    #[test]
    fn explicit_density_overrides_quality() {
        assert_eq!(density(&options(Some(17), Some(CliQuality::High))), 17);
    }

    #[test]
    fn legacy_gap_rows_is_used_when_millimetres_are_absent() {
        assert_eq!(gap_rows(None, Some(13), 4), 13);
    }

    #[test]
    fn millimetres_override_the_legacy_gap_rows_alias() {
        assert_eq!(gap_rows(Some(1.25), Some(13), 4), 10);
    }
}
