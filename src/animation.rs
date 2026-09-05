use crate::stream::unfilter;
use crate::{
    AlphaMode, DecodeLimits, DecodeMode, DecodedImage, Decoder, EncodeOptions, EncodedResource,
    Error, ErrorKind, ImageView, PixelFormat, ResourceEncoding, ResourceHeader, Result,
    StorageFormat, StreamHeader, Warning, WarningKind,
};

const ANIMATION_HEADER_LEN: usize = 16;
const ANIMATION_CONTROL_LEN: usize = 8;
const FRAME_HEADER_LEN: usize = 30;

/// How a displayed frame region is treated before the following frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DisposalMethod {
    #[default]
    None,
    Background,
    Previous,
}

/// How a frame is combined with the existing canvas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum BlendMode {
    #[default]
    Source,
    Over,
}

/// Animation repetition behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Repeat {
    Infinite,
    Finite(u32),
}

/// Metadata for one stored animation frame rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameInfo {
    sequence: u32,
    width: u32,
    height: u32,
    x_offset: u32,
    y_offset: u32,
    delay_numerator: u16,
    delay_denominator: u16,
    disposal: DisposalMethod,
    blend: BlendMode,
}

impl FrameInfo {
    pub const fn sequence(self) -> u32 {
        self.sequence
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub const fn x_offset(self) -> u32 {
        self.x_offset
    }

    pub const fn y_offset(self) -> u32 {
        self.y_offset
    }

    pub const fn delay_numerator(self) -> u16 {
        self.delay_numerator
    }

    pub const fn delay_denominator(self) -> u16 {
        self.delay_denominator
    }

    pub const fn effective_delay_denominator(self) -> u16 {
        if self.delay_denominator == 0 {
            100
        } else {
            self.delay_denominator
        }
    }

    pub const fn disposal(self) -> DisposalMethod {
        self.disposal
    }

    pub const fn blend(self) -> BlendMode {
        self.blend
    }

    pub(crate) const fn still(width: u32, height: u32) -> Self {
        Self {
            sequence: 0,
            width,
            height,
            x_offset: 0,
            y_offset: 0,
            delay_numerator: 0,
            delay_denominator: 0,
            disposal: DisposalMethod::None,
            blend: BlendMode::Source,
        }
    }
}

/// Borrowed pixels and metadata supplied for one animation frame rectangle.
#[derive(Clone, Copy, Debug)]
pub struct FrameView<'a> {
    image: ImageView<'a>,
    x_offset: u32,
    y_offset: u32,
    delay_numerator: u16,
    delay_denominator: u16,
    disposal: DisposalMethod,
    blend: BlendMode,
}

impl<'a> FrameView<'a> {
    pub fn new(
        image: ImageView<'a>,
        x_offset: u32,
        y_offset: u32,
        delay_numerator: u16,
        delay_denominator: u16,
    ) -> Self {
        Self {
            image,
            x_offset,
            y_offset,
            delay_numerator,
            delay_denominator,
            disposal: DisposalMethod::None,
            blend: BlendMode::Source,
        }
    }

    pub fn disposal(mut self, disposal: DisposalMethod) -> Self {
        self.disposal = disposal;
        self
    }

    pub fn blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    pub const fn image(self) -> ImageView<'a> {
        self.image
    }

    pub const fn x_offset(self) -> u32 {
        self.x_offset
    }

    pub const fn y_offset(self) -> u32 {
        self.y_offset
    }

    pub const fn delay_numerator(self) -> u16 {
        self.delay_numerator
    }

    pub const fn delay_denominator(self) -> u16 {
        self.delay_denominator
    }

    pub const fn disposal_method(self) -> DisposalMethod {
        self.disposal
    }

    pub const fn blend_mode(self) -> BlendMode {
        self.blend
    }
}

#[derive(Debug)]
struct OwnedFrame {
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
    x_offset: u32,
    y_offset: u32,
    delay_numerator: u16,
    delay_denominator: u16,
    disposal: DisposalMethod,
    blend: BlendMode,
}

