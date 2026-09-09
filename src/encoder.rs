use crate::stream::paeth;
use crate::{
    Error, ErrorKind, ImageView, PixelFormat, ResourceFormat, ResourceHeader, Result, StorageFormat,
};

/// Base color precision requested from the encoder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ColorDepth {
    #[default]
    Rgb565,
    Rgb888,
}

/// Policy for retaining or discarding an input alpha channel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum AlphaMode {
    /// Retain alpha only when the input contains a non-opaque pixel.
    #[default]
    Auto,
    /// Always emit an alpha-capable storage format.
    Preserve,
    /// Always emit an opaque storage format.
    Discard,
}

/// Static resource representation written by the encoder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceEncoding {
    #[default]
    Ezip,
    Pixel,
}

/// Color dithering applied while reducing eight-bit RGB channels to RGB565.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum Rgb565Dithering {
    /// Discard the low channel bits without modifying the input colors.
    None,
    /// Apply quantization-aware 8x8 ordered dithering with stable reconstruction levels.
    #[default]
    Balanced8x8,
    /// Reproduce the reference encoder's component-specific 8x8 dither.
    Reference8x8,
}

/// Strategy used to select the filtered representation and DEFLATE stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum CompressionStrategy {
    /// Compress the requested row-filter plan once at the configured level.
    #[default]
    Fast,
    /// Search deterministic filter plans and compressors for the smallest result.
    ///
    /// Encoding with this strategy requires the `smallest` Cargo feature.
    Smallest,
}

/// Options controlling static resource encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodeOptions {
    color_depth: ColorDepth,
    alpha_mode: AlphaMode,
    resource_encoding: ResourceEncoding,
    rgb565_dithering: Rgb565Dithering,
    block_rows: u8,
    row_filters: bool,
    compression_level: u8,
    history_limit: u16,
    compression_strategy: CompressionStrategy,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            color_depth: ColorDepth::Rgb565,
            alpha_mode: AlphaMode::Auto,
            resource_encoding: ResourceEncoding::Ezip,
            rgb565_dithering: Rgb565Dithering::Balanced8x8,
            block_rows: 32,
            row_filters: true,
            compression_level: 6,
            history_limit: 8192,
            compression_strategy: CompressionStrategy::Fast,
        }
    }
}

impl EncodeOptions {
    pub fn new(color_depth: ColorDepth) -> Self {
        Self {
            color_depth,
            ..Self::default()
        }
    }

    pub fn alpha_mode(mut self, alpha_mode: AlphaMode) -> Self {
        self.alpha_mode = alpha_mode;
        self
    }

    pub fn resource_encoding(mut self, resource_encoding: ResourceEncoding) -> Self {
        self.resource_encoding = resource_encoding;
        self
    }

    pub fn dithering(mut self, dithering: Rgb565Dithering) -> Self {
        self.rgb565_dithering = dithering;
        self
    }

    pub fn block_rows(mut self, block_rows: u8) -> Result<Self> {
        if block_rows == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "eZIP block-row count must be positive",
            ));
        }
        self.block_rows = block_rows;
        Ok(self)
    }

    pub fn row_filters(mut self, enabled: bool) -> Self {
        self.row_filters = enabled;
        self
    }

    /// Bound the DEFLATE sliding window. Defaults to 8 KiB for hardware
    /// compatibility. Larger windows require a target verified to support them.
    pub fn history_limit(mut self, bytes: u16) -> Result<Self> {
        if !(512..=32768).contains(&bytes) || !bytes.is_power_of_two() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "history limit must be a power of two from 512 through 32768 bytes",
            ));
        }
        self.history_limit = bytes;
        Ok(self)
    }

    /// Select compression effort from 0 through 10. Bounded histories use
    /// levels 0 through 9; level 10 is equivalent to 9 for those streams.
    pub fn compression_level(mut self, level: u8) -> Result<Self> {
        if level > 10 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("compression level {level} is outside 0..=10"),
            ));
        }
        self.compression_level = level;
        Ok(self)
    }

    pub fn compression_strategy(mut self, strategy: CompressionStrategy) -> Self {
        self.compression_strategy = strategy;
        self
    }

    pub const fn color_depth(self) -> ColorDepth {
        self.color_depth
    }

    pub const fn alpha_policy(self) -> AlphaMode {
        self.alpha_mode
    }

    pub const fn encoding(self) -> ResourceEncoding {
        self.resource_encoding
    }

    pub const fn rgb565_dithering(self) -> Rgb565Dithering {
        self.rgb565_dithering
    }

    pub const fn rows_per_block(self) -> u8 {
        self.block_rows
    }

    pub const fn uses_row_filters(self) -> bool {
        self.row_filters
    }

    pub const fn level(self) -> u8 {
        self.compression_level
    }

    pub const fn strategy(self) -> CompressionStrategy {
        self.compression_strategy
    }
}

