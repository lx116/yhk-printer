use std::fs;
use std::process::Command;
use yhk_printer::imaging::load_image;
use yhk_printer::protocol::{
    encode_batch_with_gap, encode_raster_with_gap, encode_rows_with_gap, PrintMode,
};

fn test_directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "yhkp-cli-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn encode_subcommand_matches_library_encoder() {
    let directory = test_directory();
    let image_path = directory.join("input.png");
    let output_path = directory.join("job.bin");
    image::GrayImage::from_pixel(2, 1, image::Luma([0]))
        .save(&image_path)
        .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args([
            "encode",
            image_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--mode",
            "raster",
            "--gap-mm",
            "5",
        ])
        .status()
        .unwrap();

    assert!(status.success());
    let prepared = load_image(&image_path, 384, None).unwrap();
    let expected = encode_raster_with_gap(&prepared.raster, 0x0a, 40, true, 0).unwrap();
    assert_eq!(fs::read(output_path).unwrap(), expected.bytes);
}

#[test]
fn encode_subcommand_defaults_to_rows() {
    let directory = test_directory();
    let image_path = directory.join("default-input.png");
    let output_path = directory.join("default-job.bin");
    image::GrayImage::from_pixel(2, 1, image::Luma([0]))
        .save(&image_path)
        .unwrap();

    assert!(Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args([
            "encode",
            image_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--gap-mm",
            "5",
        ])
        .status()
        .unwrap()
        .success());
    let prepared = load_image(&image_path, 384, None).unwrap();
    assert_eq!(
        fs::read(output_path).unwrap(),
        encode_rows_with_gap(&prepared.raster, 0x0a, 40, true, 0)
            .unwrap()
            .bytes
    );
}

#[test]
fn encode_roll_writes_one_batch_for_two_images() {
    let directory = test_directory();
    let first_path = directory.join("roll-one.png");
    let second_path = directory.join("roll-two.png");
    let output_path = directory.join("roll-job.bin");
    image::GrayImage::from_pixel(2, 1, image::Luma([0]))
        .save(&first_path)
        .unwrap();
    image::GrayImage::from_pixel(2, 1, image::Luma([255]))
        .save(&second_path)
        .unwrap();

    assert!(Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args([
            "encode",
            first_path.to_str().unwrap(),
            second_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--batch",
            "roll",
            "--gap-rows",
            "2",
        ])
        .status()
        .unwrap()
        .success());

    let images = [
        load_image(&first_path, 384, None).unwrap(),
        load_image(&second_path, 384, None).unwrap(),
    ];
    assert_eq!(
        fs::read(output_path).unwrap(),
        encode_batch_with_gap(&images, PrintMode::Rows, 10, 2, true, 0)
            .unwrap()
            .bytes
    );
}

#[test]
fn encode_accepts_quality_content_and_definition_flags() {
    let directory = test_directory();
    let image_path = directory.join("flags-input.png");
    let output_path = directory.join("flags-job.bin");
    image::GrayImage::from_pixel(8, 8, image::Luma([0]))
        .save(&image_path)
        .unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args([
            "encode",
            image_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--quality",
            "high",
            "--density",
            "17",
            "--content",
            "text",
            "--definition",
            "hd",
        ])
        .status()
        .unwrap()
        .success());
    assert_eq!(
        &fs::read(output_path).unwrap()[..4],
        &[0x1d, 0x49, 0xf0, 17]
    );
}

#[test]
fn encode_requires_an_image_or_document() {
    let directory = test_directory();
    let output_path = directory.join("empty.bin");
    let output = Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args(["encode", "--out", output_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("no input: pass at least one IMAGE or --doc"));
    assert!(!output_path.exists());
}

#[test]
fn doc_rejects_an_invalid_font_size() {
    let directory = test_directory();
    let input_path = directory.join("document.md");
    let output_path = directory.join("document.png");
    fs::write(&input_path, "# Heading").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args([
            "doc",
            input_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
            "--size",
            "0",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("size must be between 4.0 and 48.0 points"));
    assert!(!output_path.exists());
}

#[test]
fn doc_preview_writes_a_384_pixel_png_from_stdin() {
    let directory = test_directory();
    let output_path = directory.join("document.png");
    let mut child = Command::new(env!("CARGO_BIN_EXE_yhkp"))
        .args(["doc", "-", "--out", output_path.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Hello document")
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(image::open(&output_path).unwrap().width(), 384);
}

#[test]
fn doc_preview_treats_an_unknown_extension_as_plain_text() {
    let directory = test_directory();
    let input_path = directory.join("document.unknown");
    let text_path = directory.join("document.txt");
    let output_path = directory.join("document-unknown.png");
    let text_output_path = directory.join("document-text.png");
    fs::write(&input_path, "**literal markdown**").unwrap();
    fs::write(&text_path, "**literal markdown**").unwrap();

    for (input, output) in [(&input_path, &output_path), (&text_path, &text_output_path)] {
        assert!(Command::new(env!("CARGO_BIN_EXE_yhkp"))
            .args([
                "doc",
                input.to_str().unwrap(),
                "--out",
                output.to_str().unwrap(),
            ])
            .status()
            .unwrap()
            .success());
    }
    assert_eq!(image::open(&output_path).unwrap().width(), 384);
    assert_eq!(
        fs::read(output_path).unwrap(),
        fs::read(text_output_path).unwrap()
    );
}
