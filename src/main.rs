#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use ezipr::{
    AlphaMode, AnimationEncoder, BlendMode, ColorDepth, DecodeMode, DecodeOptions, Decoder,
    DisposalMethod, EncodeOptions, Encoder, FrameView, ImageView, PixelFormat, Repeat,
    ResourceEncoding, ResourceKind,
};
use serde::Deserialize;

type CliResult<T> = Result<T, Box<dyn StdError>>;

#[derive(Debug, Parser)]
#[command(about = "Inspect, verify, decode, and encode eZIP image resources")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print resource metadata without writing output.
    Info {
        input: PathBuf,
        /// Recover from supported inconsistencies and report warnings.
        #[arg(long)]
        diagnostic: bool,
    },
    /// Strictly validate a resource and decode every frame.
    Verify { input: PathBuf },
    /// Decode a resource to PNG/APNG or a directory of composited PNG frames.
    Decode(DecodeArgs),
    /// Encode PNG, APNG, GIF, or a TOML frame manifest.
    Encode(EncodeArgs),
}

#[derive(Debug, Args)]
struct DecodeArgs {
    input: PathBuf,
    /// PNG or APNG output path. Omit when using --frames.
    output: Option<PathBuf>,
    /// Export full composited animation frames to this directory.
    #[arg(long, value_name = "DIRECTORY", conflicts_with = "output")]
    frames: Option<PathBuf>,
    /// Recover from supported inconsistencies and report warnings.
    #[arg(long)]
    diagnostic: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DepthArg {
    Rgb565,
    Rgb888,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AlphaArg {
    Auto,
    Preserve,
    Discard,
}

#[derive(Debug, Args)]
struct EncodeArgs {
    input: PathBuf,
    output: PathBuf,
    /// Stored color precision. A manifest value is used when omitted.
    #[arg(long, value_enum)]
    depth: Option<DepthArg>,
    /// Alpha-channel policy. A manifest value is used when omitted.
    #[arg(long, value_enum)]
    alpha: Option<AlphaArg>,
    /// Write an uncompressed PIXEL resource. Static images only.
    #[arg(long)]
    pixel: bool,
    /// Rows in each independently filtered block.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
    block_rows: Option<u8>,
    /// Disable adaptive row filters.
    #[arg(long)]
    no_filters: bool,
    /// DEFLATE compression level.
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=10))]
    compression: Option<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ManifestDepth {
    Rgb565,
    Rgb888,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ManifestAlpha {
    Auto,
    Preserve,
    Discard,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ManifestDisposal {
    None,
    Background,
    Previous,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ManifestBlend {
    Source,
    Over,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnimationManifest {
    width: u32,
    height: u32,
    /// Zero means infinite; positive values are finite play counts.
    repeat: u32,
    depth: Option<ManifestDepth>,
    alpha: Option<ManifestAlpha>,
    block_rows: Option<u8>,
    filters: Option<bool>,
    compression: Option<u8>,
    frames: Vec<ManifestFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFrame {
    file: PathBuf,
    #[serde(default)]
    x: u32,
    #[serde(default)]
    y: u32,
    delay_numerator: u16,
    delay_denominator: u16,
    #[serde(default = "default_disposal")]
    disposal: ManifestDisposal,
    #[serde(default = "default_blend")]
    blend: ManifestBlend,
}

fn default_disposal() -> ManifestDisposal {
    ManifestDisposal::None
}

fn default_blend() -> ManifestBlend {
    ManifestBlend::Source
}

#[derive(Debug)]
struct LoadedFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    format: PixelFormat,
    x: u32,
    y: u32,
    delay_numerator: u16,
    delay_denominator: u16,
    disposal: DisposalMethod,
    blend: BlendMode,
}

impl LoadedFrame {
    fn image(&self) -> ezipr::Result<ImageView<'_>> {
        ImageView::new(
            self.width,
            self.height,
            self.format,
            self.width as usize * self.format.bytes_per_pixel(),
            &self.pixels,
        )
    }
}

#[derive(Debug)]
struct LoadedAnimation {
    width: u32,
    height: u32,
    repeat: Repeat,
    animated: bool,
    frames: Vec<LoadedFrame>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        let mut source = error.source();
        while let Some(cause) = source {
            eprintln!("  caused by: {cause}");
            source = cause.source();
        }
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    match Cli::parse().command {
        Command::Info { input, diagnostic } => info(&input, diagnostic),
        Command::Verify { input } => verify(&input),
        Command::Decode(args) => decode(args),
        Command::Encode(args) => encode(args),
    }
}

fn parse_decoder(bytes: &[u8], diagnostic: bool) -> ezipr::Result<Decoder<'_>> {
    let mode = if diagnostic {
        DecodeMode::Diagnostic
    } else {
        DecodeMode::Strict
    };
    Decoder::with_options(bytes, DecodeOptions::new().mode(mode))
}

fn info(path: &Path, diagnostic: bool) -> CliResult<()> {
    let bytes = fs::read(path)?;
    let decoder = parse_decoder(&bytes, diagnostic)?;
    let info = decoder.info();
    println!("kind: {}", kind_name(info.kind()));
    println!("resource format: {:?}", info.resource_format());
    println!("storage format: {:?}", info.storage_format());
    println!("dimensions: {}x{}", info.width(), info.height());
    println!("frames: {}", info.frame_count());
    if let Some(repeat) = decoder.repeat() {
        println!("repeat: {}", repeat_name(repeat));
    }
    for index in 0..info.frame_count() {
        let frame = decoder.frame_info(index)?;
        if info.kind() == ResourceKind::Animation {
            println!(
                "frame {index}: {}x{}+{}+{}, delay {}/{}, disposal {:?}, blend {:?}",
                frame.width(),
                frame.height(),
                frame.x_offset(),
                frame.y_offset(),
                frame.delay_numerator(),
                frame.effective_delay_denominator(),
                frame.disposal(),
                frame.blend()
            );
        }
    }
    print_warnings(&decoder);
    Ok(())
}

fn verify(path: &Path) -> CliResult<()> {
    let bytes = fs::read(path)?;
    let decoder = parse_decoder(&bytes, false)?;
    let mut compositor = decoder.compositor(PixelFormat::Rgba8)?;
    let mut count = 0;
    while compositor.next_frame()?.is_some() {
        count += 1;
    }
    if decoder.info().kind() == ResourceKind::Animation {
        println!("valid: {count} animation frames decoded");
    } else {
        println!("valid: static resource decoded");
    }
    Ok(())
}

fn decode(args: DecodeArgs) -> CliResult<()> {
    let output = match (&args.output, &args.frames) {
        (Some(output), None) => Some(output),
        (None, Some(_)) => None,
        (None, None) => return Err(input_error("an output path or --frames is required")),
        (Some(_), Some(_)) => unreachable!("clap enforces the conflict"),
    };
    let bytes = fs::read(&args.input)?;
    let decoder = parse_decoder(&bytes, args.diagnostic)?;
    match decoder.info().kind() {
        ResourceKind::Animation => {
            if let Some(directory) = args.frames {
                write_composited_frames(&decoder, &directory)?;
            } else {
                write_apng(&decoder, output.expect("output was validated"))?;
            }
        }
        ResourceKind::Ezip | ResourceKind::Pixel => {
            if args.frames.is_some() {
                return Err(input_error("--frames requires an animated resource"));
            }
            let image = decoder.decode_frame(0, PixelFormat::Rgba8)?;
            write_png(
                output.expect("output was validated"),
                image.width(),
                image.height(),
                image.format(),
                image.pixels(),
            )?;
        }
        _ => return Err(input_error("unsupported resource kind")),
    }
    print_warnings(&decoder);
    Ok(())
}

fn encode(args: EncodeArgs) -> CliResult<()> {
    let extension = extension(&args.input)?;
    let encoded = match extension.as_str() {
        "toml" => encode_manifest(&args)?,
        "gif" => {
            let animation = load_gif(&args.input)?;
            encode_animation(&args, animation, None)?
        }
        "png" | "apng" => {
            let loaded = load_png(&args.input)?;
            if !loaded.animated {
                let options = build_options(&args, None)?;
                let frame = &loaded.frames[0];
                Encoder::new(options).encode(frame.image()?)?
            } else {
                encode_animation(&args, loaded, None)?
            }
        }
        _ => return Err(input_error("input must be PNG, APNG, GIF, or TOML")),
    };
    fs::write(&args.output, encoded.as_bytes())?;
    println!(
        "encoded {} bytes as {:?}",
        encoded.as_bytes().len(),
        encoded.storage_format()
    );
    Ok(())
}

fn encode_manifest(args: &EncodeArgs) -> CliResult<ezipr::EncodedResource> {
    if args.pixel {
        return Err(input_error(
            "--pixel cannot be used with an animation manifest",
        ));
    }
    let source = fs::read_to_string(&args.input)?;
    let manifest: AnimationManifest = toml::from_str(&source)?;
    if manifest.frames.is_empty() {
        return Err(input_error("animation manifest has no frames"));
    }
    let repeat = manifest_repeat(manifest.repeat)?;
    let base = args.input.parent().unwrap_or_else(|| Path::new("."));
    let mut loaded = LoadedAnimation {
        width: manifest.width,
        height: manifest.height,
        repeat,
        animated: true,
        frames: Vec::with_capacity(manifest.frames.len()),
    };
    for entry in manifest.frames {
        let image_path = base.join(entry.file);
        let image = load_png(&image_path)?;
        if image.animated || image.frames.len() != 1 {
            return Err(input_error(format!(
                "manifest frame {} is animated",
                image_path.display()
            )));
        }
        let mut frame = image
            .frames
            .into_iter()
            .next()
            .expect("one frame was checked");
        frame.x = entry.x;
        frame.y = entry.y;
        frame.delay_numerator = entry.delay_numerator;
        frame.delay_denominator = entry.delay_denominator;
        frame.disposal = disposal_from_manifest(entry.disposal);
        frame.blend = blend_from_manifest(entry.blend);
        loaded.frames.push(frame);
    }
    let manifest_options = ManifestOptions {
        depth: manifest.depth,
        alpha: manifest.alpha,
        block_rows: manifest.block_rows,
        filters: manifest.filters,
        compression: manifest.compression,
    };
    encode_animation(args, loaded, Some(manifest_options))
}

#[derive(Clone, Copy)]
struct ManifestOptions {
    depth: Option<ManifestDepth>,
    alpha: Option<ManifestAlpha>,
    block_rows: Option<u8>,
    filters: Option<bool>,
    compression: Option<u8>,
}

fn encode_animation(
    args: &EncodeArgs,
    animation: LoadedAnimation,
    manifest: Option<ManifestOptions>,
) -> CliResult<ezipr::EncodedResource> {
    if args.pixel {
        return Err(input_error("--pixel cannot be used with an animation"));
    }
    let options = build_options(args, manifest)?;
    let mut encoder =
        AnimationEncoder::new(animation.width, animation.height, animation.repeat, options)?;
    for frame in &animation.frames {
        encoder.push_frame(
            FrameView::new(
                frame.image()?,
                frame.x,
                frame.y,
                frame.delay_numerator,
                frame.delay_denominator,
            )
            .disposal(frame.disposal)
            .blend(frame.blend),
        )?;
    }
    Ok(encoder.finish()?)
}

fn build_options(args: &EncodeArgs, manifest: Option<ManifestOptions>) -> CliResult<EncodeOptions> {
    let depth = args
        .depth
        .map(depth_from_arg)
        .or_else(|| manifest.and_then(|value| value.depth.map(depth_from_manifest)))
        .unwrap_or(ColorDepth::Rgb565);
    let alpha = args
        .alpha
        .map(alpha_from_arg)
        .or_else(|| manifest.and_then(|value| value.alpha.map(alpha_from_manifest)))
        .unwrap_or(AlphaMode::Auto);
    let block_rows = args
        .block_rows
        .or_else(|| manifest.and_then(|value| value.block_rows))
        .unwrap_or(32);
    let filters = if args.no_filters {
        false
    } else {
        manifest.and_then(|value| value.filters).unwrap_or(true)
    };
    let compression = args
        .compression
        .or_else(|| manifest.and_then(|value| value.compression))
        .unwrap_or(6);
    let encoding = if args.pixel {
        ResourceEncoding::Pixel
    } else {
        ResourceEncoding::Ezip
    };
    let options = EncodeOptions::new(depth)
        .alpha_mode(alpha)
        .resource_encoding(encoding)
        .row_filters(filters)
        .block_rows(block_rows)?
        .compression_level(compression)?;
    Ok(options)
}

fn load_png(path: &Path) -> CliResult<LoadedAnimation> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let canvas_width = reader.info().width;
    let canvas_height = reader.info().height;
    let animation = reader.info().animation_control;
    let default_is_separate = animation.is_some() && reader.info().frame_control.is_none();
    let total = animation
        .map(|control| control.num_frames as usize + usize::from(default_is_separate))
        .unwrap_or(1);
    let repeat = match animation.map(|control| control.num_plays) {
        Some(0) => Repeat::Infinite,
        Some(count) => Repeat::Finite(count),
        None => Repeat::Finite(1),
    };
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| input_error("PNG image is too large"))?;
    let mut buffer = vec![0; buffer_size];
    let mut frames = Vec::with_capacity(total);
    for index in 0..total {
        let output = reader.next_frame(&mut buffer)?;
        if default_is_separate && index == 0 {
            continue;
        }
        let control = reader.info().frame_control;
        let (x, y, delay_numerator, delay_denominator, disposal, blend) = control
            .map(|frame| {
                (
                    frame.x_offset,
                    frame.y_offset,
                    frame.delay_num,
                    frame.delay_den,
                    disposal_from_png(frame.dispose_op),
                    blend_from_png(frame.blend_op),
                )
            })
            .unwrap_or((0, 0, 0, 0, DisposalMethod::None, BlendMode::Source));
        let (pixels, format) = normalize_png_frame(&buffer[..output.buffer_size()], &output)?;
        frames.push(LoadedFrame {
            width: output.width,
            height: output.height,
            pixels,
            format,
            x,
            y,
            delay_numerator,
            delay_denominator,
            disposal,
            blend,
        });
    }
    if frames.is_empty() {
        return Err(input_error("PNG contains no animation frames"));
    }
    Ok(LoadedAnimation {
        width: canvas_width,
        height: canvas_height,
        repeat,
        animated: animation.is_some(),
        frames,
    })
}