/// Encoded bytes and the storage format selected for them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedResource {
    bytes: Vec<u8>,
    storage_format: StorageFormat,
    resource_format: ResourceFormat,
}

impl EncodedResource {
    pub(crate) fn new(
        bytes: Vec<u8>,
        storage_format: StorageFormat,
        resource_format: ResourceFormat,
    ) -> Self {
        Self {
            bytes,
            storage_format,
            resource_format,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub const fn storage_format(&self) -> StorageFormat {
        self.storage_format
    }

    pub const fn resource_format(&self) -> ResourceFormat {
        self.resource_format
    }
}

/// Reusable static resource encoder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Encoder {
    options: EncodeOptions,
}

impl Encoder {
    pub fn new(options: EncodeOptions) -> Self {
        Self { options }
    }

    pub const fn options(&self) -> EncodeOptions {
        self.options
    }

    pub fn encode(&self, image: ImageView<'_>) -> Result<EncodedResource> {
        if self.options.resource_encoding == ResourceEncoding::Pixel
            && self.options.compression_strategy == CompressionStrategy::Smallest
        {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "smallest-output compression is not applicable to PIXEL resources",
            ));
        }
        validate_compression_strategy(self.options)?;
        let width = u16::try_from(image.width()).map_err(|_| {
            Error::new(
                ErrorKind::InvalidDimensions,
                format!(
                    "image width {} does not fit the resource header",
                    image.width()
                ),
            )
        })?;
        let height = u16::try_from(image.height()).map_err(|_| {
            Error::new(
                ErrorKind::InvalidDimensions,
                format!(
                    "image height {} does not fit the resource header",
                    image.height()
                ),
            )
        })?;
        let has_alpha = resolve_alpha(image, self.options.alpha_mode);
        let storage_format = resolve_storage(self.options.color_depth, has_alpha);
        let resource_format =
            resolve_resource_format(self.options.resource_encoding, storage_format);
        let header = ResourceHeader::new(resource_format, width, height)?;
        let stored_pixels =
            encode_storage_pixels(image, storage_format, self.options.rgb565_dithering, 0, 0)?;
        let mut bytes = header.to_bytes().to_vec();
        match self.options.resource_encoding {
            ResourceEncoding::Pixel => {
                bytes.extend_from_slice(&stored_pixels);
                bytes.extend_from_slice(&crc32fast::hash(&stored_pixels).to_le_bytes());
            }
            ResourceEncoding::Ezip => {
                let result = compress_pixels_search(
                    &stored_pixels,
                    width as usize,
                    height as usize,
                    storage_format.bytes_per_pixel(),
                    self.options,
                    true,
                );
                let stream_size = crate::StreamHeader::BYTE_LEN
                    .checked_add(result.compressed.len())
                    .and_then(|size| size.checked_add(crate::StreamHeader::CHECKSUM_LEN))
                    .and_then(|size| u32::try_from(size).ok())
                    .ok_or_else(|| {
                        Error::new(ErrorKind::LimitExceeded, "encoded eZIP stream is too large")
                    })?;
                bytes.extend_from_slice(&stream_size.to_be_bytes());
                bytes.push(0x10 | color_type(storage_format));
                bytes.push(storage_bit_depth(storage_format));
                bytes.push(self.options.block_rows);
                bytes.push(0);
                bytes.extend_from_slice(&width.to_be_bytes());
                bytes.extend_from_slice(&height.to_be_bytes());
                bytes.push(u8::from(!result.has_row_filters));
                bytes.extend_from_slice(&[0, 0, 0]);
                bytes.extend_from_slice(&result.compressed);
                let checksum =
                    miniz_oxide::mz_adler32_oxide(miniz_oxide::MZ_ADLER32_INIT, &result.filtered);
                bytes.extend_from_slice(&checksum.to_be_bytes());
            }
        }
        Ok(EncodedResource::new(bytes, storage_format, resource_format))
    }
}

