use crate::pixels::decode_storage_pixels;
use crate::{
    DecodedImage, Error, ErrorKind, PixelFormat, ResourceFormat, ResourceHeader, ResourceKind,
    Result, StorageFormat, Warning,
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
                if payload.len() > options.limits.max_decoded_bytes {
                    return Err(Error::new(
                        ErrorKind::LimitExceeded,
                        "PIXEL payload exceeds configured decoded-byte limit",
                    ));
                }
                let candidate_bpp: &[usize] = if header.format().has_alpha() {
                    &[3, 4]
                } else {
                    &[2, 3]
                };
                let exact = candidate_bpp.iter().find_map(|&bpp| {
                    let pixel_bytes = pixel_count.checked_mul(bpp)?;
                    (payload.len() == pixel_bytes + 4).then_some((bpp, pixel_bytes))
                });
                let selected = exact.or_else(|| {
                    (options.mode == DecodeMode::Diagnostic).then(|| {
                        candidate_bpp.iter().find_map(|&bpp| {
                            let pixel_bytes = pixel_count.checked_mul(bpp)?;
                            (payload.len() >= pixel_bytes).then_some((bpp, pixel_bytes))
                        })
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
                let storage = StorageFormat::from_alpha_and_bpp(header.format().has_alpha(), bpp)?;
                let mut warnings = Vec::new();
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
            ResourceKind::Ezip => Err(Error::new(
                ErrorKind::UnsupportedFormat,
                "compressed eZIP decoding is not available yet",
            )),
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

    pub fn decode_frame(&self, index: usize, output: PixelFormat) -> Result<DecodedImage> {
        if index != 0 {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("frame index {index} is outside a one-frame resource"),
            )
            .in_frame(index));
        }
        let Payload::Pixel(payload) = self.payload;
        let pixels = decode_storage_pixels(
            payload,
            self.info.width,
            self.info.height,
            self.info.storage_format,
            output,
        )?;
        Ok(DecodedImage::new(
            self.info.width,
            self.info.height,
            output,
            pixels,
        ))
    }
}
