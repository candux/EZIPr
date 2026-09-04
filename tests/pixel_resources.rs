use ezipr::{
    DecodeLimits, Decoder, ErrorKind, PixelFormat, ResourceFormat, ResourceHeader, ResourceKind,
    StorageFormat,
};

fn resource(format: ResourceFormat, width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
    let mut data = ResourceHeader::new(format, width, height)
        .unwrap()
        .to_bytes()
        .to_vec();
    data.extend_from_slice(pixels);
    data.extend_from_slice(&crc32fast::hash(pixels).to_le_bytes());
    data
}

#[test]
fn parses_resource_header_and_ignores_reserved_bits() {
    let word = 2_u32 | (0b10101 << 5) | (485 << 10) | (520 << 21);
    let header = ResourceHeader::parse(&word.to_le_bytes()).unwrap();
    assert_eq!(header.format(), ResourceFormat::EzipWithAlpha);
    assert_eq!(header.reserved(), 0b10101);
    assert_eq!(header.width(), 485);
    assert_eq!(header.height(), 520);
}

#[test]
fn rejects_unknown_format_and_zero_dimensions() {
    let unknown = 3_u32 | (1 << 10) | (1 << 21);
    assert_eq!(
        ResourceHeader::parse(&unknown.to_le_bytes())
            .unwrap_err()
            .kind(),
        ErrorKind::UnsupportedFormat
    );
    let zero_width = 4_u32 | (1 << 21);
    assert_eq!(
        ResourceHeader::parse(&zero_width.to_le_bytes())
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidDimensions
    );
}

#[test]
fn decodes_rgb565_pixel_resource() {
    let data = resource(ResourceFormat::Pixel, 3, 1, &hex("00 f8 e0 07 1f 00"));
    let decoder = Decoder::new(&data).unwrap();
    assert_eq!(decoder.info().kind(), ResourceKind::Pixel);
    assert_eq!(decoder.info().storage_format(), StorageFormat::Rgb565);
    let image = decoder.decode_frame(0, PixelFormat::Rgb8).unwrap();
    assert_eq!(image.pixels(), &[255, 0, 0, 0, 255, 0, 0, 0, 255]);
}

#[test]
fn decodes_rgb888_pixel_resource() {
    let data = resource(
        ResourceFormat::Pixel,
        3,
        1,
        &hex("00 00 ff 00 ff 00 ff 00 00"),
    );
    let decoder = Decoder::new(&data).unwrap();
    assert_eq!(decoder.info().storage_format(), StorageFormat::Rgb888);
    let image = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!(
        image.pixels(),
        &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]
    );
}

#[test]
fn decodes_argb565_pixel_resource() {
    let data = resource(
        ResourceFormat::PixelWithAlpha,
        2,
        1,
        &hex("00 f8 20 1f 00 80"),
    );
    let decoder = Decoder::new(&data).unwrap();
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb565);
    let image = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!(image.pixels(), &[255, 0, 0, 32, 0, 0, 255, 128]);
}

#[test]
fn decodes_argb888_pixel_resource() {
    let data = resource(
        ResourceFormat::PixelWithAlpha,
        2,
        1,
        &hex("1e 14 0a 28 46 3c 32 50"),
    );
    let decoder = Decoder::new(&data).unwrap();
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb888);
    let image = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!(image.pixels(), &[10, 20, 30, 40, 50, 60, 70, 80]);
}

#[test]
fn enforces_decode_limits() {
    let data = resource(ResourceFormat::Pixel, 2, 1, &hex("00 00 00 00"));
    let options = ezipr::DecodeOptions::new().limits(DecodeLimits::new().max_dimensions(1, 1));
    assert_eq!(
        Decoder::with_options(&data, options).unwrap_err().kind(),
        ErrorKind::LimitExceeded
    );
}

#[test]
fn validates_pixel_crc32() {
    let mut data = resource(ResourceFormat::Pixel, 1, 1, &hex("00 f8"));
    *data.last_mut().unwrap() ^= 0xff;
    assert_eq!(
        Decoder::new(&data).unwrap_err().kind(),
        ErrorKind::ChecksumMismatch
    );

    let options = ezipr::DecodeOptions::new().mode(ezipr::DecodeMode::Diagnostic);
    let decoder = Decoder::with_options(&data, options).unwrap();
    assert_eq!(decoder.warnings().len(), 1);
    assert_eq!(
        decoder.warnings()[0].kind(),
        ezipr::WarningKind::ChecksumMismatch
    );
}

#[test]
fn decodes_external_pixel_layouts() {
    let cases = [
        (
            include_bytes!("fixtures/static/pixel-rgb565.bin").as_slice(),
            StorageFormat::Rgb565,
        ),
        (
            include_bytes!("fixtures/static/pixel-rgb888.bin").as_slice(),
            StorageFormat::Rgb888,
        ),
        (
            include_bytes!("fixtures/static/pixel-argb565.bin").as_slice(),
            StorageFormat::Argb565,
        ),
        (
            include_bytes!("fixtures/static/pixel-argb888.bin").as_slice(),
            StorageFormat::Argb888,
        ),
    ];

    for (data, expected_format) in cases {
        let decoder = Decoder::new(data).unwrap();
        assert_eq!((decoder.info().width(), decoder.info().height()), (8, 4));
        assert_eq!(decoder.info().storage_format(), expected_format);
        assert_eq!(
            decoder
                .decode_frame(0, PixelFormat::Rgba8)
                .unwrap()
                .pixels()
                .len(),
            8 * 4 * 4
        );
    }
}

#[test]
fn external_rgb888_pixels_match_source_exactly() {
    let colors = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 255],
        [0, 0, 0],
        [17, 33, 65],
        [123, 45, 210],
        [250, 128, 7],
    ];
    let mut expected = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            expected.extend_from_slice(&colors[(x + y * 2) % colors.len()]);
        }
    }

    let decoder = Decoder::new(include_bytes!("fixtures/static/pixel-rgb888.bin")).unwrap();
    let image = decoder.decode_frame(0, PixelFormat::Rgb8).unwrap();
    assert_eq!(image.pixels(), expected);
}

#[test]
fn external_argb888_pixels_and_alpha_match_source_exactly() {
    let colors = [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 255],
        [0, 0, 0],
        [17, 33, 65],
        [123, 45, 210],
        [250, 128, 7],
    ];
    let alphas = [0, 1, 31, 64, 127, 128, 200, 255];
    let mut expected = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            expected.extend_from_slice(&colors[(x + y * 2) % colors.len()]);
            expected.push(alphas[(x + y * 3) % alphas.len()]);
        }
    }

    let decoder = Decoder::new(include_bytes!("fixtures/static/pixel-argb888.bin")).unwrap();
    let image = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!(image.pixels(), expected);
}

#[test]
fn external_rgb565_color_expansion_is_consistent_with_argb565() {
    let opaque = Decoder::new(include_bytes!("fixtures/static/pixel-rgb565.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    let alpha = Decoder::new(include_bytes!("fixtures/static/pixel-argb565.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert_eq!(opaque.pixels(), alpha.pixels());
}

fn hex(value: &str) -> Vec<u8> {
    value
        .split_ascii_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