/// Builder for a complete eZIP-A animation resource.
#[derive(Debug)]
pub struct AnimationEncoder {
    width: u16,
    height: u16,
    repeat: Repeat,
    options: EncodeOptions,
    frames: Vec<OwnedFrame>,
}

impl AnimationEncoder {
    pub fn new(width: u32, height: u32, repeat: Repeat, options: EncodeOptions) -> Result<Self> {
        let width = u16::try_from(width).map_err(|_| {
            Error::new(
                ErrorKind::InvalidDimensions,
                "animation width does not fit the resource header",
            )
        })?;
        let height = u16::try_from(height).map_err(|_| {
            Error::new(
                ErrorKind::InvalidDimensions,
                "animation height does not fit the resource header",
            )
        })?;
        ResourceHeader::new(crate::ResourceFormat::Ezip, width, height)?;
        if options.encoding() != ResourceEncoding::Ezip {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "animation encoding requires the eZIP representation",
            ));
        }
        crate::encoder::validate_compression_strategy(options)?;
        if repeat == Repeat::Finite(0) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "a finite animation repeat count must be positive",
            ));
        }
        Ok(Self {
            width,
            height,
            repeat,
            options,
            frames: Vec::new(),
        })
    }

    pub const fn width(&self) -> u16 {
        self.width
    }

    pub const fn height(&self) -> u16 {
        self.height
    }

    pub const fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub const fn options(&self) -> EncodeOptions {
        self.options
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn push_frame(&mut self, frame: FrameView<'_>) -> Result<()> {
        let image = frame.image;
        if frame
            .x_offset
            .checked_add(image.width())
            .is_none_or(|right| right > u32::from(self.width))
            || frame
                .y_offset
                .checked_add(image.height())
                .is_none_or(|bottom| bottom > u32::from(self.height))
        {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                format!(
                    "frame rectangle {}x{}+{}+{} exceeds the {}x{} canvas",
                    image.width(),
                    image.height(),
                    frame.x_offset,
                    frame.y_offset,
                    self.width,
                    self.height
                ),
            ));
        }
        let row_bytes = image.width() as usize * image.format().bytes_per_pixel();
        let capacity = row_bytes
            .checked_mul(image.height() as usize)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "frame size overflow"))?;
        let mut pixels = Vec::with_capacity(capacity);
        for row in 0..image.height() as usize {
            let start = row * image.stride();
            pixels.extend_from_slice(&image.pixels()[start..start + row_bytes]);
        }
        self.frames.push(OwnedFrame {
            width: image.width(),
            height: image.height(),
            format: image.format(),
            pixels,
            x_offset: frame.x_offset,
            y_offset: frame.y_offset,
            delay_numerator: frame.delay_numerator,
            delay_denominator: frame.delay_denominator,
            disposal: frame.disposal,
            blend: frame.blend,
        });
        Ok(())
    }

    pub fn finish(self) -> Result<EncodedResource> {
        if self.frames.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidAnimation,
                "cannot encode an animation without frames",
            ));
        }
        let frame_count = u32::try_from(self.frames.len())
            .map_err(|_| Error::new(ErrorKind::LimitExceeded, "too many animation frames"))?;
        let has_alpha = match self.options.alpha_policy() {
            AlphaMode::Preserve => true,
            AlphaMode::Discard => false,
            AlphaMode::Auto => self.frames.iter().any(|frame| {
                frame.format == PixelFormat::Rgba8
                    && frame.pixels.chunks_exact(4).any(|pixel| pixel[3] != 255)
            }),
        };
        let storage = crate::encoder::resolve_storage(self.options.color_depth(), has_alpha);
        let resource_format =
            crate::encoder::resolve_resource_format(ResourceEncoding::Ezip, storage);
        let outer = ResourceHeader::new(resource_format, self.width, self.height)?;

        let mut stored_frames = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let stride = frame.width as usize * frame.format.bytes_per_pixel();
            let image = ImageView::new(
                frame.width,
                frame.height,
                frame.format,
                stride,
                &frame.pixels,
            )?;
            stored_frames.push(crate::encoder::encode_storage_pixels(
                image,
                storage,
                self.options.rgb565_dithering(),
                frame.x_offset,
                frame.y_offset,
            )?);
        }
        let compress_frames = |options| {
            self.frames
                .iter()
                .zip(&stored_frames)
                .map(|(frame, stored)| {
                    crate::encoder::compress_animation_pixels_miniz(
                        stored,
                        frame.width as usize,
                        frame.height as usize,
                        storage.bytes_per_pixel(),
                        options,
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut compressed_frames = compress_frames(self.options);
        if self.options.strategy() == crate::CompressionStrategy::Smallest
            && self.options.uses_row_filters()
        {
            let filterless_frames = compress_frames(self.options.row_filters(false));
            if compressed_frame_data_size(&filterless_frames)
                < compressed_frame_data_size(&compressed_frames)
            {
                compressed_frames = filterless_frames;
            }
        }
        if self.options.strategy() == crate::CompressionStrategy::Smallest {
            compressed_frames = compressed_frames
                .into_iter()
                .map(crate::encoder::optimize_with_zopfli)
                .collect::<Result<Vec<_>>>()?;
        }
        // The miniz comparison replaces the entire vector at once, and the
        // Zopfli pass cannot alter filtering, so every frame has this mode.
        let has_row_filters = compressed_frames[0].has_row_filters;

        let table_len = self
            .frames
            .len()
            .checked_mul(4)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "frame table size overflow"))?;
        let mut inner =
            Vec::with_capacity(ANIMATION_HEADER_LEN + ANIMATION_CONTROL_LEN + table_len);
        inner.extend_from_slice(&[0; 4]);
        inner.push(0x50 | crate::encoder::color_type(storage));
        inner.push(crate::encoder::storage_bit_depth(storage));
        inner.push(self.options.rows_per_block());
        inner.push(0);
        inner.extend_from_slice(&self.width.to_be_bytes());
        inner.extend_from_slice(&self.height.to_be_bytes());
        inner.push(u8::from(!has_row_filters));
        inner.extend_from_slice(&[0, 0, 0]);
        inner.extend_from_slice(&frame_count.to_be_bytes());
        let play_count = match self.repeat {
            Repeat::Infinite => 0,
            Repeat::Finite(count) => count,
        };
        inner.extend_from_slice(&play_count.to_be_bytes());
        let table_offset = inner.len();
        inner.resize(table_offset + table_len, 0);

        let mut offsets = Vec::with_capacity(self.frames.len());
        for (index, (frame, result)) in self.frames.iter().zip(compressed_frames).enumerate() {
            if index != 0 {
                while !inner.len().is_multiple_of(4) {
                    inner.push(0);
                }
            }
            offsets.push(u32::try_from(inner.len()).map_err(|_| {
                Error::new(ErrorKind::LimitExceeded, "animation offset exceeds 32 bits")
            })?);
            inner.extend_from_slice(&(index as u32).to_be_bytes());
            inner.extend_from_slice(&frame.width.to_be_bytes());
            inner.extend_from_slice(&frame.height.to_be_bytes());
            inner.extend_from_slice(&frame.x_offset.to_be_bytes());
            inner.extend_from_slice(&frame.y_offset.to_be_bytes());
            inner.extend_from_slice(&frame.delay_numerator.to_be_bytes());
            inner.extend_from_slice(&frame.delay_denominator.to_be_bytes());
            inner.push(match frame.disposal {
                DisposalMethod::None => 0,
                DisposalMethod::Background => 1,
                DisposalMethod::Previous => 2,
            });
            inner.push(match frame.blend {
                BlendMode::Source => 0,
                BlendMode::Over => 1,
            });
            let compressed_size = u32::try_from(result.compressed.len()).map_err(|_| {
                Error::new(ErrorKind::LimitExceeded, "compressed frame exceeds 32 bits")
            })?;
            inner.extend_from_slice(&((compressed_size >> 16) as u16).to_be_bytes());
            inner.extend_from_slice(&(compressed_size as u16).to_be_bytes());
            inner.extend_from_slice(&result.compressed);
            let checksum =
                miniz_oxide::mz_adler32_oxide(miniz_oxide::MZ_ADLER32_INIT, &result.filtered);
            inner.extend_from_slice(&checksum.to_be_bytes());
        }
        for (index, offset) in offsets.into_iter().enumerate() {
            inner[table_offset + index * 4..table_offset + index * 4 + 4]
                .copy_from_slice(&offset.to_be_bytes());
        }
        let declared_size = u32::try_from(inner.len()).map_err(|_| {
            Error::new(
                ErrorKind::LimitExceeded,
                "animation container exceeds 32 bits",
            )
        })?;
        inner[0..4].copy_from_slice(&declared_size.to_be_bytes());
        let crc = crc32fast::hash(&inner);
        let mut bytes = outer.to_bytes().to_vec();
        bytes.extend_from_slice(&inner);
        bytes.extend_from_slice(&crc.to_le_bytes());
        Ok(EncodedResource::new(bytes, storage, resource_format))
    }
}

