#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ezipr::{
    AnimationEncoder, Decoder, EncodeOptions, FrameView, ImageView, PixelFormat, Repeat,
    ResourceKind, StorageFormat,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ezipr-cli-test-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("create isolated test directory");
        Self(path)
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated test directory");
    }
}

fn ezipr(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ezipr"))
        .args(arguments)
        .output()
        .expect("run ezipr command")
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("command output is UTF-8")
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("test path is UTF-8")
}

#[test]
fn info_and_verify_cover_static_and_animation_resources() {
    let info = assert_success(ezipr(&["info", "tests/fixtures/animation/controlled.bin"]));
    assert!(info.contains("kind: eZIP-A"));
    assert!(info.contains("dimensions: 8x6"));
    assert!(info.contains("frames: 3"));
    assert!(info.contains("repeat: 2"));
    assert!(info.contains("frame 2: 4x4+4+2"));

    let verify = assert_success(ezipr(&["verify", "tests/fixtures/static/ezip-rgb565.bin"]));
    assert!(verify.contains("valid: static resource decoded"));
}

#[test]
fn static_png_decode_encode_round_trip_uses_requested_layout() {
    let directory = TestDirectory::new();
    let png = directory.join("decoded.png");
    let resource = directory.join("encoded.bin");

    assert_success(ezipr(&[
        "decode",
        "tests/fixtures/static/ezip-argb888.bin",
        path_text(&png),
    ]));
    assert_success(ezipr(&[
        "encode",
        path_text(&png),
        path_text(&resource),
        "--depth",
        "rgb888",
    ]));

    let bytes = fs::read(resource).expect("read encoded resource");
    let decoder = Decoder::new(&bytes).expect("decode CLI output");
    assert_eq!(decoder.info().kind(), ResourceKind::Ezip);
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb888);
    let image = decoder
        .decode_frame(0, PixelFormat::Rgba8)
        .expect("decode pixels");
    assert_eq!((image.width(), image.height()), (8, 4));
}

#[test]
fn rgb565_cli_defaults_to_balanced_dithering_and_exposes_other_modes() {
    let directory = TestDirectory::new();
    let png = directory.join("black.png");
    let default_resource = directory.join("default.bin");
    let balanced_resource = directory.join("balanced.bin");
    let reference_resource = directory.join("reference.bin");
    let direct_resource = directory.join("direct.bin");
    let file = fs::File::create(&png).expect("create PNG");
    let mut encoder = png::Encoder::new(file, 8, 8);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("write PNG header")
        .write_image_data(&[0; 8 * 8 * 3])
        .expect("write PNG pixels");

    assert_success(ezipr(&[
        "encode",
        path_text(&png),
        path_text(&default_resource),
        "--depth",
        "rgb565",
        "--pixel",
    ]));
    assert_success(ezipr(&[
        "encode",
        path_text(&png),
        path_text(&balanced_resource),
        "--depth",
        "rgb565",
        "--pixel",
        "--dither",
        "balanced",
    ]));
    assert_success(ezipr(&[
        "encode",
        path_text(&png),
        path_text(&reference_resource),
        "--depth",
        "rgb565",
        "--pixel",
        "--dither",
        "reference",
    ]));
    assert_success(ezipr(&[
        "encode",
        path_text(&png),
        path_text(&direct_resource),
        "--depth",
        "rgb565",
        "--pixel",
        "--dither",
        "none",
    ]));

    let default = fs::read(default_resource).expect("read default resource");
    let balanced = fs::read(balanced_resource).expect("read balanced resource");
    assert_eq!(default, balanced);
    let balanced = Decoder::new(&balanced)
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert!(balanced.pixels().iter().all(|&channel| channel == 0));

    let reference = fs::read(reference_resource).expect("read reference resource");
    let reference = Decoder::new(&reference)
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert!(reference.pixels().iter().any(|&channel| channel != 0));

    let direct = fs::read(direct_resource).expect("read direct resource");
    let direct = Decoder::new(&direct)
        .unwrap()
        .decode_frame(0, PixelFormat::Rgb8)
        .unwrap();
    assert!(direct.pixels().iter().all(|&channel| channel == 0));
}

#[test]
fn animation_apng_round_trip_preserves_control_metadata() {
    let directory = TestDirectory::new();
    let apng = directory.join("decoded.apng");
    let resource = directory.join("encoded.bin");

    assert_success(ezipr(&[
        "decode",
        "tests/fixtures/animation/controlled.bin",
        path_text(&apng),
    ]));
    assert_success(ezipr(&[
        "encode",
        path_text(&apng),
        path_text(&resource),
        "--depth",
        "rgb888",
    ]));

    let bytes = fs::read(resource).expect("read encoded animation");
    let decoder = Decoder::new(&bytes).expect("decode CLI animation");
    assert_eq!(decoder.info().kind(), ResourceKind::Animation);
    assert_eq!(decoder.info().storage_format(), StorageFormat::Argb888);
    assert_eq!(decoder.info().frame_count(), 3);
    assert_eq!(decoder.repeat(), Some(Repeat::Finite(2)));
    let last = decoder.frame_info(2).expect("last frame metadata");
    assert_eq!((last.width(), last.height()), (4, 4));
    assert_eq!((last.x_offset(), last.y_offset()), (4, 2));
    assert_eq!((last.delay_numerator(), last.delay_denominator()), (1, 20));
}

