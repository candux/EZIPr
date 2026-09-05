#[cfg(feature = "smallest")]
use ezipr::CompressionStrategy;
use ezipr::{
    AlphaMode, AnimationEncoder, BlendMode, ColorDepth, Decoder, DisposalMethod, EncodeOptions,
    FrameView, ImageView, PixelFormat, Repeat, ResourceFormat, ResourceKind, Rgb565Dithering,
    StorageFormat,
};

fn solid_rgba(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    color.repeat((width * height) as usize)
}

fn add_frame(
    encoder: &mut AnimationEncoder,
    dimensions: (u32, u32),
    pixels: &[u8],
    offset: (u32, u32),
    delay: (u16, u16),
    disposal: DisposalMethod,
    blend: BlendMode,
) {
    let (width, height) = dimensions;
    let image = ImageView::new(
        width,
        height,
        PixelFormat::Rgba8,
        width as usize * 4,
        pixels,
    )
    .unwrap();
    encoder
        .push_frame(
            FrameView::new(image, offset.0, offset.1, delay.0, delay.1)
                .disposal(disposal)
                .blend(blend),
        )
        .unwrap();
}

fn encoded_animation() -> ezipr::EncodedResource {
    let options = EncodeOptions::new(ColorDepth::Rgb565)
        .alpha_mode(AlphaMode::Auto)
        .dithering(Rgb565Dithering::None);
    let mut encoder = AnimationEncoder::new(3, 3, Repeat::Finite(2), options).unwrap();
    add_frame(
        &mut encoder,
        (3, 3),
        &solid_rgba(3, 3, [255, 0, 0, 255]),
        (0, 0),
        (1, 10),
        DisposalMethod::None,
        BlendMode::Source,
    );
    add_frame(
        &mut encoder,
        (2, 2),
        &solid_rgba(2, 2, [0, 255, 0, 128]),
        (1, 1),
        (1, 4),
        DisposalMethod::Background,
        BlendMode::Over,
    );
    add_frame(
        &mut encoder,
        (1, 1),
        &solid_rgba(1, 1, [0, 0, 255, 255]),
        (2, 0),
        (1, 20),
        DisposalMethod::Previous,
        BlendMode::Source,
    );
    add_frame(
        &mut encoder,
        (1, 1),
        &solid_rgba(1, 1, [255, 255, 255, 255]),
        (0, 2),
        (0, 0),
        DisposalMethod::None,
        BlendMode::Source,
    );
    encoder.finish().unwrap()
}

#[test]
fn balanced_dithering_uses_animation_canvas_coordinates() {
    let options = EncodeOptions::new(ColorDepth::Rgb565).alpha_mode(AlphaMode::Preserve);
    let mut encoder = AnimationEncoder::new(8, 8, Repeat::Finite(1), options).unwrap();
    add_frame(
        &mut encoder,
        (1, 1),
        &[5, 3, 5, 37],
        (3, 1),
        (1, 10),
        DisposalMethod::None,
        BlendMode::Source,
    );

    let encoded = encoder.finish().unwrap();
    let decoder = Decoder::new(encoded.as_bytes()).unwrap();
    let frame = decoder.decode_frame(0, PixelFormat::Rgba8).unwrap();
    assert_eq!(frame.pixels(), &[8, 4, 8, 37]);
}

#[test]
fn writes_animation_container_and_frame_metadata() {
    let encoded = encoded_animation();
    assert_eq!(encoded.storage_format(), StorageFormat::Argb565);
    assert_eq!(encoded.resource_format(), ResourceFormat::EzipArgb565);

    let decoder = Decoder::new(encoded.as_bytes()).unwrap();
    assert_eq!(decoder.info().kind(), ResourceKind::Animation);
    assert_eq!((decoder.info().width(), decoder.info().height()), (3, 3));
    assert_eq!(decoder.info().frame_count(), 4);
    assert_eq!(decoder.repeat(), Some(Repeat::Finite(2)));

    let second = decoder.frame_info(1).unwrap();
    assert_eq!((second.width(), second.height()), (2, 2));
    assert_eq!((second.x_offset(), second.y_offset()), (1, 1));
    assert_eq!(
        (second.delay_numerator(), second.delay_denominator()),
        (1, 4)
    );
    assert_eq!(second.disposal(), DisposalMethod::Background);
    assert_eq!(second.blend(), BlendMode::Over);
}