fn normalize_png_frame(
    input: &[u8],
    output: &png::OutputInfo,
) -> CliResult<(Vec<u8>, PixelFormat)> {
    if output.bit_depth != png::BitDepth::Eight {
        return Err(input_error(
            "PNG normalization did not produce 8-bit samples",
        ));
    }
    match output.color_type {
        png::ColorType::Rgb => Ok((input.to_vec(), PixelFormat::Rgb8)),
        png::ColorType::Rgba => Ok((input.to_vec(), PixelFormat::Rgba8)),
        png::ColorType::Grayscale => {
            let mut pixels = Vec::with_capacity(input.len() * 3);
            for &value in input {
                pixels.extend_from_slice(&[value, value, value]);
            }
            Ok((pixels, PixelFormat::Rgb8))
        }
        png::ColorType::GrayscaleAlpha => {
            let mut pixels = Vec::with_capacity(input.len() * 2);
            for sample in input.chunks_exact(2) {
                pixels.extend_from_slice(&[sample[0], sample[0], sample[0], sample[1]]);
            }
            Ok((pixels, PixelFormat::Rgba8))
        }
        png::ColorType::Indexed => Err(input_error("indexed PNG was not expanded")),
    }
}

fn load_gif(path: &Path) -> CliResult<LoadedAnimation> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(File::open(path)?)?;
    let width = u32::from(decoder.width());
    let height = u32::from(decoder.height());
    let repeat = match decoder.repeat() {
        gif::Repeat::Infinite => Repeat::Infinite,
        gif::Repeat::Finite(0) => Repeat::Finite(1),
        gif::Repeat::Finite(count) => Repeat::Finite(u32::from(count)),
    };
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame()? {
        frames.push(LoadedFrame {
            width: u32::from(frame.width),
            height: u32::from(frame.height),
            pixels: frame.buffer.to_vec(),
            format: PixelFormat::Rgba8,
            x: u32::from(frame.left),
            y: u32::from(frame.top),
            delay_numerator: frame.delay,
            delay_denominator: 100,
            disposal: match frame.dispose {
                gif::DisposalMethod::Any | gif::DisposalMethod::Keep => DisposalMethod::None,
                gif::DisposalMethod::Background => DisposalMethod::Background,
                gif::DisposalMethod::Previous => DisposalMethod::Previous,
            },
            blend: BlendMode::Over,
        });
    }
    if frames.is_empty() {
        return Err(input_error("GIF contains no frames"));
    }
    Ok(LoadedAnimation {
        width,
        height,
        repeat,
        animated: true,
        frames,
    })
}

fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: &[u8],
) -> CliResult<()> {
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), width, height);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_color(match format {
        PixelFormat::Rgb8 => png::ColorType::Rgb,
        PixelFormat::Rgba8 => png::ColorType::Rgba,
        _ => return Err(input_error("unsupported output pixel format")),
    });
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}

fn write_apng(decoder: &Decoder<'_>, path: &Path) -> CliResult<()> {
    let info = decoder.info();
    let frame_count = u32::try_from(info.frame_count())?;
    let repeat = match decoder.repeat().expect("animation has repeat metadata") {
        Repeat::Infinite => 0,
        Repeat::Finite(count) => count,
        _ => return Err(input_error("unsupported animation repeat mode")),
    };
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        info.width(),
        info.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(frame_count, repeat)?;
    encoder.set_sep_def_img(true)?;
    let mut writer = encoder.write_header()?;
    let default_size = (info.width() as usize)
        .checked_mul(info.height() as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| input_error("APNG default image size overflow"))?;
    writer.write_image_data(&vec![0; default_size])?;
    for index in 0..info.frame_count() {
        let frame_info = decoder.frame_info(index)?;
        let image = decoder.decode_frame(index, PixelFormat::Rgba8)?;
        writer.reset_frame_position()?;
        writer.set_frame_dimension(frame_info.width(), frame_info.height())?;
        writer.set_frame_position(frame_info.x_offset(), frame_info.y_offset())?;
        writer.set_frame_delay(frame_info.delay_numerator(), frame_info.delay_denominator())?;
        writer.set_dispose_op(disposal_to_png(frame_info.disposal())?)?;
        writer.set_blend_op(blend_to_png(frame_info.blend())?)?;
        writer.write_image_data(image.pixels())?;
    }
    Ok(())
}

