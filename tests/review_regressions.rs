use ezipr::*;

fn one_pixel() -> ImageView<'static> {
    ImageView::new(1, 1, PixelFormat::Rgb8, 3, &[82, 80, 82]).unwrap()
}

#[test]
fn unknown_representations_require_explicit_diagnostic_recovery() {
    let encoded = Encoder::default().encode(one_pixel()).unwrap();
    for representation in [0x00, 0x20, 0x30, 0x60, 0x80, 0xf0] {
        let mut bytes = encoded.as_bytes().to_vec();
        bytes[8] = representation | (bytes[8] & 15);
        assert_eq!(
            Decoder::new(&bytes).unwrap_err().kind(),
            ErrorKind::UnsupportedFormat
        );
        let decoder =
            Decoder::with_options(&bytes, DecodeOptions::new().mode(DecodeMode::Diagnostic))
                .unwrap();
        assert!(
            decoder
                .warnings()
                .iter()
                .any(|warning| warning.kind() == WarningKind::MetadataMismatch
                    && warning.message().contains("attempting standard"))
        );
        assert_eq!(
            decoder.decode_frame(0, PixelFormat::Rgb8).unwrap().pixels(),
            &[82, 80, 82]
        );
    }
}

#[test]
fn four_pixel_layout_ambiguity_is_reported_and_valid_crc_wins() {
    let source = [
        255, 0, 0, 37, 0, 255, 0, 81, 0, 0, 255, 129, 255, 255, 255, 255,
    ];
    let image = ImageView::new(4, 1, PixelFormat::Rgba8, 16, &source).unwrap();
    let diagnostic = DecodeOptions::new().mode(DecodeMode::Diagnostic);
    for alpha in [AlphaMode::Discard, AlphaMode::Preserve] {
        let options = EncodeOptions::new(ColorDepth::Rgb888)
            .alpha_mode(alpha)
            .resource_encoding(ResourceEncoding::Pixel);
        let encoded = Encoder::new(options).encode(image).unwrap();
        let expected = Decoder::new(encoded.as_bytes())
            .unwrap()
            .decode_frame(0, PixelFormat::Rgba8)
            .unwrap();
        let mut bytes = encoded.as_bytes().to_vec();
        bytes.truncate(bytes.len() - 4);
        assert!(Decoder::new(&bytes).is_err());
        let decoder = Decoder::with_options(&bytes, diagnostic).unwrap();
        assert_eq!(decoder.info().storage_format(), encoded.storage_format());
        assert_eq!(
            decoder
                .decode_frame(0, PixelFormat::Rgba8)
                .unwrap()
                .pixels(),
            expected.pixels()
        );
        assert!(
            decoder
                .warnings()
                .iter()
                .any(|warning| warning.kind() == WarningKind::MetadataMismatch
                    && warning.message().contains("ambiguous"))
        );

        let packed = Encoder::new(
            EncodeOptions::new(ColorDepth::Rgb565)
                .alpha_mode(alpha)
                .resource_encoding(ResourceEncoding::Pixel),
        )
        .encode(image)
        .unwrap();
        let decoder = Decoder::with_options(packed.as_bytes(), diagnostic).unwrap();
        assert_eq!(decoder.info().storage_format(), packed.storage_format());
        assert!(decoder.warnings().is_empty());
    }
}

#[test]
fn diagnostic_animation_padding_respects_cumulative_limit() {
    let mut encoder =
        AnimationEncoder::new(8, 6, Repeat::Infinite, EncodeOptions::default()).unwrap();
    encoder
        .push_frame(FrameView::new(one_pixel(), 0, 0, 1, 10))
        .unwrap();
    let mut data = encoder.finish().unwrap().as_bytes().to_vec();
    data.truncate(data.len() - 4);
    // One-frame table points to inner offset 28; frame dimensions are u32 BE.
    data[36..40].copy_from_slice(&8_u32.to_be_bytes());
    data[40..44].copy_from_slice(&6_u32.to_be_bytes());
    let options = DecodeOptions::new()
        .mode(DecodeMode::Diagnostic)
        .limits(DecodeLimits::new().max_decoded_bytes(64));
    let error = Decoder::with_options(&data, options).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
    assert_eq!(error.frame_index(), Some(0));
}

#[test]
fn animation_unfilter_errors_have_frame_context() {
    let mut encoder =
        AnimationEncoder::new(1, 1, Repeat::Infinite, EncodeOptions::default()).unwrap();
    encoder
        .push_frame(FrameView::new(one_pixel(), 0, 0, 1, 10))
        .unwrap();
    let mut data = encoder.finish().unwrap().as_bytes().to_vec();
    data.truncate(data.len() - 4);
    data[10] = 0;
    let error = Decoder::new(&data).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidHeader);
    assert_eq!(error.frame_index(), Some(0));
}

#[test]
fn disposal_outside_diagnostic_canvas_is_empty() {
    let mut encoder =
        AnimationEncoder::new(8, 6, Repeat::Infinite, EncodeOptions::default()).unwrap();
    encoder
        .push_frame(FrameView::new(one_pixel(), 7, 5, 1, 10).disposal(DisposalMethod::Background))
        .unwrap();
    encoder
        .push_frame(FrameView::new(one_pixel(), 0, 0, 1, 10))
        .unwrap();
    let mut data = encoder.finish().unwrap().as_bytes().to_vec();
    data[..4].copy_from_slice(
        &ResourceHeader::new(ResourceFormat::Ezip, 1, 6)
            .unwrap()
            .to_bytes(),
    );
    let decoder =
        Decoder::with_options(&data, DecodeOptions::new().mode(DecodeMode::Diagnostic)).unwrap();
    let mut compositor = decoder.compositor(PixelFormat::Rgba8).unwrap();
    assert!(compositor.next_frame().unwrap().is_some());
    assert!(compositor.next_frame().unwrap().is_some());
    assert!(compositor.next_frame().unwrap().is_none());
}

#[test]
fn diagnostic_padding_cannot_bypass_storage_limit() {
    let mut data = Encoder::default()
        .encode(one_pixel())
        .unwrap()
        .as_bytes()
        .to_vec();
    data[..4].copy_from_slice(
        &ResourceHeader::new(ResourceFormat::Ezip, 2047, 2047)
            .unwrap()
            .to_bytes(),
    );
    let options = DecodeOptions::new()
        .mode(DecodeMode::Diagnostic)
        .limits(DecodeLimits::new().max_decoded_bytes(64));
    assert_eq!(
        Decoder::with_options(&data, options).unwrap_err().kind(),
        ErrorKind::LimitExceeded
    );
}

#[test]
fn rgba_and_canvas_allocations_respect_output_limit() {
    let data = Encoder::new(EncodeOptions::default().row_filters(false))
        .encode(one_pixel())
        .unwrap();
    let decoder = Decoder::with_options(
        data.as_bytes(),
        DecodeOptions::new().limits(DecodeLimits::new().max_decoded_bytes(2)),
    )
    .unwrap();
    assert_eq!(
        decoder
            .decode_frame(0, PixelFormat::Rgba8)
            .unwrap_err()
            .kind(),
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        decoder.compositor(PixelFormat::Rgba8).unwrap_err().kind(),
        ErrorKind::LimitExceeded
    );
}