#[test]
fn manifest_encodes_explicit_frame_rectangles() {
    let directory = TestDirectory::new();
    let manifest = directory.join("animation.toml");
    let resource = directory.join("animation.bin");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/static/source-rgba.png");
    fs::write(
        &manifest,
        format!(
            r#"width = 8
height = 4
repeat = 0
depth = "rgb888"
alpha = "preserve"
dither = "reference"

[[frames]]
file = {:?}
delay_numerator = 1
delay_denominator = 10

[[frames]]
file = {:?}
delay_numerator = 3
delay_denominator = 20
disposal = "background"
blend = "over"
"#,
            source, source
        ),
    )
    .expect("write manifest");

    assert_success(ezipr(&[
        "encode",
        path_text(&manifest),
        path_text(&resource),
    ]));

    let bytes = fs::read(resource).expect("read encoded manifest");
    let decoder = Decoder::new(&bytes).expect("decode manifest output");
    assert_eq!(decoder.info().frame_count(), 2);
    assert_eq!(decoder.repeat(), Some(Repeat::Infinite));
    let second = decoder.frame_info(1).expect("second frame metadata");
    assert_eq!(
        (second.delay_numerator(), second.delay_denominator()),
        (3, 20)
    );
}

#[test]
fn malformed_input_fails_without_creating_output() {
    let directory = TestDirectory::new();
    let malformed = directory.join("malformed.bin");
    let output = directory.join("output.png");
    fs::write(&malformed, [1, 2, 3]).expect("write malformed input");
    let result = ezipr(&["decode", path_text(&malformed), path_text(&output)]);
    assert!(!result.status.success());
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("error:"));
}

#[test]
fn one_frame_apng_remains_an_animation() {
    let directory = TestDirectory::new();
    let apng = directory.join("one-frame.apng");
    let resource = directory.join("one-frame.bin");
    let file = fs::File::create(&apng).expect("create APNG");
    let mut encoder = png::Encoder::new(file, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(1, 0).expect("enable animation");
    let mut writer = encoder.write_header().expect("write APNG header");
    writer.set_frame_delay(1, 10).expect("set delay");
    writer
        .write_image_data(&[255, 0, 0, 255].repeat(4))
        .expect("write APNG frame");
    drop(writer);

    assert_success(ezipr(&["encode", path_text(&apng), path_text(&resource)]));
    let bytes = fs::read(resource).expect("read encoded animation");
    let decoder = Decoder::new(&bytes).expect("decode one-frame animation");
    assert_eq!(decoder.info().kind(), ResourceKind::Animation);
    assert_eq!(decoder.info().frame_count(), 1);
    assert_eq!(decoder.repeat(), Some(Repeat::Infinite));
}

#[test]
fn apng_output_allows_a_frame_to_grow_after_a_shifted_frame() {
    let directory = TestDirectory::new();
    let resource = directory.join("growing.bin");
    let apng = directory.join("growing.apng");
    let round_trip = directory.join("growing-round-trip.bin");
    let small_pixels = [0, 255, 0, 255].repeat(4);
    let large_pixels = [0, 0, 255, 255].repeat(16);
    let small = ImageView::new(2, 2, PixelFormat::Rgba8, 8, &small_pixels).unwrap();
    let large = ImageView::new(4, 4, PixelFormat::Rgba8, 16, &large_pixels).unwrap();
    let mut encoder =
        AnimationEncoder::new(4, 4, Repeat::Finite(1), EncodeOptions::default()).unwrap();
    encoder
        .push_frame(FrameView::new(small, 2, 2, 1, 10))
        .unwrap();
    encoder
        .push_frame(FrameView::new(large, 0, 0, 1, 5))
        .unwrap();
    fs::write(&resource, encoder.finish().unwrap().as_bytes()).expect("write animation");

    assert_success(ezipr(&["decode", path_text(&resource), path_text(&apng)]));
    assert_success(ezipr(&["encode", path_text(&apng), path_text(&round_trip)]));
    let bytes = fs::read(round_trip).expect("read round-trip animation");
    let decoder = Decoder::new(&bytes).expect("decode round-trip animation");
    assert_eq!(decoder.info().frame_count(), 2);
    assert_eq!(
        (
            decoder.frame_info(0).unwrap().x_offset(),
            decoder.frame_info(0).unwrap().y_offset()
        ),
        (2, 2)
    );
    assert_eq!(
        (
            decoder.frame_info(1).unwrap().width(),
            decoder.frame_info(1).unwrap().height()
        ),
        (4, 4)
    );
}
