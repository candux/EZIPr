use crate::{Error, ErrorKind, Result};

/// Pixel layout used by caller-owned image buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}

/// Pixel layout stored inside a SiFli resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum StorageFormat {
    Rgb565,
    Rgb888,
    Argb565,
    Argb888,
}

impl StorageFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb565 => 2,
            Self::Rgb888 | Self::Argb565 => 3,
            Self::Argb888 => 4,
        }
    }

    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Argb565 | Self::Argb888)
    }

    pub(crate) fn from_alpha_and_bpp(has_alpha: bool, bpp: usize) -> Result<Self> {
        match (has_alpha, bpp) {
            (false, 2) => Ok(Self::Rgb565),
            (false, 3) => Ok(Self::Rgb888),
            (true, 3) => Ok(Self::Argb565),
            (true, 4) => Ok(Self::Argb888),
            _ => Err(Error::new(
                ErrorKind::InvalidPixelLayout,
                format!("no storage format for alpha={has_alpha}, {bpp} bytes per pixel"),
            )),
        }
    }
}

/// Borrowed raw image supplied to an encoder.
#[derive(Clone, Copy, Debug)]
pub struct ImageView<'a> {
    width: u32,
    height: u32,
    format: PixelFormat,
    stride: usize,
    pixels: &'a [u8],
}

impl<'a> ImageView<'a> {
    pub fn new(
        width: u32,
        height: u32,
        format: PixelFormat,
        stride: usize,
        pixels: &'a [u8],
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                "image dimensions must be positive",
            ));
        }
        let row_bytes = (width as usize)
            .checked_mul(format.bytes_per_pixel())
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "image row size overflow"))?;
        if stride < row_bytes {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("stride {stride} is shorter than the {row_bytes}-byte row"),
            ));
        }
        let required = stride
            .checked_mul(height.saturating_sub(1) as usize)
            .and_then(|prefix| prefix.checked_add(row_bytes))
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "image buffer size overflow"))?;
        if pixels.len() < required {
            return Err(Error::new(
                ErrorKind::TruncatedData,
                format!(
                    "image buffer has {} bytes; {required} required",
                    pixels.len()
                ),
            ));
        }
        Ok(Self {
            width,
            height,
            format,
            stride,
            pixels,
        })
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub const fn format(self) -> PixelFormat {
        self.format
    }

    pub const fn stride(self) -> usize {
        self.stride
    }

    pub const fn pixels(self) -> &'a [u8] {
        self.pixels
    }
}

/// Owned decoded pixels with tightly packed rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImage {
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
}

impl DecodedImage {
    pub(crate) fn new(width: u32, height: u32, format: PixelFormat, pixels: Vec<u8>) -> Self {
        Self {
            width,
            height,
            format,
            pixels,
        }
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn stride(&self) -> usize {
        self.width as usize * self.format.bytes_per_pixel()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}

pub(crate) fn decode_storage_pixels(
    input: &[u8],
    width: u32,
    height: u32,
    storage: StorageFormat,
    output: PixelFormat,
) -> Result<Vec<u8>> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "pixel count overflow"))?;
    let expected = pixel_count
        .checked_mul(storage.bytes_per_pixel())
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "stored pixel size overflow"))?;
    if input.len() != expected {
        return Err(Error::new(
            ErrorKind::InvalidPixelLayout,
            format!("pixel data has {} bytes; expected {expected}", input.len()),
        ));
    }

    let mut decoded = Vec::with_capacity(pixel_count * output.bytes_per_pixel());
    for pixel in input.chunks_exact(storage.bytes_per_pixel()) {
        let (red, green, blue, alpha) = match storage {
            StorageFormat::Rgb565 | StorageFormat::Argb565 => {
                let packed = u16::from_le_bytes([pixel[0], pixel[1]]);
                let red = (((packed >> 11) & 0x1f) as u32 * 255 / 31) as u8;
                let green = (((packed >> 5) & 0x3f) as u32 * 255 / 63) as u8;
                let blue = ((packed & 0x1f) as u32 * 255 / 31) as u8;
                let alpha = if storage == StorageFormat::Argb565 {
                    pixel[2]
                } else {
                    255
                };
                (red, green, blue, alpha)
            }
            StorageFormat::Rgb888 => (pixel[2], pixel[1], pixel[0], 255),
            StorageFormat::Argb888 => (pixel[2], pixel[1], pixel[0], pixel[3]),
        };
        decoded.extend_from_slice(&[red, green, blue]);
        if output == PixelFormat::Rgba8 {
            decoded.push(alpha);
        }
    }
    Ok(decoded)
}