#[test]
fn round_trip_compositor_applies_background_and_previous() {
    let encoded = encoded_animation();
    let decoder = Decoder::new(encoded.as_bytes()).unwrap();
    let mut compositor = decoder.compositor(PixelFormat::Rgba8).unwrap();

    let first = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&first.pixels()[0..4], &[255, 0, 0, 255]);

    let second = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&second.pixels()[0..4], &[255, 0, 0, 255]);
    assert_eq!(&second.pixels()[16..20], &[127, 128, 0, 255]);

    let third = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&third.pixels()[8..12], &[0, 0, 255, 255]);
    assert_eq!(&third.pixels()[16..20], &[0, 0, 0, 0]);

    let fourth = compositor.next_frame().unwrap().unwrap();
    assert_eq!(&fourth.pixels()[8..12], &[255, 0, 0, 255]);
    assert_eq!(&fourth.pixels()[16..20], &[0, 0, 0, 0]);
    assert_eq!(&fourth.pixels()[24..28], &[255, 255, 255, 255]);
}

#[test]
fn argb888_animation_uses_generic_outer_resource_format() {
    let options = EncodeOptions::new(ColorDepth::Rgb888).alpha_mode(AlphaMode::Preserve);
    let mut encoder = AnimationEncoder::new(1, 1, Repeat::Infinite, options).unwrap();
    add_frame(
        &mut encoder,
        (1, 1),
        &[1, 2, 3, 4],
        (0, 0),
        (1, 100),
        DisposalMethod::None,
        BlendMode::Source,
    );
    let encoded = encoder.finish().unwrap();
    assert_eq!(encoded.storage_format(), StorageFormat::Argb888);
    assert_eq!(encoded.resource_format(), ResourceFormat::Ezip);
    let decoder = Decoder::new(encoded.as_bytes()).unwrap();
    assert_eq!(decoder.repeat(), Some(Repeat::Infinite));
    assert_eq!(
        decoder
            .decode_frame(0, PixelFormat::Rgba8)
            .unwrap()
            .pixels(),
        &[1, 2, 3, 4]
    );
}

#[test]
fn validates_animation_builder_input() {
    let options = EncodeOptions::default();
    assert!(AnimationEncoder::new(3, 3, Repeat::Finite(0), options).is_err());

    let pixel_options = options.resource_encoding(ezipr::ResourceEncoding::Pixel);
    assert!(AnimationEncoder::new(3, 3, Repeat::Infinite, pixel_options).is_err());

    let encoder = AnimationEncoder::new(3, 3, Repeat::Infinite, options).unwrap();
    assert!(encoder.finish().is_err());

    let mut encoder = AnimationEncoder::new(3, 3, Repeat::Infinite, options).unwrap();
    let pixels = solid_rgba(2, 2, [0, 0, 0, 0]);
    let image = ImageView::new(2, 2, PixelFormat::Rgba8, 8, &pixels).unwrap();
    assert!(
        encoder
            .push_frame(FrameView::new(image, 2, 2, 0, 0))
            .is_err()
    );
}

#[test]
#[cfg(not(feature = "smallest"))]
fn smallest_animation_strategy_requires_its_cargo_feature() {
    let options =
        EncodeOptions::default().compression_strategy(ezipr::CompressionStrategy::Smallest);
    let error = AnimationEncoder::new(3, 3, Repeat::Infinite, options).unwrap_err();
    assert_eq!(error.kind(), ezipr::ErrorKind::InvalidInput);
    assert!(error.message().contains("Cargo feature"));
}

#[test]
fn animation_output_is_deterministic_for_the_locked_graph() {
    let first = encoded_animation();
    let second = encoded_animation();
    assert_eq!(first, second);
    assert_eq!(crc32fast::hash(first.as_bytes()), 0xb816_1c5d);
}

#[test]
#[cfg(feature = "smallest")]
fn smallest_strategy_optimizes_animation_frames_without_changing_pixels() {
    let pixels: Vec<_> = (0_u8..16)
        .flat_map(|y| {
            (0_u8..16).flat_map(move |x| [x.wrapping_mul(16), y.wrapping_mul(16), x ^ y, 255])
        })
        .collect();
    let image = ImageView::new(16, 16, PixelFormat::Rgba8, 16 * 4, &pixels).unwrap();
    let base_options = EncodeOptions::new(ColorDepth::Rgb888).alpha_mode(AlphaMode::Discard);

    let encode = |options| {
        let mut encoder = AnimationEncoder::new(16, 16, Repeat::Finite(1), options).unwrap();
        encoder
            .push_frame(FrameView::new(image, 0, 0, 1, 10))
            .unwrap();
        encoder.finish().unwrap()
    };
    let baseline = encode(base_options);
    let smallest_options = base_options.compression_strategy(CompressionStrategy::Smallest);
    let smallest = encode(smallest_options);
    let repeated = encode(smallest_options);

    assert!(smallest.as_bytes().len() <= baseline.as_bytes().len());
    assert_eq!(smallest, repeated);
    let decoded = Decoder::new(smallest.as_bytes())
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    let expected: Vec<_> = pixels
        .chunks_exact(4)
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect();
    assert_eq!(decoded.pixels(), expected);
}
