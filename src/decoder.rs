use crate::pixels::{decode_storage_pixels, decode_storage_pixels_into, decoded_pixel_len};
use crate::stream::{inflate_stream, unfilter};
use crate::{
    DecodedImage, Error, ErrorKind, FrameInfo, PixelFormat, Repeat, ResourceFormat, ResourceHeader,
    ResourceKind, Result, StorageFormat, Warning,
};

/// Decoder behavior when recoverable inconsistencies are encountered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecodeMode {
    #[default]
    Strict,
    Diagnostic,
}

/// Resource limits applied before allocating decoded output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeLimits {
    max_width: u32,
    max_height: u32,
    max_frames: usize,
    max_decoded_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_width: 8_192,
            max_height: 8_192,
            max_frames: 10_000,
            max_decoded_bytes: 512 * 1024 * 1024,
        }
    }
}

impl DecodeLimits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_dimensions(mut self, width: u32, height: u32) -> Self {
        self.max_width = width;
        self.max_height = height;
        self
    }

    pub fn max_frames(mut self, frames: usize) -> Self {
        self.max_frames = frames;
        self
    }

    pub fn max_decoded_bytes(mut self, bytes: usize) -> Self {
        self.max_decoded_bytes = bytes;
        self
    }

    pub const fn width_limit(self) -> u32 {
        self.max_width
    }

    pub const fn height_limit(self) -> u32 {
        self.max_height
    }

    pub const fn frame_limit(self) -> usize {
        self.max_frames
    }

    pub const fn decoded_byte_limit(self) -> usize {
        self.max_decoded_bytes
    }
}

/// Options controlling strictness and resource limits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeOptions {
    mode: DecodeMode,
    limits: DecodeLimits,
}

impl DecodeOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(mut self, mode: DecodeMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn limits(mut self, limits: DecodeLimits) -> Self {
        self.limits = limits;
        self
    }

    pub const fn decode_mode(self) -> DecodeMode {
        self.mode
    }

    pub const fn decode_limits(self) -> DecodeLimits {
        self.limits
    }
}

/// Metadata known without decoding a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceInfo {
    kind: ResourceKind,
    format: ResourceFormat,
    storage_format: StorageFormat,
    width: u32,
    height: u32,
    frame_count: usize,
}

impl ResourceInfo {
    pub const fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub const fn resource_format(&self) -> ResourceFormat {
        self.format
    }

    pub const fn storage_format(&self) -> StorageFormat {
        self.storage_format
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn frame_count(&self) -> usize {
        self.frame_count
    }
}

#[derive(Debug)]
enum Payload<'a> {
    Pixel(&'a [u8]),
    Ezip(Vec<u8>),
    Animation(crate::animation::AnimationData),
}

/// Parsed image resource borrowing its encoded bytes.
#[derive(Debug)]
pub struct Decoder<'a> {
    info: ResourceInfo,
    payload: Payload<'a>,
    options: DecodeOptions,
    warnings: Vec<Warning>,
}

impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        Self::with_options(data, DecodeOptions::default())
    }

    pub fn with_options(data: &'a [u8], options: DecodeOptions) -> Result<Self> {
        let header = ResourceHeader::parse(data)?;
        let width = u32::from(header.width());
        let height = u32::from(header.height());
        if width > options.limits.max_width || height > options.limits.max_height {
            return Err(Error::new(
                ErrorKind::LimitExceeded,
                format!("resource dimensions {width}x{height} exceed configured limits"),
            ));
        }
        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "pixel count overflow"))?;
        let payload = &data[ResourceHeader::BYTE_LEN..];

        match header.format().kind() {
            ResourceKind::Pixel => {
                let pixel_has_alpha = header.format() == ResourceFormat::PixelWithAlpha;
                let candidate_bpp: &[usize] = if pixel_has_alpha { &[3, 4] } else { &[2, 3] };
                let exact = candidate_bpp.iter().find_map(|&bpp| {
                    let pixel_bytes = pixel_count.checked_mul(bpp)?;
                    (payload.len() == pixel_bytes + 4).then_some((bpp, pixel_bytes))
                });
                let inferred = exact.is_none();
                let selected = exact.or_else(|| {
                    (options.mode == DecodeMode::Diagnostic).then(|| {
                        candidate_bpp
                            .iter()
                            .filter_map(|&bpp| {
                                let pixel_bytes = pixel_count.checked_mul(bpp)?;
                                (payload.len() >= pixel_bytes).then_some((bpp, pixel_bytes))
                            })
                            .min_by_key(|&(_, pixel_bytes)| payload.len().abs_diff(pixel_bytes))
                    })?
                });
                let (bpp, pixel_bytes) = selected.ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidPixelLayout,
                        format!(
                            "{} PIXEL payload bytes do not match {pixel_count} pixels and a CRC-32 trailer",
                            payload.len()
                        ),
                    )
                })?;
                if pixel_bytes > options.limits.max_decoded_bytes {
                    return Err(Error::new(
                        ErrorKind::LimitExceeded,
                        "PIXEL data exceeds configured decoded-byte limit",
                    ));
                }
                let storage = StorageFormat::from_alpha_and_bpp(pixel_has_alpha, bpp)?;
                let mut warnings = Vec::new();
                if inferred {
                    warnings.push(Warning::new(
                        crate::WarningKind::MetadataMismatch,
                        format!(
                            "inferred {storage:?} from noncanonical {}-byte PIXEL payload; expected {} bytes including CRC-32",
                            payload.len(),
                            pixel_bytes + 4
                        ),
                    ));
                }
                let pixels = &payload[..pixel_bytes];
                if payload.len() >= pixel_bytes + 4 {
                    let stored = u32::from_le_bytes(
                        payload[pixel_bytes..pixel_bytes + 4]
                            .try_into()
                            .expect("trailer length was checked"),
                    );
                    let calculated = crc32fast::hash(pixels);
                    if stored != calculated {
                        let message = format!(
                            "PIXEL CRC-32 mismatch: stored {stored:08x}, calculated {calculated:08x}"
                        );
                        if options.mode == DecodeMode::Strict {
                            return Err(Error::new(ErrorKind::ChecksumMismatch, message)
                                .at_offset(ResourceHeader::BYTE_LEN + pixel_bytes));
                        }
                        warnings.push(
                            Warning::new(crate::WarningKind::ChecksumMismatch, message)
                                .at_offset(ResourceHeader::BYTE_LEN + pixel_bytes),
                        );
                    }
                    if payload.len() > pixel_bytes + 4 {
                        warnings.push(
                            Warning::new(
                                crate::WarningKind::TrailingData,
                                format!(
                                    "ignored {} bytes after PIXEL CRC-32",
                                    payload.len() - pixel_bytes - 4
                                ),
                            )
                            .at_offset(ResourceHeader::BYTE_LEN + pixel_bytes + 4),
                        );
                    }
                } else {
                    warnings.push(
                        Warning::new(
                            crate::WarningKind::MissingChecksum,
                            "PIXEL resource has no CRC-32 trailer",
                        )
                        .at_offset(ResourceHeader::BYTE_LEN + pixel_bytes),
                    );
                }
                Ok(Self {
                    info: ResourceInfo {
                        kind: ResourceKind::Pixel,
                        format: header.format(),
                        storage_format: storage,
                        width,
                        height,
                        frame_count: 1,
                    },
                    payload: Payload::Pixel(pixels),
                    options,
                    warnings,
                })
            }
            ResourceKind::Ezip => {
                let stream_header = crate::StreamHeader::parse(payload)?;
                if stream_header.palette_count() != 0 {
                    return Err(Error::new(
                        ErrorKind::UnsupportedFormat,
                        "palette-backed eZIP resources require a verified fixture",
                    ));
                }
                let mut warnings = Vec::new();
                if (stream_header.width(), stream_header.height())
                    != (header.width(), header.height())
                {
                    let message = format!(
                        "resource dimensions {}x{} differ from eZIP stream dimensions {}x{}",
                        header.width(),
                        header.height(),
                        stream_header.width(),
                        stream_header.height()
                    );
                    if options.mode == DecodeMode::Strict {
                        return Err(Error::new(ErrorKind::InvalidDimensions, message));
                    }
                    warnings.push(Warning::new(crate::WarningKind::MetadataMismatch, message));
                }
                let storage = stream_header.storage_format()?;
                let outer_matches = match header.format() {
                    ResourceFormat::Ezip => storage != StorageFormat::Argb565,
                    ResourceFormat::EzipArgb565 => storage == StorageFormat::Argb565,
                    ResourceFormat::Pixel | ResourceFormat::PixelWithAlpha => unreachable!(),
                };
                if !outer_matches {
                    let message = format!(
                        "resource format {:?} does not match stream storage format {storage:?}",
                        header.format()
                    );
                    if options.mode == DecodeMode::Strict {
                        return Err(Error::new(ErrorKind::InvalidPixelLayout, message));
                    }
                    warnings.push(Warning::new(crate::WarningKind::MetadataMismatch, message));
                }
                let expected_bit_depth = match storage {
                    StorageFormat::Rgb565 => 16,
                    StorageFormat::Argb565 => 24,
                    StorageFormat::Rgb888 | StorageFormat::Argb888 => 8,
                };
                if stream_header.bit_depth() != expected_bit_depth {
                    let message = format!(
                        "eZIP bit depth {} does not match stream storage format {storage:?}",
                        stream_header.bit_depth()
                    );
                    if options.mode == DecodeMode::Strict {
                        return Err(Error::new(ErrorKind::InvalidPixelLayout, message));
                    }
                    warnings.push(Warning::new(crate::WarningKind::MetadataMismatch, message));
                }
                let bytes_per_pixel = storage.bytes_per_pixel();
                if stream_header.is_animation() {
                    let animation = crate::animation::parse_animation(
                        payload,
                        stream_header,
                        storage,
                        options.mode,
                        options.limits,
                        &mut warnings,
                    )?;
                    let frame_count = animation.frames.len();
                    return Ok(Self {
                        info: ResourceInfo {
                            kind: ResourceKind::Animation,
                            format: header.format(),
                            storage_format: storage,
                            width,
                            height,
                            frame_count,
                        },
                        payload: Payload::Animation(animation),
                        options,
                        warnings,
                    });
                }
                let inflated = inflate_stream(
                    payload,
                    stream_header,
                    options.limits.max_decoded_bytes,
                    options.mode,
                    &mut warnings,
                )?;
                let decoded = unfilter(
                    &inflated.bytes,
                    width,
                    height,
                    bytes_per_pixel,
                    inflated.block_rows,
                    stream_header.has_row_filters(),
                    options.mode,
                )?;
                warnings.extend(decoded.warnings);
                Ok(Self {
                    info: ResourceInfo {
                        kind: ResourceKind::Ezip,
                        format: header.format(),
                        storage_format: storage,
                        width,
                        height,
                        frame_count: 1,
                    },
                    payload: Payload::Ezip(decoded.pixels),
                    options,
                    warnings,
                })
            }
            ResourceKind::Animation => unreachable!("no static resource ID maps to animation"),
        }
    }

    pub const fn info(&self) -> &ResourceInfo {
        &self.info
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    pub const fn options(&self) -> DecodeOptions {
        self.options
    }

    pub fn repeat(&self) -> Option<Repeat> {
        match &self.payload {
            Payload::Animation(animation) => Some(animation.repeat),
            Payload::Pixel(_) | Payload::Ezip(_) => None,
        }
    }

    pub fn frame_info(&self, index: usize) -> Result<FrameInfo> {
        match &self.payload {
            Payload::Animation(animation) => animation
                .frames
                .get(index)
                .map(|frame| frame.info)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOffset,
                        format!("frame index {index} is outside the animation"),
                    )
                    .in_frame(index)
                }),
            Payload::Pixel(_) | Payload::Ezip(_) if index == 0 => {
                Ok(FrameInfo::still(self.info.width, self.info.height))
            }
            Payload::Pixel(_) | Payload::Ezip(_) => Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("frame index {index} is outside a one-frame resource"),
            )
            .in_frame(index)),
        }
    }

    /// Create a sequential compositor for this resource.
    ///
    /// A static resource yields one full-canvas source frame and then ends.
    pub fn compositor(&self, output: PixelFormat) -> Result<crate::Compositor<'_, 'a>> {
        crate::Compositor::new(self, output)
    }

    /// Compose a frame by replaying the resource from its beginning.
    ///
    /// Repeated random access is quadratic; use [`Self::compositor`] when
    /// consuming frames sequentially.
    pub fn decode_composited_frame(
        &self,
        index: usize,
        output: PixelFormat,
    ) -> Result<DecodedImage> {
        if index >= self.info.frame_count {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("frame index {index} is outside the animation"),
            )
            .in_frame(index));
        }
        let mut compositor = self.compositor(output)?;
        let mut image = None;
        for _ in 0..=index {
            image = compositor.next_frame()?;
        }
        image.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidAnimation,
                "animation ended before the requested frame",
            )
        })
    }

    /// Compose a frame into a caller-owned tightly packed output buffer.
    ///
    /// This replays the animation from its beginning and therefore takes time
    /// proportional to `index`. Use [`Self::compositor`] for sequential access.
    pub fn decode_composited_frame_into(
        &self,
        index: usize,
        output: PixelFormat,
        destination: &mut [u8],
    ) -> Result<usize> {
        if index >= self.info.frame_count {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("frame index {index} is outside the animation"),
            )
            .in_frame(index));
        }
        let required = decoded_pixel_len(self.info.width, self.info.height, output)?;
        if destination.len() < required {
            return Err(Error::new(
                ErrorKind::OutputBufferTooSmall,
                format!(
                    "output buffer has {} bytes; {required} required",
                    destination.len()
                ),
            ));
        }
        let mut compositor = self.compositor(output)?;
        for _ in 0..=index {
            compositor.advance()?;
        }
        compositor.copy_canvas_into(destination)
    }

    /// Number of bytes required to decode a stored frame in `output` format.
    pub fn frame_buffer_size(&self, index: usize, output: PixelFormat) -> Result<usize> {
        let info = self.frame_info(index)?;
        decoded_pixel_len(info.width(), info.height(), output)
    }

    /// Decode a stored frame rectangle into a caller-owned tightly packed buffer.
    pub fn decode_frame_into(
        &self,
        index: usize,
        output: PixelFormat,
        destination: &mut [u8],
    ) -> Result<usize> {
        let (payload, width, height) = self.frame_payload(index)?;
        decode_storage_pixels_into(
            payload,
            width,
            height,
            self.info.storage_format,
            output,
            destination,
        )
    }

    pub fn decode_frame(&self, index: usize, output: PixelFormat) -> Result<DecodedImage> {
        let (payload, width, height) = self.frame_payload(index)?;
        let pixels =
            decode_storage_pixels(payload, width, height, self.info.storage_format, output)?;
        Ok(DecodedImage::new(width, height, output, pixels))
    }

    fn frame_payload(&self, index: usize) -> Result<(&[u8], u32, u32)> {
        match &self.payload {
            Payload::Pixel(payload) if index == 0 => {
                Ok((*payload, self.info.width, self.info.height))
            }
            Payload::Ezip(payload) if index == 0 => {
                Ok((payload.as_slice(), self.info.width, self.info.height))
            }
            Payload::Animation(animation) => {
                let frame = animation.frames.get(index).ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidOffset,
                        format!("frame index {index} is outside the animation"),
                    )
                    .in_frame(index)
                })?;
                Ok((
                    frame.pixels.as_slice(),
                    frame.info.width(),
                    frame.info.height(),
                ))
            }
            Payload::Pixel(_) | Payload::Ezip(_) => {
                return Err(Error::new(
                    ErrorKind::InvalidOffset,
                    format!("frame index {index} is outside a one-frame resource"),
                )
                .in_frame(index));
            }
        }
    }
}
