//! Estimates print duration from encoded row count and pacing settings.

use crate::{protocol::PrintMode, service::Settings};

const ACK_MS: u64 = 30;
const MILLIS_PER_SECOND: u64 = 1_000;

/// Rough wall-clock estimate of a print job over BLE, in seconds.
///
/// Calibrated on the real printer: 14,346 bytes took about 36 s at chunk 20 and
/// 20 ms delay with acknowledged writes, which is roughly 50 ms per chunk.
pub fn estimate_print_seconds(rows: u32, settings: &Settings) -> u32 {
    let bytes_per_row: u64 = match settings.mode {
        PrintMode::Rows => 8 + 48,
        PrintMode::Raster => 48,
        PrintMode::Gray => 4 + 384,
    };
    let bytes = u64::from(rows) * bytes_per_row;
    let chunk = u64::try_from(settings.chunk).unwrap_or(u64::MAX).max(1);
    let per_chunk = settings.delay_ms + u64::from(settings.acked) * ACK_MS;
    let millis = bytes.div_ceil(chunk) * per_chunk + u64::from(rows) * settings.row_delay_ms;
    u32::try_from(millis.div_ceil(MILLIS_PER_SECOND)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_time_estimate_follows_bytes_chunk_and_delay() {
        let settings = Settings::default();
        let slow = estimate_print_seconds(902, &settings);
        assert!(
            (110..=145).contains(&slow),
            "902 rows at the default pacing should take about two minutes, got {slow}s"
        );
        let fast = estimate_print_seconds(
            902,
            &Settings {
                chunk: 100,
                delay_ms: 10,
                ..settings.clone()
            },
        );
        assert!(
            fast * 4 < slow,
            "bigger chunks and shorter pauses must be faster"
        );
        assert_eq!(estimate_print_seconds(0, &settings), 0);
    }
}