fn compressed_frame_data_size(frames: &[crate::encoder::CompressionResult]) -> usize {
    frames
        .iter()
        .enumerate()
        .fold(0_usize, |total, (index, frame)| {
            let size = FRAME_HEADER_LEN
                .saturating_add(frame.compressed.len())
                .saturating_add(StreamHeader::CHECKSUM_LEN);
            let aligned = if index + 1 == frames.len() {
                size
            } else {
                size.saturating_add(3) & !3
            };
            total.saturating_add(aligned)
        })
}

#[derive(Debug)]
pub(crate) struct AnimationFrame {
    pub info: FrameInfo,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct AnimationData {
    pub repeat: Repeat,
    pub frames: Vec<AnimationFrame>,
}

pub(crate) fn parse_animation(
    stream: &[u8],
    header: StreamHeader,
    storage: StorageFormat,
    mode: DecodeMode,
    limits: DecodeLimits,
    warnings: &mut Vec<Warning>,
) -> Result<AnimationData> {
    validate_container_crc(stream, header, mode, warnings)?;
    let declared = header.data_size() as usize;
    let palette_bytes = usize::from(header.palette_count())
        .checked_mul(4)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "palette size overflow"))?;
    if palette_bytes != 0 {
        return Err(Error::new(
            ErrorKind::UnsupportedFormat,
            "palette animation decoding requires a verified palette fixture",
        ));
    }
    let control_offset = ANIMATION_HEADER_LEN + palette_bytes;
    let control = stream
        .get(control_offset..control_offset + ANIMATION_CONTROL_LEN)
        .ok_or_else(|| Error::new(ErrorKind::TruncatedData, "animation control is truncated"))?;
    let frame_count = u32::from_be_bytes(control[0..4].try_into().expect("fixed-size field"));
    let play_count = u32::from_be_bytes(control[4..8].try_into().expect("fixed-size field"));
    let frame_count = usize::try_from(frame_count)
        .map_err(|_| Error::new(ErrorKind::LimitExceeded, "frame count does not fit usize"))?;
    if frame_count == 0 {
        return Err(Error::new(
            ErrorKind::InvalidAnimation,
            "animation contains no frames",
        ));
    }
    if frame_count > limits.frame_limit() {
        return Err(Error::new(
            ErrorKind::LimitExceeded,
            format!("animation has {frame_count} frames, exceeding the configured limit"),
        ));
    }
    let table_offset = control_offset + ANIMATION_CONTROL_LEN;
    let table_len = frame_count
        .checked_mul(4)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "frame table size overflow"))?;
    let table = stream
        .get(table_offset..table_offset + table_len)
        .ok_or_else(|| Error::new(ErrorKind::TruncatedData, "frame-offset table is truncated"))?;
    let mut offsets = Vec::with_capacity(frame_count);
    for (index, bytes) in table.chunks_exact(4).enumerate() {
        let offset = u32::from_be_bytes(bytes.try_into().expect("fixed-size field")) as usize;
        if !offset.is_multiple_of(4) || offset < table_offset + table_len || offset >= declared {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("animation frame {index} has invalid offset {offset}"),
            )
            .in_frame(index)
            .at_offset(table_offset + index * 4));
        }
        if offsets.last().is_some_and(|previous| *previous >= offset) {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                "animation frame offsets are not strictly increasing",
            )
            .in_frame(index));
        }
        offsets.push(offset);
    }

    let mut frames = Vec::with_capacity(frame_count);
    let mut total_decoded = 0_usize;
    for (index, &offset) in offsets.iter().enumerate() {
        let frame_end = offsets.get(index + 1).copied().unwrap_or(declared);
        let raw_header = stream
            .get(offset..offset + FRAME_HEADER_LEN)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::TruncatedData,
                    "animation frame header is truncated",
                )
                .in_frame(index)
                .at_offset(offset)
            })?;
        let sequence = be_u32(raw_header, 0);
        let width = be_u32(raw_header, 4);
        let height = be_u32(raw_header, 8);
        let x_offset = be_u32(raw_header, 12);
        let y_offset = be_u32(raw_header, 16);
        let delay_numerator = be_u16(raw_header, 20);
        let delay_denominator = be_u16(raw_header, 22);
        let disposal = match raw_header[24] {
            0 => DisposalMethod::None,
            1 => DisposalMethod::Background,
            2 => DisposalMethod::Previous,
            value => {
                return Err(Error::new(
                    ErrorKind::InvalidAnimation,
                    format!("frame {index} has invalid disposal operation {value}"),
                )
                .in_frame(index)
                .at_offset(offset + 24));
            }
        };
        let blend = match raw_header[25] {
            0 => BlendMode::Source,
            1 => BlendMode::Over,
            value => {
                return Err(Error::new(
                    ErrorKind::InvalidAnimation,
                    format!("frame {index} has invalid blend operation {value}"),
                )
                .in_frame(index)
                .at_offset(offset + 25));
            }
        };
        let compressed_size =
            ((u32::from(be_u16(raw_header, 26))) << 16) | u32::from(be_u16(raw_header, 28));
        let compressed_size = compressed_size as usize;
        if width == 0
            || height == 0
            || x_offset
                .checked_add(width)
                .is_none_or(|right| right > u32::from(header.width()))
            || y_offset
                .checked_add(height)
                .is_none_or(|bottom| bottom > u32::from(header.height()))
        {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                format!(
                    "frame {index} rectangle {width}x{height}+{x_offset}+{y_offset} exceeds the canvas"
                ),
            )
            .in_frame(index));
        }
        if sequence != index as u32 {
            let message = format!("frame table index {index} contains sequence {sequence}");
            if mode == DecodeMode::Strict {
                return Err(Error::new(ErrorKind::InvalidAnimation, message).in_frame(index));
            }
            warnings.push(Warning::new(WarningKind::MetadataMismatch, message).in_frame(index));
        }
        let compressed_offset = offset + FRAME_HEADER_LEN;
        let checksum_offset = compressed_offset
            .checked_add(compressed_size)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "frame size overflow"))?;
        if checksum_offset
            .checked_add(4)
            .is_none_or(|end| end > frame_end)
        {
            return Err(Error::new(
                ErrorKind::TruncatedData,
                format!("frame {index} compressed payload or checksum is truncated"),
            )
            .in_frame(index)
            .at_offset(compressed_offset));
        }
        let compressed = &stream[compressed_offset..checksum_offset];
        let decoded_size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|count| count.checked_mul(storage.bytes_per_pixel()))
            .and_then(|size| total_decoded.checked_add(size))
            .ok_or_else(|| {
                Error::new(ErrorKind::LimitExceeded, "decoded size overflow").in_frame(index)
            })?;
        if width > limits.width_limit()
            || height > limits.height_limit()
            || decoded_size > limits.decoded_byte_limit()
        {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                "animation frame exceeds configured decode limits",
            )
            .in_frame(index));
        }
        let remaining = limits.decoded_byte_limit().saturating_sub(total_decoded);
        let filtered = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, remaining)
            .map_err(|error| {
                let kind = if error.output.len() >= remaining {
                    ErrorKind::LimitExceeded
                } else {
                    ErrorKind::InvalidCompression
                };
                Error::new(
                    kind,
                    format!("invalid animation frame DEFLATE stream: {error}"),
                )
                .in_frame(index)
                .at_offset(compressed_offset)
            })?;
        let stored_checksum = u32::from_be_bytes(
            stream[checksum_offset..checksum_offset + 4]
                .try_into()
                .expect("checksum slice length was checked"),
        );
        let calculated = miniz_oxide::mz_adler32_oxide(miniz_oxide::MZ_ADLER32_INIT, &filtered);
        if stored_checksum != calculated {
            let message = format!(
                "frame {index} Adler-32 mismatch: stored {stored_checksum:08x}, calculated {calculated:08x}"
            );
            if mode == DecodeMode::Strict {
                return Err(Error::new(ErrorKind::ChecksumMismatch, message)
                    .in_frame(index)
                    .at_offset(checksum_offset));
            }
            warnings.push(
                Warning::new(WarningKind::ChecksumMismatch, message)
                    .in_frame(index)
                    .at_offset(checksum_offset),
            );
        }
        let padding = &stream[checksum_offset + 4..frame_end];
        if padding.len() > 3 || padding.iter().any(|&byte| byte != 0) {
            let message = format!("frame {index} has invalid alignment padding");
            if mode == DecodeMode::Strict {
                return Err(Error::new(ErrorKind::InvalidAnimation, message)
                    .in_frame(index)
                    .at_offset(checksum_offset + 4));
            }
            warnings.push(
                Warning::new(WarningKind::TrailingData, message)
                    .in_frame(index)
                    .at_offset(checksum_offset + 4),
            );
        }
        let decoded = unfilter(
            &filtered,
            width,
            height,
            storage.bytes_per_pixel(),
            header.block_rows(),
            header.has_row_filters(),
            mode,
        )
        .map_err(|error| error.in_frame(index))?;
        warnings.extend(
            decoded
                .warnings
                .into_iter()
                .map(|warning| warning.in_frame(index)),
        );
        total_decoded = total_decoded
            .checked_add(decoded.pixels.len())
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "decoded size overflow"))?;
        frames.push(AnimationFrame {
            info: FrameInfo {
                sequence,
                width,
                height,
                x_offset,
                y_offset,
                delay_numerator,
                delay_denominator,
                disposal,
                blend,
            },
            pixels: decoded.pixels,
        });
    }
    Ok(AnimationData {
        repeat: if play_count == 0 {
            Repeat::Infinite
        } else {
            Repeat::Finite(play_count)
        },
        frames,
    })
}

