use ezipr::{
    AlphaMode, ColorDepth, DecodeMode, DecodeOptions, Decoder, EncodeOptions, Encoder, ImageView,
    PixelFormat, ResourceEncoding, StorageFormat, StreamHeader,
};

const RGBA: &[u8] = &[
    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255, 17, 33, 65, 200, 123, 45,
    210, 1,
];

fn image() -> ImageView<'static> {
    ImageView::new(3, 2, PixelFormat::Rgba8, 12, RGBA).unwrap()
}

#[test]
fn resolves_depth_and_alpha_policy_to_all_storage_formats() {
    let cases = [
        (
            ColorDepth::Rgb565,
            AlphaMode::Discard,
            StorageFormat::Rgb565,
        ),
        (
            ColorDepth::Rgb888,
            AlphaMode::Discard,
            StorageFormat::Rgb888,
        ),
        (
            ColorDepth::Rgb565,
            AlphaMode::Preserve,
            StorageFormat::Argb565,
        ),
        (
            ColorDepth::Rgb888,
            AlphaMode::Preserve,
            StorageFormat::Argb888,
        ),
    ];
    for (depth, alpha, expected) in cases {
        let encoded = Encoder::new(EncodeOptions::new(depth).alpha_mode(alpha))
            .encode(image())
            .unwrap();
        assert_eq!(encoded.storage_format(), expected);
        assert_eq!(
            Decoder::new(encoded.as_bytes())
                .unwrap()
                .info()
                .storage_format(),
            expected
        );
    }
}

#[test]
fn auto_alpha_reports_the_resolved_format() {
    let encoded = Encoder::new(EncodeOptions::new(ColorDepth::Rgb565))
        .encode(image())
        .unwrap();
    assert_eq!(encoded.storage_format(), StorageFormat::Argb565);

    let opaque = [10, 20, 30, 255];
    let opaque = ImageView::new(1, 1, PixelFormat::Rgba8, 4, &opaque).unwrap();
    let encoded = Encoder::new(EncodeOptions::new(ColorDepth::Rgb565))
        .encode(opaque)
        .unwrap();
    assert_eq!(encoded.storage_format(), StorageFormat::Rgb565);
}

#[test]
fn emits_standard_filtered_and_filterless_streams() {
    for filters in [true, false] {
        let options = EncodeOptions::new(ColorDepth::Rgb888)
            .alpha_mode(AlphaMode::Discard)
            .row_filters(filters);
        let encoded = Encoder::new(options).encode(image()).unwrap();
        let stream = StreamHeader::parse(&encoded.as_bytes()[4..]).unwrap();
        assert_eq!(stream.control(), 0x12);
        assert_eq!(stream.bit_depth(), 8);
        assert_eq!(stream.has_row_filters(), filters);
        assert_eq!(stream.data_size() as usize, encoded.as_bytes().len() - 4);

        let decoded = Decoder::new(encoded.as_bytes())
            .unwrap()
            .decode_frame(0, PixelFormat::Rgb8)
            .unwrap();
        let expected: Vec<_> = RGBA
            .chunks_exact(4)
            .flat_map(|pixel| pixel[..3].iter().copied())
            .collect();
        assert_eq!(decoded.pixels(), expected);
    }
}

#[test]
fn emits_pixel_resources_with_valid_crc() {
    let options = EncodeOptions::new(ColorDepth::Rgb888)
        .alpha_mode(AlphaMode::Preserve)
        .resource_encoding(ResourceEncoding::Pixel);
    let encoded = Encoder::new(options).encode(image()).unwrap();
    assert_eq!(encoded.storage_format(), StorageFormat::Argb888);
    let decoded = Decoder::new(encoded.as_bytes())
        .unwrap()
        .decode_frame(0, PixelFormat::Rgba8)
        .unwrap();
    assert_eq!(decoded.pixels(), RGBA);
}

#[test]
fn respects_input_stride() {
    let padded = [255, 0, 0, 99, 99, 0, 255, 0, 88, 88];
    let image = ImageView::new(1, 2, PixelFormat::Rgb8, 5, &padded).unwrap();
    let encoded = Encoder::default().encode(image).unwrap();
    let decoded = Decoder::new(encoded.as_bytes())
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert_eq!(decoded.pixels(), &[255, 0, 0, 0, 255, 0]);
}

#[test]
fn encoded_checksum_is_enforced_independently() {
    let mut bytes = Encoder::default().encode(image()).unwrap().into_bytes();
    *bytes.last_mut().unwrap() ^= 1;
    assert!(Decoder::new(&bytes).is_err());

    let diagnostic = DecodeOptions::new().mode(DecodeMode::Diagnostic);
    assert_eq!(
        Decoder::with_options(&bytes, diagnostic)
            .unwrap()
            .warnings()
            .len(),
        1
    );
}

#[test]
fn validates_encoder_options() {
    assert!(EncodeOptions::default().block_rows(0).is_err());
    assert!(EncodeOptions::default().compression_level(11).is_err());
}
