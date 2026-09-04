use ezipr::{
    AlphaMode, AnimationEncoder, BlendMode, ColorDepth, Decoder, DisposalMethod, EncodeOptions,
    FrameView, ImageView, PixelFormat, Repeat, ResourceFormat, ResourceKind, StorageFormat,
};

fn solid_rgba(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    color.repeat((width * height) as usize)
}

fn add_frame(
    encoder: &mut AnimationEncoder,
    width: u32,
    height: u32,
    pixels: &[u8],
    x: u32,
    y: u32,
    delay: (u16, u16),
    disposal: DisposalMethod,
    blend: BlendMode,
) {
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
            FrameView::new(image, x, y, delay.0, delay.1)
                .disposal(disposal)
                .blend(blend),
        )
        .unwrap();
}

fn encoded_animation() -> ezipr::EncodedResource {
    let options = EncodeOptions::new(ColorDepth::Rgb565).alpha_mode(AlphaMode::Auto);
    let mut encoder = AnimationEncoder::new(3, 3, Repeat::Finite(2), options).unwrap();
    add_frame(
        &mut encoder,
        3,
        3,
        &solid_rgba(3, 3, [255, 0, 0, 255]),
        0,
        0,
        (1, 10),
        DisposalMethod::None,
        BlendMode::Source,
    );
    add_frame(
        &mut encoder,
        2,
        2,
        &solid_rgba(2, 2, [0, 255, 0, 128]),
        1,
        1,
        (1, 4),
        DisposalMethod::Background,
        BlendMode::Over,
    );
    add_frame(
        &mut encoder,
        1,
        1,
        &solid_rgba(1, 1, [0, 0, 255, 255]),
        2,
        0,
        (1, 20),
        DisposalMethod::Previous,
        BlendMode::Source,
    );
    add_frame(
        &mut encoder,
        1,
        1,
        &solid_rgba(1, 1, [255, 255, 255, 255]),
        0,
        2,
        (0, 0),
        DisposalMethod::None,
        BlendMode::Source,
    );
    encoder.finish().unwrap()
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
        1,
        1,
        &[1, 2, 3, 4],
        0,
        0,
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
fn animation_output_is_deterministic_for_the_locked_graph() {
    let first = encoded_animation();
    let second = encoded_animation();
    assert_eq!(first, second);
    assert_eq!(crc32fast::hash(first.as_bytes()), 0xb816_1c5d);
}