pub(crate) struct CompressionResult {
    pub filtered: Vec<u8>,
    pub compressed: Vec<u8>,
    pub has_row_filters: bool,
}

pub(crate) fn compress_animation_pixels_search(
    pixels: &[u8],
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    options: EncodeOptions,
) -> CompressionResult {
    // eZIP-A has one filter-mode flag for every frame, so a frame must not
    // independently switch the whole animation to filterless storage.
    compress_pixels_search(pixels, width, height, bytes_per_pixel, options, false)
}

fn compress_bounded(input: &[u8], level: u8, history: u16) -> Vec<u8> {
    if history == 32768 {
        return miniz_oxide::deflate::compress_to_vec(input, level);
    }
    let mut compressor = flate2::Compress::new_with_window_bits(
        flate2::Compression::new(u32::from(level.min(9))),
        false,
        history.ilog2() as u8,
    );
    let mut output = Vec::new();
    loop {
        output.reserve(16384);
        let consumed = compressor.total_in() as usize;
        let status = compressor
            .compress_vec(
                &input[consumed..],
                &mut output,
                flate2::FlushCompress::Finish,
            )
            .expect("in-memory compression failed");
        if status == flate2::Status::StreamEnd {
            return output;
        }
    }
}

fn compress_pixels_search(
    pixels: &[u8],
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    options: EncodeOptions,
    allow_filterless_candidate: bool,
) -> CompressionResult {
    let baseline = if options.uses_row_filters() {
        (
            true,
            filter_rows(
                pixels,
                width,
                height,
                bytes_per_pixel,
                options.rows_per_block(),
            ),
        )
    } else {
        (false, pixels.to_vec())
    };
    if options.strategy() == CompressionStrategy::Fast {
        let compressed = compress_bounded(&baseline.1, options.level(), options.history_limit);
        return CompressionResult {
            filtered: baseline.1,
            compressed,
            has_row_filters: baseline.0,
        };
    }

    let mut best = CompressionResult {
        compressed: compress_bounded(&baseline.1, options.level(), options.history_limit),
        filtered: baseline.1.clone(),
        has_row_filters: baseline.0,
    };
    search_candidate(
        &mut best,
        baseline.0,
        baseline.1,
        Some(options.level()),
        options.history_limit,
    );
    if options.uses_row_filters() {
        for filter in 0..=4 {
            search_candidate(
                &mut best,
                true,
                filter_rows_fixed(
                    pixels,
                    width,
                    height,
                    bytes_per_pixel,
                    options.rows_per_block(),
                    filter,
                ),
                None,
                options.history_limit,
            );
        }
        if allow_filterless_candidate {
            search_candidate(
                &mut best,
                false,
                pixels.to_vec(),
                None,
                options.history_limit,
            );
        }
    }
    best
}

#[cfg(feature = "smallest")]
pub(crate) fn validate_compression_strategy(_options: EncodeOptions) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "smallest"))]
pub(crate) fn validate_compression_strategy(options: EncodeOptions) -> Result<()> {
    if options.strategy() == CompressionStrategy::Smallest {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "smallest-output compression requires the `smallest` Cargo feature",
        ));
    }
    Ok(())
}

fn search_candidate(
    best: &mut CompressionResult,
    has_row_filters: bool,
    filtered: Vec<u8>,
    seed_level: Option<u8>,
    history: u16,
) {
    // Retain only the winner and current candidate. Identical winning data has
    // already been searched, except on the initial baseline pass.
    if seed_level.is_none() && best.has_row_filters == has_row_filters && best.filtered == filtered
    {
        return;
    }
    let mut improved = false;
    for level in 0..=if history < 32768 { 9 } else { 10 } {
        if seed_level == Some(level) {
            continue;
        }
        let compressed = compress_bounded(&filtered, level, history);
        if compressed.len() < best.compressed.len() {
            best.compressed = compressed;
            improved = true;
        }
    }
    if improved {
        best.filtered = filtered;
        best.has_row_filters = has_row_filters;
    }
}

