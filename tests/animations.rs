use ezipr::{
    BlendMode, DecodeLimits, DecodeMode, DecodeOptions, Decoder, DisposalMethod, ErrorKind,
    PixelFormat, Repeat, ResourceFormat, ResourceHeader, ResourceKind, StorageFormat, WarningKind,
};

const ANIMATION: &[u8] = include_bytes!("fixtures/animation/controlled.bin");
const ANIMATION_ARGB888: &[u8] = include_bytes!("fixtures/animation/controlled-argb888.bin");
const ANIMATION_RGB565: &[u8] = include_bytes!("fixtures/animation/opaque-rgb565.bin");
const ANIMATION_RGB888: &[u8] = include_bytes!("fixtures/animation/opaque-rgb888.bin");

fn opaque_source_color(frame: usize, x: usize, y: usize) -> [u8; 3] {
    match frame {
        0 => [
            (x * 31 + y * 7) as u8,
            (x * 9 + y * 23) as u8,
            (x * 3 + y * 5) as u8,
        ],
        1 => [
            (x * 5 + y * 13) as u8,
            (200 + x * 4 - y * 7) as u8,
            (x * 29 + y * 11) as u8,
        ],
        2 => [
            (220 - x * 17 + y * 3) as u8,
            (x * 15 + y * 19) as u8,
            (180 + x * 7 - y * 9) as u8,
        ],
        _ => unreachable!(),
    }
}

fn opaque_frame(frame: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(8 * 6 * 3);
    for y in 0..6 {
        for x in 0..8 {
            pixels.extend_from_slice(&opaque_source_color(frame, x, y));
        }
    }
    pixels
}

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
fn decodes_owned_opaque_animation_layouts() {
    for (data, storage, rgb565) in [
        (ANIMATION_RGB565, StorageFormat::Rgb565, true),
        (ANIMATION_RGB888, StorageFormat::Rgb888, false),
    ] {
        let decoder = Decoder::new(data).unwrap();
        assert_eq!(decoder.info().kind(), ResourceKind::Animation);
        assert_eq!(decoder.info().storage_format(), storage);
        assert_eq!(decoder.info().frame_count(), 3);
        assert_eq!(decoder.repeat(), Some(Repeat::Finite(2)));
        for index in 0..3 {
            let image = decoder.decode_frame(index, PixelFormat::Rgb8).unwrap();
            let expected = opaque_frame(index);
            if rgb565 {
                for (actual, source) in image.pixels().chunks_exact(3).zip(expected.chunks_exact(3))
                {
                    assert!(actual[0].abs_diff(source[0]) <= 13);
                    assert!(actual[1].abs_diff(source[1]) <= 5);
                    assert!(actual[2].abs_diff(source[2]) <= 13);
                }
            } else {
                assert_eq!(image.pixels(), expected);
            }
        }
    }
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

#[test]
fn validates_zero_alignment_padding_between_frames() {
    let mut data = ANIMATION.to_vec();
    data[130] = 1;
    let declared = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let crc = crc32fast::hash(&data[4..4 + declared]);
    data[4 + declared..4 + declared + 4].copy_from_slice(&crc.to_le_bytes());

    let error = Decoder::new(&data).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidAnimation);
    assert_eq!(error.frame_index(), Some(0));
}

#[test]
fn composes_frames_into_reusable_caller_buffers() {
    let decoder = Decoder::new(ANIMATION).unwrap();
    let mut compositor = decoder.compositor(PixelFormat::Rgba8).unwrap();
    assert_eq!(compositor.output_format(), PixelFormat::Rgba8);
    assert_eq!(compositor.output_buffer_size().unwrap(), 8 * 6 * 4);

    let mut too_small = vec![0; 8 * 6 * 4 - 1];
    let error = compositor.next_frame_into(&mut too_small).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::OutputBufferTooSmall);
    assert_eq!(compositor.next_index(), 0);

    let mut destination = vec![0; 8 * 6 * 4];
    assert_eq!(
        compositor.next_frame_into(&mut destination).unwrap(),
        Some(8 * 6 * 4)
    );
    assert_eq!(&destination[..4], &[255, 0, 0, 255]);
    assert_eq!(compositor.next_index(), 1);

    let expected = decoder
        .decode_composited_frame(2, PixelFormat::Rgb8)
        .unwrap();
    let mut random = vec![0; 8 * 6 * 3];
    assert_eq!(
        decoder
            .decode_composited_frame_into(2, PixelFormat::Rgb8, &mut random)
            .unwrap(),
        random.len()
    );
    assert_eq!(random, expected.pixels());
}

#[test]
fn diagnostic_compositor_clips_frames_to_the_resource_canvas() {
    let mut data = ANIMATION.to_vec();
    data[..4].copy_from_slice(
        &ResourceHeader::new(ResourceFormat::EzipArgb565, 7, 5)
            .unwrap()
            .to_bytes(),
    );
    let options = DecodeOptions::new().mode(DecodeMode::Diagnostic);
    let decoder = Decoder::with_options(&data, options).unwrap();
    assert_eq!((decoder.info().width(), decoder.info().height()), (7, 5));
    assert_eq!(decoder.warnings().len(), 1);
    assert_eq!(decoder.warnings()[0].kind(), WarningKind::MetadataMismatch);

    let mut compositor = decoder.compositor(PixelFormat::Rgba8).unwrap();
    for _ in 0..3 {
        let image = compositor.next_frame().unwrap().unwrap();
        assert_eq!((image.width(), image.height()), (7, 5));
        assert_eq!(image.pixels().len(), 7 * 5 * 4);
    }
    assert!(compositor.next_frame().unwrap().is_none());
}
