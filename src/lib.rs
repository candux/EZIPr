#![forbid(unsafe_code)]
//! Decode and encode SiFli eZIP and PIXEL image resources.

mod decoder;
mod error;
mod header;
mod pixels;
mod stream;

pub use decoder::{DecodeLimits, DecodeMode, DecodeOptions, Decoder, ResourceInfo};
pub use error::{Error, ErrorKind, Result, Warning, WarningKind};
pub use header::{ResourceFormat, ResourceHeader, ResourceKind};
pub use pixels::{DecodedImage, ImageView, PixelFormat, StorageFormat};
pub use stream::StreamHeader;
