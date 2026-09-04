use crate::{Error, ErrorKind, Result};

const FORMAT_MASK: u32 = 0x1f;
const DIMENSION_MASK: u32 = 0x7ff;

/// High-level resource payload kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceKind {
    Ezip,
    Pixel,
    Animation,
}

/// Format identifier stored in the low five resource-header bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceFormat {
    Ezip = 1,
    EzipWithAlpha = 2,
    Pixel = 4,
    PixelWithAlpha = 5,
}

impl ResourceFormat {
    pub fn from_id(id: u8) -> Result<Self> {
        match id {
            1 => Ok(Self::Ezip),
            2 => Ok(Self::EzipWithAlpha),
            4 => Ok(Self::Pixel),
            5 => Ok(Self::PixelWithAlpha),
            _ => Err(Error::new(
                ErrorKind::UnsupportedFormat,
                format!("unsupported resource format {id}"),
            )),
        }
    }

    pub const fn id(self) -> u8 {
        self as u8
    }

    pub const fn kind(self) -> ResourceKind {
        match self {
            Self::Ezip | Self::EzipWithAlpha => ResourceKind::Ezip,
            Self::Pixel | Self::PixelWithAlpha => ResourceKind::Pixel,
        }
    }

    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::EzipWithAlpha | Self::PixelWithAlpha)
    }
}

/// Parsed four-byte image resource header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceHeader {
    format: ResourceFormat,
    width: u16,
    height: u16,
    reserved: u8,
}

impl ResourceHeader {
    pub const BYTE_LEN: usize = 4;
    pub const MAX_DIMENSION: u16 = 0x7ff;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let bytes: [u8; 4] = data
            .get(..4)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::TruncatedData,
                    "input is shorter than a resource header",
                )
            })?
            .try_into()
            .expect("slice length was checked");
        let value = u32::from_le_bytes(bytes);
        let format = ResourceFormat::from_id((value & FORMAT_MASK) as u8)?;
        let width = ((value >> 10) & DIMENSION_MASK) as u16;
        let height = ((value >> 21) & DIMENSION_MASK) as u16;
        if width == 0 || height == 0 {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                format!("resource dimensions must be positive, got {width}x{height}"),
            ));
        }
        Ok(Self {
            format,
            width,
            height,
            reserved: ((value >> 5) & 0x1f) as u8,
        })
    }

    pub fn new(format: ResourceFormat, width: u16, height: u16) -> Result<Self> {
        if width == 0 || height == 0 || width > Self::MAX_DIMENSION || height > Self::MAX_DIMENSION
        {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                format!("resource dimensions {width}x{height} are outside 1..=2047"),
            ));
        }
        Ok(Self {
            format,
            width,
            height,
            reserved: 0,
        })
    }

    pub const fn format(self) -> ResourceFormat {
        self.format
    }

    pub const fn width(self) -> u16 {
        self.width
    }

    pub const fn height(self) -> u16 {
        self.height
    }

    pub const fn reserved(self) -> u8 {
        self.reserved
    }

    pub const fn to_bytes(self) -> [u8; 4] {
        let value = (self.format.id() as u32)
            | ((self.reserved as u32) << 5)
            | ((self.width as u32) << 10)
            | ((self.height as u32) << 21);
        value.to_le_bytes()
    }
}
