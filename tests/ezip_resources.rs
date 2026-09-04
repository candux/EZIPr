use ezipr::{
    DecodeMode, DecodeOptions, Decoder, ErrorKind, PixelFormat, ResourceKind, StorageFormat,
    StreamHeader, WarningKind,
};

fn source_colors() -> [[u8; 3]; 8] {
    [
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 255],
        [0, 0, 0],
        [17, 33, 65],
        [123, 45, 210],
        [250, 128, 7],
    ]
}

fn expected_rgb() -> Vec<u8> {
    let colors = source_colors();
    let mut expected = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            expected.extend_from_slice(&colors[(x + y * 2) % colors.len()]);
        }
    }
    expected
}

fn expected_rgba() -> Vec<u8> {
    let colors = source_colors();
    let alphas = [0, 1, 31, 64, 127, 128, 200, 255];
    let mut expected = Vec::new();
    for y in 0..4 {
        for x in 0..8 {
            expected.extend_from_slice(&colors[(x + y * 2) % colors.len()]);
            expected.push(alphas[(x + y * 3) % alphas.len()]);
        }
    }
    expected
}

fn expected_multiblock_rgb() -> Vec<u8> {
    let mut expected = Vec::with_capacity(8 * 70 * 3);
    for y in 0..70 {
        for x in 0..8 {
            expected.extend_from_slice(&[
                (x * 37 + y * 11 + (y / 32) * 53) as u8,
                (x * 17 + y * 29) as u8,
                (x * 97 + y * 7) as u8,
            ]);
        }
    }
    expected
}

#[test]
fn parses_shared_huffman_stream_header() {
    let data = include_bytes!("fixtures/static/ezip-rgb565.bin");
    let header = StreamHeader::parse(&data[4..]).unwrap();
    assert_eq!(header.data_size(), 114);
    assert_eq!(header.control(), 0x48);
    assert_eq!(header.bit_depth(), 16);
    assert_eq!(header.block_rows(), 32);
    assert_eq!((header.width(), header.height()), (8, 4));
    assert_eq!(header.filter_mode(), 0);
    assert!(header.has_row_filters());
    assert!(header.uses_shared_huffman());
}

#[test]
fn decodes_all_owned_shared_huffman_fixtures() {
    let cases = [
        (
            include_bytes!("fixtures/static/ezip-rgb565.bin").as_slice(),
            StorageFormat::Rgb565,
        ),
        (
            include_bytes!("fixtures/static/ezip-rgb888.bin").as_slice(),
            StorageFormat::Rgb888,
        ),
        (
            include_bytes!("fixtures/static/ezip-argb565.bin").as_slice(),
            StorageFormat::Argb565,
        ),
        (
            include_bytes!("fixtures/static/ezip-argb888.bin").as_slice(),
            StorageFormat::Argb888,
        ),
    ];
    for (data, storage) in cases {
        let decoder = Decoder::new(data).unwrap();
        assert_eq!(decoder.info().kind(), ResourceKind::Ezip);
        assert_eq!(decoder.info().storage_format(), storage);
        assert_eq!((decoder.info().width(), decoder.info().height()), (8, 4));
        assert!(decoder.warnings().is_empty());
    }
}

#[test]
fn decoded_888_pixels_match_owned_sources_exactly() {
    let rgb = Decoder::new(include_bytes!("fixtures/static/ezip-rgb888.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert_eq!(rgb.pixels(), expected_rgb());

    let rgba = Decoder::new(include_bytes!("fixtures/static/ezip-argb888.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgba8)
        .unwrap();
    assert_eq!(rgba.pixels(), expected_rgba());
}

#[test]
fn decoded_565_colors_are_consistent_and_alpha_is_exact() {
    let opaque = Decoder::new(include_bytes!("fixtures/static/ezip-rgb565.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    let alpha = Decoder::new(include_bytes!("fixtures/static/ezip-argb565.bin"))
        .unwrap()
        .decode_frame(0, PixelFormat::Rgba8)
        .unwrap();
    let alpha_rgb: Vec<_> = alpha
        .pixels()
        .chunks_exact(4)
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect();
    let alpha_values: Vec<_> = alpha
        .pixels()
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect();
    let expected_alpha: Vec<_> = expected_rgba()
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect();
    assert_eq!(opaque.pixels(), alpha_rgb);
    assert_eq!(alpha_values, expected_alpha);
}

#[test]
fn validates_shared_huffman_crc32() {
    let mut data = include_bytes!("fixtures/static/ezip-rgb565.bin").to_vec();
    *data.last_mut().unwrap() ^= 1;
    assert_eq!(
        Decoder::new(&data).unwrap_err().kind(),
        ErrorKind::ChecksumMismatch
    );

    let options = DecodeOptions::new().mode(DecodeMode::Diagnostic);
    let decoder = Decoder::with_options(&data, options).unwrap();
    assert_eq!(decoder.warnings().len(), 1);
    assert_eq!(decoder.warnings()[0].kind(), WarningKind::ChecksumMismatch);
}

#[test]
fn decodes_shared_huffman_across_multiple_row_blocks() {
    let data = include_bytes!("fixtures/static/ezip-rgb888-multiblock.bin");
    let stream = StreamHeader::parse(&data[4..]).unwrap();
    assert!(stream.uses_shared_huffman());
    assert_eq!(stream.block_rows(), 32);
    assert_eq!((stream.width(), stream.height()), (8, 70));

    let decoder = Decoder::new(data).unwrap();
    let image = decoder.decode_frame(0, PixelFormat::Rgb8).unwrap();
    assert_eq!((image.width(), image.height()), (8, 70));
    assert_eq!(image.pixels(), expected_multiblock_rgb());
}

#[test]
fn validates_shared_huffman_block_offsets() {
    let mut data = include_bytes!("fixtures/static/ezip-rgb888-multiblock.bin").to_vec();
    data[24..28].copy_from_slice(&0_u32.to_be_bytes());
    let declared = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let crc = crc32fast::hash(&data[4..4 + declared]);
    data[4 + declared..8 + declared].copy_from_slice(&crc.to_le_bytes());

    assert_eq!(
        Decoder::new(&data).unwrap_err().kind(),
        ErrorKind::InvalidOffset
    );
}

#[test]
fn reports_palette_resources_as_unsupported() {
    let mut data = include_bytes!("fixtures/static/ezip-rgb565.bin").to_vec();
    data[17] = 1;
    assert_eq!(
        Decoder::new(&data).unwrap_err().kind(),
        ErrorKind::UnsupportedFormat
    );
}
