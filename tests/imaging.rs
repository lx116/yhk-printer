use image::{DynamicImage, GrayImage as ImageBuffer, Luma};
use yhk_printer::imaging::{prepare_image, MAX_HEIGHT};

#[test]
fn pipeline_resizes_and_packs_black_msb_first() {
    let input = ImageBuffer::from_fn(2, 1, |x, _| if x == 0 { Luma([0]) } else { Luma([255]) });
    let prepared = prepare_image(DynamicImage::ImageLuma8(input), 384, None).unwrap();

    assert_eq!((prepared.gray.width, prepared.gray.height), (384, 192));
    assert_eq!(prepared.raster.data.len(), 48 * 192);
    assert_eq!(prepared.raster.data[0] & 0x80, 0x80);
    assert_eq!(prepared.raster.data[47] & 0x01, 0x00);
}

#[test]
fn pipeline_rejects_images_over_height_limit() {
    let input = ImageBuffer::from_pixel(1, MAX_HEIGHT + 1, Luma([255]));
    let error = prepare_image(DynamicImage::ImageLuma8(input), 8, None).unwrap_err();
    assert!(error.to_string().contains("16000"));
}