fn resolve_alpha(image: ImageView<'_>, mode: AlphaMode) -> bool {
    match mode {
        AlphaMode::Preserve => true,
        AlphaMode::Discard => false,
        AlphaMode::Auto => {
            image.format() == PixelFormat::Rgba8
                && (0..image.height() as usize).any(|row| {
                    image.pixels()
                        [row * image.stride()..row * image.stride() + image.width() as usize * 4]
                        .chunks_exact(4)
                        .any(|pixel| pixel[3] != 255)
                })
        }
    }
}

pub(crate) const fn resolve_storage(depth: ColorDepth, has_alpha: bool) -> StorageFormat {
    match (depth, has_alpha) {
        (ColorDepth::Rgb565, false) => StorageFormat::Rgb565,
        (ColorDepth::Rgb888, false) => StorageFormat::Rgb888,
        (ColorDepth::Rgb565, true) => StorageFormat::Argb565,
        (ColorDepth::Rgb888, true) => StorageFormat::Argb888,
    }
}

pub(crate) const fn resolve_resource_format(
    encoding: ResourceEncoding,
    storage: StorageFormat,
) -> ResourceFormat {
    match (encoding, storage) {
        (ResourceEncoding::Ezip, StorageFormat::Argb565) => ResourceFormat::EzipArgb565,
        (ResourceEncoding::Ezip, _) => ResourceFormat::Ezip,
        (ResourceEncoding::Pixel, StorageFormat::Rgb565 | StorageFormat::Rgb888) => {
            ResourceFormat::Pixel
        }
        (ResourceEncoding::Pixel, StorageFormat::Argb565 | StorageFormat::Argb888) => {
            ResourceFormat::PixelWithAlpha
        }
    }
}

pub(crate) fn encode_storage_pixels(
    image: ImageView<'_>,
    storage: StorageFormat,
    dithering: Rgb565Dithering,
    x_offset: u32,
    y_offset: u32,
) -> Result<Vec<u8>> {
    let pixel_count = (image.width() as usize)
        .checked_mul(image.height() as usize)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "pixel count overflow"))?;
    let capacity = pixel_count
        .checked_mul(storage.bytes_per_pixel())
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "stored image size overflow"))?;
    let mut output = Vec::with_capacity(capacity);
    let input_bpp = image.format().bytes_per_pixel();
    for row in 0..image.height() as usize {
        let row_start = row * image.stride();
        let row_end = row_start + image.width() as usize * input_bpp;
        for (column, pixel) in image.pixels()[row_start..row_end]
            .chunks_exact(input_bpp)
            .enumerate()
        {
            let red = pixel[0];
            let green = pixel[1];
            let blue = pixel[2];
            let alpha = if input_bpp == 4 { pixel[3] } else { 255 };
            match storage {
                StorageFormat::Rgb565 | StorageFormat::Argb565 => {
                    let x = ((x_offset & 7) as usize + column) & 7;
                    let y = ((y_offset & 7) as usize + row) & 7;
                    let packed = pack_rgb565(red, green, blue, dithering, y * 8 + x);
                    output.extend_from_slice(&packed.to_le_bytes());
                    if storage == StorageFormat::Argb565 {
                        output.push(alpha);
                    }
                }
                StorageFormat::Rgb888 => output.extend_from_slice(&[blue, green, red]),
                StorageFormat::Argb888 => output.extend_from_slice(&[blue, green, red, alpha]),
            }
        }
    }
    Ok(output)
}

const BAYER_8X8: [u8; 64] = [
    0, 48, 12, 60, 3, 51, 15, 63, 32, 16, 44, 28, 35, 19, 47, 31, 8, 56, 4, 52, 11, 59, 7, 55, 40,
    24, 36, 20, 43, 27, 39, 23, 2, 50, 14, 62, 1, 49, 13, 61, 34, 18, 46, 30, 33, 17, 45, 29, 10,
    58, 6, 54, 9, 57, 5, 53, 42, 26, 38, 22, 41, 25, 37, 21,
];

