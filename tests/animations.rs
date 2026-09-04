use ezipr::{
    BlendMode, DecodeLimits, DecodeOptions, Decoder, DisposalMethod, ErrorKind, PixelFormat,
    Repeat, ResourceKind, StorageFormat,
};

const ANIMATION: &[u8] = include_bytes!("fixtures/animation/controlled.bin");
const ANIMATION_ARGB888: &[u8] = include_bytes!("fixtures/animation/controlled-argb888.bin");

#[test]
fn parses_owned_animation_metadata() {
    let decoder = Decoder::new(ANIMATION).unwrap();
    assert_eq!(decoder.info().kind(), ResourceKind::Animation);
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb565);
    assert_eq!((decoder.info().width(), decoder.info().height()), (8, 6));
    assert_eq!(decoder.info().frame_count(), 3);
    assert_eq!(decoder.repeat(), Some(Repeat::Finite(2)));

    let expected = [
        (
            0,
            8,
            6,
            0,
            0,
            1,
            10,
            DisposalMethod::None,
            BlendMode::Source,
        ),
        (
            1,
            8,
            6,
            0,
            0,
            1,
            4,
            DisposalMethod::Background,
            BlendMode::Over,
        ),
        (
            2,
            4,
            4,
            4,
            2,
            1,
            20,
            DisposalMethod::Previous,
            BlendMode::Over,
        ),
    ];
    for (index, expected) in expected.into_iter().enumerate() {
        let frame = decoder.frame_info(index).unwrap();
        assert_eq!(
            (
                frame.sequence(),
                frame.width(),
                frame.height(),
                frame.x_offset(),
                frame.y_offset(),
                frame.delay_numerator(),
                frame.delay_denominator(),
                frame.disposal(),
                frame.blend(),
            ),
            expected
        );
    }
}

#[test]
fn decodes_stored_frame_rectangles() {
    let decoder = Decoder::new(ANIMATION).unwrap();
    let first = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!((first.width(), first.height()), (8, 6));
    assert!(
        first
            .pixels()
            .chunks_exact(4)
            .all(|pixel| { pixel[0] == 255 && pixel[1] <= 4 && pixel[2] <= 8 && pixel[3] == 255 })
    );

    let second = decoder.decode_frame(1, PixelFormat::Rgba8).unwrap();
    assert_eq!(&second.pixels()[0..4], &[0, 0, 0, 0]);
    assert_eq!(
        &second.pixels()[(1 * 8 + 2) * 4..(1 * 8 + 2) * 4 + 4],
        &[0, 255, 0, 128]
    );

    let third = decoder.decode_frame(2, PixelFormat::Rgba8).unwrap();
    assert_eq!((third.width(), third.height()), (4, 4));
    assert!(
        third
            .pixels()
            .chunks_exact(4)
            .all(|pixel| { pixel[0] <= 8 && pixel[1] <= 4 && pixel[2] == 255 && pixel[3] == 255 })
    );
}

#[test]
fn decodes_owned_argb888_animation_exactly() {
    let decoder = Decoder::new(ANIMATION_ARGB888).unwrap();
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb888);
    assert_eq!(decoder.info().frame_count(), 3);

    let first = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert!(
        first
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel == [255, 0, 0, 255])
    );
    let second = decoder.decode_frame(1, PixelFormat::Rgba8).unwrap();
    assert_eq!(&second.pixels()[0..4], &[0, 0, 0, 0]);
    assert_eq!(
        &second.pixels()[(1 * 8 + 2) * 4..(1 * 8 + 2) * 4 + 4],
        &[0, 255, 0, 128]
    );
    let third = decoder.decode_frame(2, PixelFormat::Rgba8).unwrap();
    assert!(
        third
            .pixels()
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 255, 255])
    );
}

#[test]
fn sequential_compositor_applies_blend_and_disposal() {
    let decoder = Decoder::new(ANIMATION).unwrap();
    let mut compositor = decoder.compositor(PixelFormat::Rgba8).unwrap();

    let first = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&first.pixels()[0..4], &[255, 0, 0, 255]);

    let second = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&second.pixels()[0..4], &[255, 0, 0, 255]);
    assert_eq!(
        &second.pixels()[(1 * 8 + 2) * 4..(1 * 8 + 2) * 4 + 4],
        &[127, 128, 0, 255]
    );

    let third = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&third.pixels()[0..4], &[0, 0, 0, 0]);
    assert_eq!(
        &third.pixels()[(2 * 8 + 4) * 4..(2 * 8 + 4) * 4 + 4],
        &[0, 0, 255, 255]
    );
    assert!(compositor.next_frame().unwrap().is_none());

    compositor.reset();
    assert_eq!(compositor.next_index(), 0);
    assert!(compositor.next_frame().unwrap().is_some());
}

#[test]
fn random_composition_replays_to_requested_frame() {
    let decoder = Decoder::new(ANIMATION).unwrap();
    let random = decoder
        .decode_composited_frame(2, PixelFormat::Rgba8)
        .unwrap();
    let mut sequential = decoder.compositor(PixelFormat::Rgba8).unwrap();
    sequential.next_frame().unwrap();
    sequential.next_frame().unwrap();
    let expected = sequential.next_frame().unwrap().unwrap();
    assert_eq!(random, expected);
}

#[test]
fn validates_container_and_frame_checksums_with_context() {
    let mut container = ANIMATION.to_vec();
    *container.last_mut().unwrap() ^= 1;
    assert_eq!(
        Decoder::new(&container).unwrap_err().kind(),
        ErrorKind::ChecksumMismatch
    );

    let mut frame = ANIMATION.to_vec();
    frame[126] ^= 1;
    let declared = u32::from_be_bytes(frame[4..8].try_into().unwrap()) as usize;
    let crc = crc32fast::hash(&frame[4..4 + declared]);
    frame[4 + declared..4 + declared + 4].copy_from_slice(&crc.to_le_bytes());
    let error = Decoder::new(&frame).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::ChecksumMismatch);
    assert_eq!(error.frame_index(), Some(0));
}

#[test]
fn enforces_animation_frame_limit() {
    let options = DecodeOptions::new().limits(DecodeLimits::new().max_frames(2));
    assert_eq!(
        Decoder::with_options(ANIMATION, options)
            .unwrap_err()
            .kind(),
        ErrorKind::LimitExceeded
    );
}

#[test]
fn malformed_offsets_return_errors_without_panicking() {
    for offset in [0_u32, 37, u32::MAX] {
        let mut data = ANIMATION.to_vec();
        data[28..32].copy_from_slice(&offset.to_be_bytes());
        let declared = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
        let crc = crc32fast::hash(&data[4..4 + declared]);
        data[4 + declared..4 + declared + 4].copy_from_slice(&crc.to_le_bytes());
        assert!(Decoder::new(&data).is_err());
    }
}
