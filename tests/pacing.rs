use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use yhk_printer::{
    imaging::RasterImage,
    protocol::encode_rows,
    transport::{Pacing, PacingWriter, Sleeper, Transport},
};

#[derive(Default)]
struct FakeTransport {
    writes: Vec<Vec<u8>>,
}

#[async_trait]
impl Transport for FakeTransport {
    async fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writes.push(data.to_vec());
        Ok(())
    }

    async fn read_reply(&mut self, _timeout: Duration) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct RecordingSleeper {
    delays: Vec<Duration>,
}

#[async_trait]
impl Sleeper for RecordingSleeper {
    async fn sleep(&mut self, duration: Duration) {
        self.delays.push(duration);
    }
}

#[tokio::test]
async fn payload_is_written_in_order_in_configured_chunks() {
    let transport = FakeTransport::default();
    let sleeper = RecordingSleeper::default();
    let mut writer = PacingWriter::new(
        transport,
        sleeper,
        Pacing {
            chunk: 20,
            delay: Duration::from_millis(20),
            row_delay: Duration::ZERO,
        },
    )
    .unwrap();

    writer
        .send(&(0_u8..45).collect::<Vec<_>>(), &[])
        .await
        .unwrap();
    let (transport, sleeper) = writer.into_parts();

    assert_eq!(
        transport.writes.iter().map(Vec::len).collect::<Vec<_>>(),
        [20, 20, 5]
    );
    assert_eq!(transport.writes.concat(), (0_u8..45).collect::<Vec<_>>());
    assert_eq!(sleeper.delays, vec![Duration::from_millis(20); 3]);
}

#[tokio::test]
async fn row_delay_occurs_at_a_row_boundary_even_inside_a_chunk() {
    let mut writer = PacingWriter::new(
        FakeTransport::default(),
        RecordingSleeper::default(),
        Pacing {
            chunk: 20,
            delay: Duration::from_millis(20),
            row_delay: Duration::from_millis(7),
        },
    )
    .unwrap();

    writer.send(&[0; 30], &[12]).await.unwrap();
    let (transport, sleeper) = writer.into_parts();

    assert_eq!(
        transport.writes.iter().map(Vec::len).collect::<Vec<_>>(),
        [12, 18]
    );
    assert_eq!(
        sleeper.delays,
        [
            Duration::from_millis(20),
            Duration::from_millis(7),
            Duration::from_millis(20),
        ]
    );
}

#[test]
fn zero_chunk_is_rejected() {
    let result = PacingWriter::new(
        FakeTransport::default(),
        RecordingSleeper::default(),
        Pacing {
            chunk: 0,
            delay: Duration::ZERO,
            row_delay: Duration::ZERO,
        },
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn rows_jobs_delay_after_each_complete_row_command() {
    let job = encode_rows(
        &RasterImage {
            width: 384,
            height: 2,
            data: vec![0; 96],
        },
        10,
    )
    .unwrap();
    let mut writer = PacingWriter::new(
        FakeTransport::default(),
        RecordingSleeper::default(),
        Pacing {
            chunk: 100,
            delay: Duration::ZERO,
            row_delay: Duration::from_millis(7),
        },
    )
    .unwrap();
    let row_ends = job
        .raster_rows
        .iter()
        .map(|row| row.end)
        .collect::<Vec<_>>();

    writer.send(&job.bytes, &row_ends).await.unwrap();
    let (transport, sleeper) = writer.into_parts();

    assert_eq!(
        transport.writes.iter().map(Vec::len).collect::<Vec<_>>(),
        [62, 56, 4]
    );
    assert_eq!(sleeper.delays, [Duration::from_millis(7); 2]);
}
