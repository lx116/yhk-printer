use yhk_printer::imaging::{GrayImage, RasterImage};
use yhk_printer::protocol::{
    encode_gray, encode_raster, encode_rows, parse_error, parse_product, parse_serial,
    parse_status, ERROR_COMMAND, PRODUCT_COMMAND, SERIAL_COMMAND, STATUS_COMMAND,
};

#[test]
fn query_commands_match_the_protocol() {
    assert_eq!(STATUS_COMMAND, [0x1e, 0x47, 0x03]);
    assert_eq!(SERIAL_COMMAND, [0x1d, 0x67, 0x39]);
    assert_eq!(ERROR_COMMAND, [0x1d, 0x67, 0x53]);
    assert_eq!(PRODUCT_COMMAND, [0x1d, 0x67, 0x69]);
}

#[test]
fn raster_header_and_bits_are_exact() {
    let mut data = vec![0; 48 * 2];
    data[0] = 0b1000_0001;
    let job = encode_raster(
        &RasterImage {
            width: 384,
            height: 2,
            data,
        },
        0x0a,
    )
    .unwrap();

    assert_eq!(
        &job.bytes[6..14],
        &[0x1d, 0x76, 0x30, 0x00, 0x30, 0x00, 0x02, 0x00]
    );
    assert_eq!(job.bytes[14], 0b1000_0001);
    assert_eq!(&job.bytes[job.bytes.len() - 4..], &[0x0a; 4]);
}

#[test]
fn rows_encoding_has_a_one_row_command_per_384_pixel_row() {
    let mut data = vec![0; 48 * 2];
    data[0] = 0b1000_0001;
    data[48] = 0b0100_0010;
    let job = encode_rows(
        &RasterImage {
            width: 384,
            height: 2,
            data,
        },
        0x0a,
    )
    .unwrap();

    assert_eq!(job.bytes.len(), 6 + 2 * (8 + 48) + 4);
    assert_eq!(
        &job.bytes[6..14],
        &[0x1d, 0x76, 0x30, 0x00, 0x30, 0x00, 0x01, 0x00]
    );
    assert_eq!(job.bytes[14], 0b1000_0001);
    assert_eq!(
        &job.bytes[62..70],
        &[0x1d, 0x76, 0x30, 0x00, 0x30, 0x00, 0x01, 0x00]
    );
    assert_eq!(job.bytes[70], 0b0100_0010);
    assert_eq!(job.raster_rows, [6..62, 62..118]);
    assert_eq!(&job.bytes[job.bytes.len() - 4..], &[0x0a; 4]);
}

#[test]
fn gray_rows_and_tail_are_exact() {
    let job = encode_gray(
        &GrayImage {
            width: 384,
            height: 1,
            data: vec![0; 384],
        },
        0x0a,
    )
    .unwrap();

    assert_eq!(&job.bytes[6..10], &[0x1d, 0x78, 0x80, 0x01]);
    let white_row = [vec![0x1d, 0x78, 0x80, 0x01], vec![0xff; 384]].concat();
    let white_start = 6 + 4 + 384;
    assert_eq!(&job.bytes[white_start..white_start + 388], white_row);
    assert_eq!(job.bytes.len(), 6 + 101 * 388 + 7);
    assert_eq!(
        &job.bytes[job.bytes.len() - 7..],
        &[0x1b, 0x23, 0x23, 0x43, 0x4c, 0x45, 0x4e]
    );
}

#[test]
fn reply_parsers_accept_captured_examples() {
    let status = parse_status(b"HV=H1.0,SV=V1.01,VOLT=7640mv,DPI=384,\0").unwrap();
    assert_eq!(status.hw, "H1.0");
    assert_eq!(status.sw, "V1.01");
    assert_eq!(status.volt_mv, 7640);
    assert_eq!(status.dpi, 384);
    assert_eq!(status.row_bytes(), 48);
    assert_eq!(
        parse_serial(b"sn:XXXXXXXXXXXXXXXX.\0").unwrap(),
        "XXXXXXXXXXXXXXXX"
    );
    assert_eq!(parse_error(b"err:\0.").unwrap(), None);
    assert_eq!(parse_error(b"err:\x02.\0").unwrap(), Some(0x02));
    assert_eq!(parse_product(b"public id:0212.\0").unwrap(), "0212");
}