fn validate_container_crc(
    stream: &[u8],
    header: StreamHeader,
    mode: DecodeMode,
    warnings: &mut Vec<Warning>,
) -> Result<()> {
    let declared = header.data_size() as usize;
    if declared > stream.len() {
        return Err(Error::new(
            ErrorKind::TruncatedData,
            "animation is shorter than its declared size",
        ));
    }
    let trailer_len = stream.len() - declared;
    if trailer_len == 0 {
        return Ok(());
    }
    if trailer_len < 4 {
        let message =
            format!("animation CRC-32 trailer is truncated: expected 4 bytes, found {trailer_len}");
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::TruncatedData, message).at_offset(declared));
        }
        warnings.push(Warning::new(WarningKind::MissingChecksum, message).at_offset(declared));
        return Ok(());
    }
    let trailer_end = declared + 4;
    let stored = u32::from_le_bytes(
        stream[declared..trailer_end]
            .try_into()
            .expect("CRC slice length was checked"),
    );
    let calculated = crc32fast::hash(&stream[..declared]);
    if stored != calculated {
        let message =
            format!("animation CRC-32 mismatch: stored {stored:08x}, calculated {calculated:08x}");
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::ChecksumMismatch, message).at_offset(declared));
        }
        warnings.push(Warning::new(WarningKind::ChecksumMismatch, message).at_offset(declared));
    }
    if stream.len() > trailer_end {
        let message = format!(
            "ignored {} bytes after the animation CRC-32",
            stream.len() - trailer_end
        );
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::InvalidAnimation, message).at_offset(trailer_end));
        }
        warnings.push(Warning::new(WarningKind::TrailingData, message).at_offset(trailer_end));
    }
    Ok(())
}