fn write_composited_frames(decoder: &Decoder<'_>, directory: &Path) -> CliResult<()> {
    fs::create_dir_all(directory)?;
    let mut compositor = decoder.compositor(PixelFormat::Rgba8)?;
    let mut index = 0;
    while let Some(image) = compositor.next_frame()? {
        let path = directory.join(format!("frame-{index:04}.png"));
        write_png(
            &path,
            image.width(),
            image.height(),
            image.format(),
            image.pixels(),
        )?;
        index += 1;
    }
    Ok(())
}

fn print_warnings(decoder: &Decoder<'_>) {
    for warning in decoder.warnings() {
        eprintln!("warning: {warning}");
    }
}

fn kind_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Ezip => "eZIP",
        ResourceKind::Pixel => "PIXEL",
        ResourceKind::Animation => "eZIP-A",
        _ => "unknown",
    }
}

fn repeat_name(repeat: Repeat) -> String {
    match repeat {
        Repeat::Infinite => "infinite".to_owned(),
        Repeat::Finite(count) => count.to_string(),
        _ => "unknown".to_owned(),
    }
}

fn extension(path: &Path) -> CliResult<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| input_error("input has no usable file extension"))
}

fn input_error(message: impl Into<String>) -> Box<dyn StdError> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn manifest_repeat(value: u32) -> CliResult<Repeat> {
    Ok(if value == 0 {
        Repeat::Infinite
    } else {
        Repeat::Finite(value)
    })
}