const REFERENCE_DITHER_RED: [u8; 64] = [
    1, 7, 3, 5, 0, 8, 2, 6, 7, 1, 5, 3, 8, 0, 6, 2, 3, 5, 0, 8, 2, 6, 1, 7, 5, 3, 8, 0, 6, 2, 7, 1,
    0, 8, 2, 6, 1, 7, 3, 5, 8, 0, 6, 2, 7, 1, 5, 3, 2, 6, 1, 7, 3, 5, 0, 8, 6, 2, 7, 1, 5, 3, 8, 0,
];

const REFERENCE_DITHER_GREEN: [u8; 64] = [
    1, 3, 2, 2, 3, 1, 2, 2, 2, 2, 0, 4, 2, 2, 4, 0, 3, 1, 2, 2, 1, 3, 2, 2, 2, 2, 4, 0, 2, 2, 0, 4,
    1, 3, 2, 2, 3, 1, 2, 2, 2, 2, 0, 4, 2, 2, 4, 0, 3, 1, 2, 2, 1, 3, 2, 2, 2, 2, 4, 0, 2, 2, 0, 4,
];

const REFERENCE_DITHER_BLUE: [u8; 64] = [
    5, 3, 8, 0, 6, 2, 7, 1, 3, 5, 0, 8, 2, 6, 1, 7, 8, 0, 6, 2, 7, 1, 5, 3, 0, 8, 2, 6, 1, 7, 3, 5,
    6, 2, 7, 1, 5, 3, 8, 0, 2, 6, 1, 7, 3, 5, 0, 8, 7, 1, 5, 3, 8, 0, 6, 2, 1, 7, 3, 5, 0, 8, 2, 6,
];

fn pack_rgb565(
    red: u8,
    green: u8,
    blue: u8,
    dithering: Rgb565Dithering,
    dither_index: usize,
) -> u16 {
    match dithering {
        Rgb565Dithering::None => {
            (u16::from(red >> 3) << 11) | (u16::from(green >> 2) << 5) | u16::from(blue >> 3)
        }
        Rgb565Dithering::Balanced8x8 => {
            let threshold = BAYER_8X8[dither_index];
            let red = quantize_balanced(red, 31, threshold);
            let green = quantize_balanced(green, 63, threshold);
            let blue = quantize_balanced(blue, 31, threshold);
            (red << 11) | (green << 5) | blue
        }
        Rgb565Dithering::Reference8x8 => {
            let red = red.saturating_add(REFERENCE_DITHER_RED[dither_index]);
            let green = green.saturating_add(REFERENCE_DITHER_GREEN[dither_index]);
            let blue = blue.saturating_add(REFERENCE_DITHER_BLUE[dither_index]);
            (u16::from(red >> 3) << 11) | (u16::from(green >> 2) << 5) | u16::from(blue >> 3)
        }
    }
}

fn quantize_balanced(value: u8, maximum: u16, threshold: u8) -> u16 {
    if let Some(index) = reconstruction_index(value, maximum) {
        return index;
    }
    let numerator =
        u32::from(value) * u32::from(maximum) + u32::from(threshold) * u32::from(u8::MAX) / 64;
    (numerator / u32::from(u8::MAX)) as u16
}

fn reconstruction_index(value: u8, maximum: u16) -> Option<u16> {
    let scaled = u32::from(value) * u32::from(maximum);
    let candidate = scaled.div_ceil(u32::from(u8::MAX)) as u16;
    (candidate <= maximum && candidate * u16::from(u8::MAX) / maximum == u16::from(value))
        .then_some(candidate)
}

pub(crate) const fn color_type(storage: StorageFormat) -> u8 {
    match storage {
        StorageFormat::Rgb888 => 2,
        StorageFormat::Argb888 => 6,
        StorageFormat::Rgb565 => 8,
        StorageFormat::Argb565 => 12,
    }
}

pub(crate) const fn storage_bit_depth(storage: StorageFormat) -> u8 {
    match storage {
        StorageFormat::Rgb565 => 16,
        StorageFormat::Argb565 => 24,
        StorageFormat::Rgb888 | StorageFormat::Argb888 => 8,
    }
}

