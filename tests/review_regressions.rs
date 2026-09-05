use ezipr::*;

fn one_pixel() -> ImageView<'static> {
    ImageView::new(1, 1, PixelFormat::Rgb8, 3, &[82, 80, 82]).unwrap()
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