fn depth_from_arg(value: DepthArg) -> ColorDepth {
    match value {
        DepthArg::Rgb565 => ColorDepth::Rgb565,
        DepthArg::Rgb888 => ColorDepth::Rgb888,
    }
}

fn alpha_from_arg(value: AlphaArg) -> AlphaMode {
    match value {
        AlphaArg::Auto => AlphaMode::Auto,
        AlphaArg::Preserve => AlphaMode::Preserve,
        AlphaArg::Discard => AlphaMode::Discard,
    }
}

fn depth_from_manifest(value: ManifestDepth) -> ColorDepth {
    match value {
        ManifestDepth::Rgb565 => ColorDepth::Rgb565,
        ManifestDepth::Rgb888 => ColorDepth::Rgb888,
    }
}

fn alpha_from_manifest(value: ManifestAlpha) -> AlphaMode {
    match value {
        ManifestAlpha::Auto => AlphaMode::Auto,
        ManifestAlpha::Preserve => AlphaMode::Preserve,
        ManifestAlpha::Discard => AlphaMode::Discard,
    }
}

fn disposal_from_manifest(value: ManifestDisposal) -> DisposalMethod {
    match value {
        ManifestDisposal::None => DisposalMethod::None,
        ManifestDisposal::Background => DisposalMethod::Background,
        ManifestDisposal::Previous => DisposalMethod::Previous,
    }
}

