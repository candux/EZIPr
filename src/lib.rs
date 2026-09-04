#![forbid(unsafe_code)]
//! Decode and encode SiFli eZIP and PIXEL image resources.

mod animation;
mod decoder;
mod encoder;
mod error;
mod header;
mod pixels;
mod stream;

pub use animation::{BlendMode, Compositor, DisposalMethod, FrameInfo, Repeat};
pub use decoder::{DecodeLimits, DecodeMode, DecodeOptions, Decoder, ResourceInfo};
pub use encoder::{
    AlphaMode, ColorDepth, EncodeOptions, EncodedResource, Encoder, ResourceEncoding,
};
pub use error::{Error, ErrorKind, Result, Warning, WarningKind};
pub use header::{ResourceFormat, ResourceHeader, ResourceKind};
pub use pixels::{DecodedImage, ImageView, PixelFormat, StorageFormat};
pub use stream::StreamHeader;
