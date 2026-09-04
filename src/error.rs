use std::fmt;

/// Broad category for a decoding or encoding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    InvalidHeader,
    InvalidDimensions,
    InvalidOffset,
    TruncatedData,
    UnsupportedFormat,
    InvalidCompression,
    ChecksumMismatch,
    InvalidFilter,
    InvalidPixelLayout,
    InvalidAnimation,
    LimitExceeded,
    OutputBufferTooSmall,
    InvalidInput,
}

/// An EZIPr operation error with optional location context.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    kind: ErrorKind,
    message: String,
    byte_offset: Option<usize>,
    frame_index: Option<usize>,
    row_index: Option<u32>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            byte_offset: None,
            frame_index: None,
            row_index: None,
        }
    }

    pub fn at_offset(mut self, byte_offset: usize) -> Self {
        self.byte_offset = Some(byte_offset);
        self
    }

    pub fn in_frame(mut self, frame_index: usize) -> Self {
        self.frame_index = Some(frame_index);
        self
    }

    pub fn on_row(mut self, row_index: u32) -> Self {
        self.row_index = Some(row_index);
        self
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn byte_offset(&self) -> Option<usize> {
        self.byte_offset
    }

    pub fn frame_index(&self) -> Option<usize> {
        self.frame_index
    }

    pub fn row_index(&self) -> Option<u32> {
        self.row_index
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Broad category for a diagnostic recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WarningKind {
    TrailingData,
    PartialData,
    UnknownFilter,
    MetadataMismatch,
    MissingChecksum,
    ChecksumMismatch,
}

/// A non-fatal diagnostic recovery reported to the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Warning {
    kind: WarningKind,
    message: String,
    byte_offset: Option<usize>,
    frame_index: Option<usize>,
    row_index: Option<u32>,
}

impl Warning {
    pub fn new(kind: WarningKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            byte_offset: None,
            frame_index: None,
            row_index: None,
        }
    }

    pub fn at_offset(mut self, byte_offset: usize) -> Self {
        self.byte_offset = Some(byte_offset);
        self
    }

    pub fn in_frame(mut self, frame_index: usize) -> Self {
        self.frame_index = Some(frame_index);
        self
    }

    pub fn on_row(mut self, row_index: u32) -> Self {
        self.row_index = Some(row_index);
        self
    }

    pub fn kind(&self) -> WarningKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn byte_offset(&self) -> Option<usize> {
        self.byte_offset
    }

    pub fn frame_index(&self) -> Option<usize> {
        self.frame_index
    }

    pub fn row_index(&self) -> Option<u32> {
        self.row_index
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