fn blend_from_manifest(value: ManifestBlend) -> BlendMode {
    match value {
        ManifestBlend::Source => BlendMode::Source,
        ManifestBlend::Over => BlendMode::Over,
    }
}

fn disposal_from_png(value: png::DisposeOp) -> DisposalMethod {
    match value {
        png::DisposeOp::None => DisposalMethod::None,
        png::DisposeOp::Background => DisposalMethod::Background,
        png::DisposeOp::Previous => DisposalMethod::Previous,
    }
}

fn blend_from_png(value: png::BlendOp) -> BlendMode {
    match value {
        png::BlendOp::Source => BlendMode::Source,
        png::BlendOp::Over => BlendMode::Over,
    }
}

fn disposal_to_png(value: DisposalMethod) -> CliResult<png::DisposeOp> {
    Ok(match value {
        DisposalMethod::None => png::DisposeOp::None,
        DisposalMethod::Background => png::DisposeOp::Background,
        DisposalMethod::Previous => png::DisposeOp::Previous,
        _ => return Err(input_error("unsupported animation disposal method")),
    })
}

fn blend_to_png(value: BlendMode) -> CliResult<png::BlendOp> {
    Ok(match value {
        BlendMode::Source => png::BlendOp::Source,
        BlendMode::Over => png::BlendOp::Over,
        _ => return Err(input_error("unsupported animation blend mode")),
    })
}