fn be_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(
        data[offset..offset + 2]
            .try_into()
            .expect("frame header length was checked"),
    )
}

fn be_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("frame header length was checked"),
    )
}

/// Stateful sequential resource compositor.
///
/// Static resources produce exactly one full-canvas frame. Animated resources
/// apply their frame blend and disposal operations as the compositor advances.
#[derive(Debug)]
pub struct Compositor<'decoder, 'data> {
    decoder: &'decoder Decoder<'data>,
    output: PixelFormat,
    canvas: Vec<u8>,
    next_index: usize,
    previous_frame: Option<FrameInfo>,
    restore_canvas: Option<Vec<u8>>,
}

impl<'decoder, 'data> Compositor<'decoder, 'data> {
    pub(crate) fn new(decoder: &'decoder Decoder<'data>, output: PixelFormat) -> Result<Self> {
        let canvas_len = (decoder.info().width() as usize)
            .checked_mul(decoder.info().height() as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "canvas size overflow"))?;
        decoder.validate_output_size(canvas_len)?;
        Ok(Self {
            decoder,
            output,
            canvas: vec![0; canvas_len],
            next_index: 0,
            previous_frame: None,
            restore_canvas: None,
        })
    }

    pub const fn next_index(&self) -> usize {
        self.next_index
    }

    pub const fn output_format(&self) -> PixelFormat {
        self.output
    }

    pub fn output_buffer_size(&self) -> Result<usize> {
        crate::pixels::decoded_pixel_len(
            self.decoder.info().width(),
            self.decoder.info().height(),
            self.output,
        )
    }

    pub fn reset(&mut self) {
        self.canvas.fill(0);
        self.next_index = 0;
        self.previous_frame = None;
        self.restore_canvas = None;
    }

    pub fn next_frame(&mut self) -> Result<Option<DecodedImage>> {
        if self.next_index >= self.decoder.info().frame_count() {
            return Ok(None);
        }
        let mut pixels = vec![0; self.output_buffer_size()?];
        self.next_frame_into(&mut pixels)?;
        Ok(Some(DecodedImage::new(
            self.decoder.info().width(),
            self.decoder.info().height(),
            self.output,
            pixels,
        )))
    }

    /// Compose the next frame into a caller-owned tightly packed buffer.
    pub fn next_frame_into(&mut self, destination: &mut [u8]) -> Result<Option<usize>> {
        if self.next_index >= self.decoder.info().frame_count() {
            return Ok(None);
        }
        let required = self.output_buffer_size()?;
        if destination.len() < required {
            return Err(Error::new(
                ErrorKind::OutputBufferTooSmall,
                format!(
                    "output buffer has {} bytes; {required} required",
                    destination.len()
                ),
            ));
        }
        self.advance()?;
        Ok(Some(self.copy_canvas_into(destination)?))
    }

    pub(crate) fn advance(&mut self) -> Result<()> {
        self.apply_previous_disposal();
        let info = self.decoder.frame_info(self.next_index)?;
        self.restore_canvas = if info.disposal == DisposalMethod::Previous {
            Some(self.canvas.clone())
        } else {
            None
        };
        let frame = self
            .decoder
            .decode_frame(self.next_index, PixelFormat::Rgba8)?;
        self.draw(info, frame.pixels());
        self.previous_frame = Some(info);
        self.next_index += 1;

        Ok(())
    }

    pub(crate) fn copy_canvas_into(&self, destination: &mut [u8]) -> Result<usize> {
        let required = self.output_buffer_size()?;
        if destination.len() < required {
            return Err(Error::new(
                ErrorKind::OutputBufferTooSmall,
                format!(
                    "output buffer has {} bytes; {required} required",
                    destination.len()
                ),
            ));
        }
        match self.output {
            PixelFormat::Rgba8 => destination[..required].copy_from_slice(&self.canvas),
            PixelFormat::Rgb8 => {
                for (source, output) in self
                    .canvas
                    .chunks_exact(4)
                    .zip(destination[..required].chunks_exact_mut(3))
                {
                    output.copy_from_slice(&source[..3]);
                }
            }
        }
        Ok(required)
    }

    fn apply_previous_disposal(&mut self) {
        let Some(previous) = self.previous_frame else {
            return;
        };
        match previous.disposal {
            DisposalMethod::None => {}
            DisposalMethod::Background => clear_rectangle(
                &mut self.canvas,
                self.decoder.info().width() as usize,
                self.decoder.info().height() as usize,
                previous,
            ),
            DisposalMethod::Previous => {
                if let Some(restore) = self.restore_canvas.take() {
                    self.canvas = restore;
                }
            }
        }
    }

    fn draw(&mut self, info: FrameInfo, pixels: &[u8]) {
        let canvas_width = self.decoder.info().width() as usize;
        let canvas_height = self.decoder.info().height() as usize;
        let x_offset = info.x_offset as usize;
        let y_offset = info.y_offset as usize;
        let draw_width = (info.width as usize).min(canvas_width.saturating_sub(x_offset));
        let draw_height = (info.height as usize).min(canvas_height.saturating_sub(y_offset));
        for y in 0..draw_height {
            for x in 0..draw_width {
                let source_index = (y * info.width as usize + x) * 4;
                let destination_index = ((y + y_offset) * canvas_width + x + x_offset) * 4;
                let source: [u8; 4] = pixels[source_index..source_index + 4]
                    .try_into()
                    .expect("decoded RGBA pixel");
                if info.blend == BlendMode::Source {
                    self.canvas[destination_index..destination_index + 4].copy_from_slice(&source);
                } else {
                    let destination: [u8; 4] = self.canvas
                        [destination_index..destination_index + 4]
                        .try_into()
                        .expect("canvas RGBA pixel");
                    self.canvas[destination_index..destination_index + 4]
                        .copy_from_slice(&over(source, destination));
                }
            }
        }
    }
}