pub(crate) fn filter_rows(
    pixels: &[u8],
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    block_rows: u8,
) -> Vec<u8> {
    let stride = width * bytes_per_pixel;
    let mut output = Vec::with_capacity(pixels.len() + height);
    for row in 0..height {
        let current = &pixels[row * stride..(row + 1) * stride];
        let previous = if row % block_rows as usize == 0 {
            None
        } else {
            Some(&pixels[(row - 1) * stride..row * stride])
        };
        let mut best_filter = 0;
        let mut best = filter_candidate(current, previous, bytes_per_pixel, 0);
        let mut best_score = filter_score(&best);
        for filter in 1..=4 {
            let candidate = filter_candidate(current, previous, bytes_per_pixel, filter);
            let score = filter_score(&candidate);
            if score < best_score {
                best_filter = filter;
                best = candidate;
                best_score = score;
            }
        }
        output.push(best_filter);
        output.extend_from_slice(&best);
    }
    output
}

fn filter_rows_fixed(
    pixels: &[u8],
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    block_rows: u8,
    filter: u8,
) -> Vec<u8> {
    let stride = width * bytes_per_pixel;
    let mut output = Vec::with_capacity(pixels.len() + height);
    for row in 0..height {
        let current = &pixels[row * stride..(row + 1) * stride];
        let previous = if row % block_rows as usize == 0 {
            None
        } else {
            Some(&pixels[(row - 1) * stride..row * stride])
        };
        output.push(filter);
        output.extend_from_slice(&filter_candidate(
            current,
            previous,
            bytes_per_pixel,
            filter,
        ));
    }
    output
}

fn filter_candidate(
    current: &[u8],
    previous: Option<&[u8]>,
    bytes_per_pixel: usize,
    filter: u8,
) -> Vec<u8> {
    current
        .iter()
        .enumerate()
        .map(|(index, &value)| {
            let left = if index >= bytes_per_pixel {
                current[index - bytes_per_pixel]
            } else {
                0
            };
            let up = previous.map_or(0, |row| row[index]);
            let up_left = if index >= bytes_per_pixel {
                previous.map_or(0, |row| row[index - bytes_per_pixel])
            } else {
                0
            };
            let prediction = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => paeth(left, up, up_left),
                _ => unreachable!("encoder only evaluates known filters"),
            };
            value.wrapping_sub(prediction)
        })
        .collect()
}