fn clear_rectangle(canvas: &mut [u8], canvas_width: usize, canvas_height: usize, info: FrameInfo) {
    let x_offset = info.x_offset as usize;
    let y_offset = info.y_offset as usize;
    let clear_width = (info.width as usize).min(canvas_width.saturating_sub(x_offset));
    let clear_height = (info.height as usize).min(canvas_height.saturating_sub(y_offset));
    if clear_width == 0 || clear_height == 0 {
        return;
    }
    for y in y_offset..y_offset + clear_height {
        let start = (y * canvas_width + x_offset) * 4;
        let end = start + clear_width * 4;
        canvas[start..end].fill(0);
    }
}

fn over(source: [u8; 4], destination: [u8; 4]) -> [u8; 4] {
    let source_alpha = u32::from(source[3]);
    let destination_alpha = u32::from(destination[3]);
    let inverse = 255 - source_alpha;
    let alpha_scaled = source_alpha * 255 + destination_alpha * inverse;
    if alpha_scaled == 0 {
        return [0; 4];
    }
    let mut output = [0; 4];
    for channel in 0..3 {
        let numerator = u32::from(source[channel]) * source_alpha * 255
            + u32::from(destination[channel]) * destination_alpha * inverse;
        output[channel] = ((numerator + alpha_scaled / 2) / alpha_scaled) as u8;
    }
    output[3] = ((alpha_scaled + 127) / 255) as u8;
    output
}