fn filter_score(row: &[u8]) -> u64 {
    row.iter()
        .map(|&value| u64::from(value.min(value.wrapping_neg())))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::quantize_balanced;

    fn error_statistics(maximum: u16) -> (i64, u64, i32) {
        let mut signed = 0;
        let mut absolute = 0;
        let mut worst = 0;
        for value in 0..=u8::MAX {
            for threshold in 0..64 {
                let quantized = quantize_balanced(value, maximum, threshold);
                let reconstructed = i32::from(quantized) * 255 / i32::from(maximum);
                let error = reconstructed - i32::from(value);
                signed += i64::from(error);
                absolute += u64::from(error.unsigned_abs());
                worst = worst.max(error.abs());
            }
        }
        (signed, absolute, worst)
    }

    #[test]
    fn balanced_quantization_has_bounded_near_zero_error() {
        assert_eq!(error_statistics(31), (-7_854, 44_162, 8));
        assert_eq!(error_statistics(63), (-6_240, 20_700, 4));
    }

    #[test]
    fn balanced_quantization_preserves_every_reconstruction_level() {
        for maximum in [31, 63] {
            for index in 0..=maximum {
                let value = (index * u16::from(u8::MAX) / maximum) as u8;
                for threshold in 0..64 {
                    assert_eq!(quantize_balanced(value, maximum, threshold), index);
                }
            }
        }
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use miniz_oxide::inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress},
    };

    fn decode_with_8k_history(input: &[u8]) -> std::result::Result<Vec<u8>, TINFLStatus> {
        let mut decoder = DecompressorOxide::new();
        let mut ring = [0u8; 8192];
        let mut consumed = 0;
        let mut output = Vec::new();
        loop {
            let (status, read, written) =
                decompress(&mut decoder, &input[consumed..], &mut ring, 0, 0);
            consumed += read;
            output.extend_from_slice(&ring[..written]);
            if status == TINFLStatus::Done {
                return Ok(output);
            }
            if status != TINFLStatus::HasMoreOutput {
                return Err(status);
            }
            assert_eq!(written, ring.len());
        }
    }

    #[test]
    fn bounded_stream_decodes_with_hardware_sized_history() {
        let mut seed = 7u32;
        let pattern: Vec<u8> = (0..10000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let input = pattern.repeat(4);
        // This fixture actually exercises references beyond the chip's history.
        assert_ne!(
            decode_with_8k_history(&compress_bounded(&input, 6, 32768)),
            Ok(input.clone())
        );
        assert_eq!(
            decode_with_8k_history(&compress_bounded(&input, 6, 8192)).unwrap(),
            input
        );
        assert_eq!(
            decode_with_8k_history(&compress_bounded(&input, 6, 1024)).unwrap(),
            input
        );
    }

    #[test]
    fn validates_history_and_smallest_feature() {
        assert_eq!(EncodeOptions::default().history_limit, 8192);
        assert!(EncodeOptions::default().history_limit(0).is_err());
        assert!(EncodeOptions::default().history_limit(32769).is_err());
        let options = EncodeOptions::default()
            .history_limit(8192)
            .unwrap()
            .compression_strategy(CompressionStrategy::Smallest);
        assert_eq!(
            validate_compression_strategy(options).is_ok(),
            cfg!(feature = "smallest")
        );
    }

    #[test]
    fn default_static_and_animation_encoders_keep_history_bounded() {
        let mut seed = 7u32;
        let pattern: Vec<u8> = (0..10000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let pixels = pattern.repeat(6);
        let image = ImageView::new(250, 80, PixelFormat::Rgb8, 750, &pixels).unwrap();
        let strategies = [CompressionStrategy::Fast, CompressionStrategy::Smallest];
        for strategy in strategies {
            if strategy == CompressionStrategy::Smallest && !cfg!(feature = "smallest") {
                continue;
            }
            for filters in [false, true] {
                for level in [0, 6, 10] {
                    let options = EncodeOptions::new(ColorDepth::Rgb888)
                        .row_filters(filters)
                        .compression_level(level)
                        .unwrap()
                        .compression_strategy(strategy);
                    let encoded = Encoder::new(options).encode(image).unwrap();
                    let bytes = encoded.as_bytes();
                    let raw = &bytes[20..bytes.len() - 4];
                    assert_eq!(
                        decode_with_8k_history(raw).unwrap(),
                        miniz_oxide::inflate::decompress_to_vec(raw).unwrap()
                    );
                    assert_eq!(
                        crate::Decoder::new(bytes)
                            .unwrap()
                            .decode_frame(0, PixelFormat::Rgb8)
                            .unwrap()
                            .pixels(),
                        pixels
                    );
                    let fast =
                        Encoder::new(options.compression_strategy(CompressionStrategy::Fast))
                            .encode(image)
                            .unwrap();
                    assert!(bytes.len() <= fast.as_bytes().len());
                }
                let options = EncodeOptions::new(ColorDepth::Rgb888)
                    .row_filters(filters)
                    .compression_strategy(strategy);
                let mut animation =
                    crate::AnimationEncoder::new(250, 80, crate::Repeat::Infinite, options)
                        .unwrap();
                for _ in 0..2 {
                    animation
                        .push_frame(crate::FrameView::new(image, 0, 0, 1, 10))
                        .unwrap();
                }
                let animation = animation.finish().unwrap();
                let bytes = animation.as_bytes();
                for index in 0..2 {
                    let start = u32::from_be_bytes(
                        bytes[28 + index * 4..32 + index * 4].try_into().unwrap(),
                    ) as usize
                        + 4;
                    let len = u32::from_be_bytes(bytes[start + 26..start + 30].try_into().unwrap())
                        as usize;
                    let raw = &bytes[start + 30..start + 30 + len];
                    assert_eq!(
                        decode_with_8k_history(raw).unwrap(),
                        miniz_oxide::inflate::decompress_to_vec(raw).unwrap()
                    );
                }
            }
        }
    }
}
