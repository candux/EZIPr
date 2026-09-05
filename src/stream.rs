use std::collections::HashMap;

use crate::{DecodeMode, Error, ErrorKind, Result, StorageFormat, Warning, WarningKind};

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Common header preceding static and animated eZIP stream data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamHeader {
    data_size: u32,
    control: u8,
    bit_depth: u8,
    block_rows: u8,
    flags: u8,
    width: u16,
    height: u16,
    filter_mode: u8,
    palette_count: u8,
}

impl StreamHeader {
    pub const BYTE_LEN: usize = 16;
    pub const CHECKSUM_LEN: usize = 4;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let header = data.get(..Self::BYTE_LEN).ok_or_else(|| {
            Error::new(
                ErrorKind::TruncatedData,
                "input is shorter than an eZIP stream header",
            )
        })?;
        let data_size = u32::from_be_bytes(header[0..4].try_into().expect("fixed-size field"));
        let width = u16::from_be_bytes(header[8..10].try_into().expect("fixed-size field"));
        let height = u16::from_be_bytes(header[10..12].try_into().expect("fixed-size field"));
        if width == 0 || height == 0 {
            return Err(Error::new(
                ErrorKind::InvalidDimensions,
                format!("eZIP stream dimensions must be positive, got {width}x{height}"),
            ));
        }
        Ok(Self {
            data_size,
            control: header[4],
            bit_depth: header[5],
            block_rows: header[6],
            flags: header[7],
            width,
            height,
            filter_mode: header[12] & 0x0f,
            palette_count: header[13],
        })
    }

    pub const fn data_size(self) -> u32 {
        self.data_size
    }

    pub const fn control(self) -> u8 {
        self.control
    }

    pub const fn bit_depth(self) -> u8 {
        self.bit_depth
    }

    pub const fn block_rows(self) -> u8 {
        self.block_rows
    }

    pub const fn flags(self) -> u8 {
        self.flags
    }

    pub const fn width(self) -> u16 {
        self.width
    }

    pub const fn height(self) -> u16 {
        self.height
    }

    pub const fn filter_mode(self) -> u8 {
        self.filter_mode
    }

    pub const fn has_row_filters(self) -> bool {
        self.filter_mode != 1
    }

    pub const fn uses_shared_huffman(self) -> bool {
        self.control & 0xf0 == 0x40
    }

    pub const fn is_animation(self) -> bool {
        self.control & 0xf0 == 0x50
    }

    pub const fn palette_count(self) -> u8 {
        self.palette_count
    }

    pub fn storage_format(self) -> Result<StorageFormat> {
        match self.control & 0x0f {
            2 => Ok(StorageFormat::Rgb888),
            6 => Ok(StorageFormat::Argb888),
            8 => Ok(StorageFormat::Rgb565),
            12 => Ok(StorageFormat::Argb565),
            color_type => Err(Error::new(
                ErrorKind::UnsupportedFormat,
                format!("unsupported eZIP color type {color_type}"),
            )),
        }
    }
}

pub(crate) struct Inflated {
    pub bytes: Vec<u8>,
    pub block_rows: u8,
}

pub(crate) fn inflate_stream(
    stream: &[u8],
    header: StreamHeader,
    max_output: usize,
    mode: DecodeMode,
    warnings: &mut Vec<Warning>,
) -> Result<Inflated> {
    if header.uses_shared_huffman() {
        return inflate_shared(stream, header, max_output, mode, warnings);
    }
    let declared_size = header.data_size as usize;
    let minimum = StreamHeader::BYTE_LEN + StreamHeader::CHECKSUM_LEN;
    if declared_size < minimum {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            format!("eZIP stream size {declared_size} is shorter than {minimum} bytes"),
        ));
    }
    if declared_size > stream.len() {
        return Err(Error::new(
            ErrorKind::TruncatedData,
            format!(
                "eZIP stream declares {declared_size} bytes but only {} are available",
                stream.len()
            ),
        ));
    }
    if stream.len() > declared_size {
        let warning = Warning::new(
            WarningKind::TrailingData,
            format!(
                "ignored {} bytes after the declared eZIP stream",
                stream.len() - declared_size
            ),
        )
        .at_offset(declared_size);
        if mode == DecodeMode::Strict {
            return Err(
                Error::new(ErrorKind::InvalidHeader, warning.message().to_owned())
                    .at_offset(declared_size),
            );
        }
        warnings.push(warning);
    }
    let checksum_offset = declared_size - StreamHeader::CHECKSUM_LEN;
    let compressed = &stream[StreamHeader::BYTE_LEN..checksum_offset];
    let output = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, max_output)
        .map_err(|error| {
            let kind = if error.output.len() >= max_output {
                ErrorKind::LimitExceeded
            } else {
                ErrorKind::InvalidCompression
            };
            Error::new(kind, format!("invalid raw-DEFLATE eZIP stream: {error}"))
                .at_offset(StreamHeader::BYTE_LEN)
        })?;
    let stored = u32::from_be_bytes(
        stream[checksum_offset..declared_size]
            .try_into()
            .expect("checksum slice length was checked"),
    );
    let calculated = miniz_oxide::mz_adler32_oxide(miniz_oxide::MZ_ADLER32_INIT, &output);
    if stored != calculated {
        let message =
            format!("eZIP Adler-32 mismatch: stored {stored:08x}, calculated {calculated:08x}");
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::ChecksumMismatch, message).at_offset(checksum_offset));
        }
        warnings
            .push(Warning::new(WarningKind::ChecksumMismatch, message).at_offset(checksum_offset));
    }
    Ok(Inflated {
        bytes: output,
        block_rows: header.block_rows,
    })
}

fn inflate_shared(
    stream: &[u8],
    header: StreamHeader,
    max_output: usize,
    mode: DecodeMode,
    warnings: &mut Vec<Warning>,
) -> Result<Inflated> {
    let declared_size = header.data_size as usize;
    if declared_size < StreamHeader::BYTE_LEN + 8 {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            "shared-Huffman eZIP stream is too short",
        ));
    }
    if declared_size > stream.len() {
        return Err(Error::new(
            ErrorKind::TruncatedData,
            format!(
                "eZIP stream declares {declared_size} bytes but only {} are available",
                stream.len()
            ),
        ));
    }
    let trailer_end = declared_size.saturating_add(4);
    if trailer_end <= stream.len() {
        let stored = u32::from_le_bytes(
            stream[declared_size..trailer_end]
                .try_into()
                .expect("CRC slice length was checked"),
        );
        let calculated = crc32fast::hash(&stream[..declared_size]);
        if stored != calculated {
            let message = format!(
                "shared-Huffman CRC-32 mismatch: stored {stored:08x}, calculated {calculated:08x}"
            );
            if mode == DecodeMode::Strict {
                return Err(
                    Error::new(ErrorKind::ChecksumMismatch, message).at_offset(declared_size)
                );
            }
            warnings.push(
                Warning::new(WarningKind::ChecksumMismatch, message).at_offset(declared_size),
            );
        }
    } else if mode == DecodeMode::Strict {
        return Err(Error::new(
            ErrorKind::TruncatedData,
            "shared-Huffman eZIP stream has no CRC-32 trailer",
        )
        .at_offset(declared_size));
    } else {
        warnings.push(
            Warning::new(
                WarningKind::MissingChecksum,
                "shared-Huffman eZIP stream has no CRC-32 trailer",
            )
            .at_offset(declared_size),
        );
    }
    if stream.len() > trailer_end {
        let message = format!(
            "ignored {} bytes after the shared-Huffman CRC-32",
            stream.len() - trailer_end
        );
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::InvalidHeader, message).at_offset(trailer_end));
        }
        warnings.push(Warning::new(WarningKind::TrailingData, message).at_offset(trailer_end));
    }

    let container = &stream[StreamHeader::BYTE_LEN..declared_size];
    let block_rows = u16::from_be_bytes([container[0], container[1]]);
    let block_count = u16::from_be_bytes([container[2], container[3]]) as usize;
    if block_rows == 0 || block_rows > u16::from(u8::MAX) {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            format!("invalid shared-Huffman block-row count {block_rows}"),
        ));
    }
    if block_count == 0 {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            "shared-Huffman stream contains no blocks",
        ));
    }
    let table_size = block_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "block table size overflow"))?;
    if table_size > container.len() {
        return Err(Error::new(
            ErrorKind::TruncatedData,
            "shared-Huffman block table is truncated",
        ));
    }
    let raw_offset = StreamHeader::BYTE_LEN + table_size;
    let mut block_offsets = Vec::with_capacity(block_count);
    for index in 0..block_count {
        let entry_offset = 4 + index * 4;
        let offset = u32::from_be_bytes(
            container[entry_offset..entry_offset + 4]
                .try_into()
                .expect("block-offset entry length was checked"),
        ) as usize;
        if !offset.is_multiple_of(4) || offset < raw_offset || offset >= declared_size {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!("shared-Huffman block {index} has invalid offset {offset}"),
            )
            .at_offset(StreamHeader::BYTE_LEN + entry_offset));
        }
        if block_offsets
            .last()
            .is_some_and(|previous| *previous >= offset)
        {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                "shared-Huffman block offsets are not strictly increasing",
            )
            .at_offset(StreamHeader::BYTE_LEN + entry_offset));
        }
        block_offsets.push(offset);
    }
    let raw_stream = &container[table_size..];
    let bytes = decode_shared_huffman(raw_stream, max_output, &block_offsets, raw_offset)?;
    Ok(Inflated {
        bytes,
        block_rows: block_rows as u8,
    })
}

#[derive(Debug)]
struct BitReader<'a> {
    data: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            bit_position: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u8> {
        let byte = self.data.get(self.bit_position / 8).ok_or_else(|| {
            Error::new(
                ErrorKind::TruncatedData,
                "shared-Huffman bit stream ended unexpectedly",
            )
        })?;
        let bit = (byte >> (self.bit_position % 8)) & 1;
        self.bit_position += 1;
        Ok(bit)
    }

    fn read_bits(&mut self, count: u8) -> Result<usize> {
        let mut value = 0_usize;
        for shift in 0..count {
            value |= usize::from(self.read_bit()?) << shift;
        }
        Ok(value)
    }

    fn align_byte(&mut self) {
        self.bit_position = self.bit_position.saturating_add(7) & !7;
    }

    fn align_four_bytes(&mut self) {
        self.align_byte();
        let byte_position = (self.bit_position / 8).saturating_add(3) & !3;
        self.bit_position = byte_position.saturating_mul(8);
    }

    fn read_aligned_bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let start = self.bit_position / 8;
        let end = start
            .checked_add(count)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "bit-stream offset overflow"))?;
        let bytes = self.data.get(start..end).ok_or_else(|| {
            Error::new(
                ErrorKind::TruncatedData,
                "shared-Huffman stored block is truncated",
            )
        })?;
        self.bit_position = end * 8;
        Ok(bytes)
    }
}

#[derive(Debug)]
struct HuffmanTree {
    symbols: HashMap<(u8, u16), u16>,
    max_length: u8,
}

impl HuffmanTree {
    fn from_lengths(lengths: &[u8]) -> Result<Self> {
        let max_length = lengths.iter().copied().max().unwrap_or(0);
        if max_length == 0 || max_length > 15 {
            return Err(Error::new(
                ErrorKind::InvalidCompression,
                "invalid shared-Huffman code lengths",
            ));
        }
        let mut counts = vec![0_u32; max_length as usize + 1];
        for &length in lengths {
            if length > 0 {
                counts[length as usize] = counts[length as usize].saturating_add(1);
            }
        }
        let mut next_code = vec![0_u16; max_length as usize + 1];
        let mut code = 0_u32;
        for bits in 1..=max_length as usize {
            code = (code + counts[bits - 1]) << 1;
            if code + counts[bits] > 1_u32 << bits {
                return Err(Error::new(
                    ErrorKind::InvalidCompression,
                    "over-subscribed shared-Huffman code lengths",
                ));
            }
            next_code[bits] = code as u16;
        }
        let mut symbols = HashMap::new();
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let code = next_code[length as usize];
            symbols.insert((length, code), symbol as u16);
            next_code[length as usize] = code + 1;
        }
        Ok(Self {
            symbols,
            max_length,
        })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut code = 0_u16;
        for length in 1..=self.max_length {
            code = (code << 1) | u16::from(reader.read_bit()?);
            if let Some(&symbol) = self.symbols.get(&(length, code)) {
                return Ok(symbol);
            }
        }
        Err(Error::new(
            ErrorKind::InvalidCompression,
            "invalid shared-Huffman code",
        ))
    }
}

fn decode_shared_huffman(
    data: &[u8],
    max_output: usize,
    block_offsets: &[usize],
    raw_offset: usize,
) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(data);
    let literal_count = reader.read_bits(5)? + 257;
    let distance_count = reader.read_bits(5)? + 1;
    let code_length_count = reader.read_bits(4)? + 4;
    if literal_count > 288 || distance_count > 32 {
        return Err(Error::new(
            ErrorKind::InvalidCompression,
            "shared-Huffman table dimensions are invalid",
        ));
    }
    let mut code_lengths = [0_u8; 19];
    for &index in &CODE_LENGTH_ORDER[..code_length_count] {
        code_lengths[index] = reader.read_bits(3)? as u8;
    }
    let code_length_tree = HuffmanTree::from_lengths(&code_lengths)?;
    let total = literal_count + distance_count;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        match code_length_tree.decode(&mut reader)? {
            symbol @ 0..=15 => lengths.push(symbol as u8),
            16 => {
                let previous = *lengths.last().ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidCompression,
                        "repeat code appears before a Huffman length",
                    )
                })?;
                let repeat = reader.read_bits(2)? + 3;
                if lengths.len() + repeat > total {
                    return Err(Error::new(
                        ErrorKind::InvalidCompression,
                        "repeated Huffman lengths exceed the table",
                    ));
                }
                lengths.resize(lengths.len() + repeat, previous);
            }
            17 => {
                let repeat = reader.read_bits(3)? + 3;
                if lengths.len() + repeat > total {
                    return Err(Error::new(
                        ErrorKind::InvalidCompression,
                        "zero Huffman lengths exceed the table",
                    ));
                }
                lengths.resize(lengths.len() + repeat, 0);
            }
            18 => {
                let repeat = reader.read_bits(7)? + 11;
                if lengths.len() + repeat > total {
                    return Err(Error::new(
                        ErrorKind::InvalidCompression,
                        "zero Huffman lengths exceed the table",
                    ));
                }
                lengths.resize(lengths.len() + repeat, 0);
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidCompression,
                    "invalid code-length symbol",
                ));
            }
        }
    }
    let literal_tree = HuffmanTree::from_lengths(&lengths[..literal_count])?;
    let distance_tree = HuffmanTree::from_lengths(&lengths[literal_count..])?;
    reader.align_four_bytes();
    let mut output = Vec::new();
    for (block_index, &expected_offset) in block_offsets.iter().enumerate() {
        let actual_offset = raw_offset
            .checked_add(reader.bit_position / 8)
            .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "block offset overflow"))?;
        if actual_offset != expected_offset {
            return Err(Error::new(
                ErrorKind::InvalidOffset,
                format!(
                    "shared-Huffman block {block_index} starts at {actual_offset}, table declares {expected_offset}"
                ),
            )
            .at_offset(expected_offset));
        }
        let final_block = reader.read_bit()? != 0;
        let block_type = reader.read_bits(2)?;
        match block_type {
            0 => {
                reader.align_byte();
                let lengths = reader.read_aligned_bytes(4)?;
                let length = u16::from_le_bytes([lengths[0], lengths[1]]);
                let complement = u16::from_le_bytes([lengths[2], lengths[3]]);
                if length != !complement {
                    return Err(Error::new(
                        ErrorKind::InvalidCompression,
                        "stored block length complement does not match",
                    ));
                }
                ensure_output_limit(output.len(), length as usize, max_output)?;
                output.extend_from_slice(reader.read_aligned_bytes(length as usize)?);
            }
            1 | 2 => loop {
                let symbol = literal_tree.decode(&mut reader)?;
                match symbol {
                    0..=255 => {
                        ensure_output_limit(output.len(), 1, max_output)?;
                        output.push(symbol as u8);
                    }
                    256 => break,
                    257..=285 => {
                        let index = symbol as usize - 257;
                        let length = LENGTH_BASE[index] + reader.read_bits(LENGTH_EXTRA[index])?;
                        let distance_symbol = distance_tree.decode(&mut reader)? as usize;
                        let (&base, &extra) = DISTANCE_BASE
                            .get(distance_symbol)
                            .zip(DISTANCE_EXTRA.get(distance_symbol))
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorKind::InvalidCompression,
                                    "invalid shared-Huffman distance symbol",
                                )
                            })?;
                        let distance = base + reader.read_bits(extra)?;
                        if distance == 0 || distance > output.len() {
                            return Err(Error::new(
                                ErrorKind::InvalidCompression,
                                "shared-Huffman back-reference is out of range",
                            ));
                        }
                        ensure_output_limit(output.len(), length, max_output)?;
                        for _ in 0..length {
                            output.push(output[output.len() - distance]);
                        }
                    }
                    _ => {
                        return Err(Error::new(
                            ErrorKind::InvalidCompression,
                            "invalid shared-Huffman literal/length symbol",
                        ));
                    }
                }
            },
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidCompression,
                    "reserved shared-Huffman block type",
                ));
            }
        }
        reader.align_four_bytes();
        let should_be_final = block_index + 1 == block_offsets.len();
        if final_block != should_be_final {
            return Err(Error::new(
                ErrorKind::InvalidCompression,
                if final_block {
                    "shared-Huffman stream ends before its declared block count"
                } else {
                    "shared-Huffman final block marker is missing"
                },
            ));
        }
    }
    Ok(output)
}

fn ensure_output_limit(current: usize, additional: usize, maximum: usize) -> Result<()> {
    if current
        .checked_add(additional)
        .is_none_or(|length| length > maximum)
    {
        return Err(Error::new(
            ErrorKind::LimitExceeded,
            "decompressed eZIP data exceeds the configured limit",
        ));
    }
    Ok(())
}

pub(crate) struct Unfiltered {
    pub pixels: Vec<u8>,
    pub warnings: Vec<Warning>,
}

pub(crate) fn unfilter(
    input: &[u8],
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
    block_rows: u8,
    has_filters: bool,
    mode: DecodeMode,
) -> Result<Unfiltered> {
    let stride = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "eZIP row size overflow"))?;
    let row_len = stride
        .checked_add(usize::from(has_filters))
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "eZIP row size overflow"))?;
    let expected_input = (height as usize)
        .checked_mul(row_len)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "eZIP image size overflow"))?;
    let output_len = (height as usize)
        .checked_mul(stride)
        .ok_or_else(|| Error::new(ErrorKind::LimitExceeded, "eZIP image size overflow"))?;
    if has_filters && block_rows == 0 {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            "filtered eZIP data requires a positive block-row count",
        ));
    }

    let mut warnings = Vec::new();
    if input.len() != expected_input {
        let message = format!(
            "decompressed eZIP data has {} bytes; expected {expected_input}",
            input.len()
        );
        if mode == DecodeMode::Strict {
            return Err(Error::new(ErrorKind::InvalidPixelLayout, message));
        }
        warnings.push(Warning::new(
            if input.len() < expected_input {
                WarningKind::PartialData
            } else {
                WarningKind::TrailingData
            },
            message,
        ));
    }

    let mut pixels = vec![0; output_len];
    if !has_filters {
        let copied = input.len().min(output_len);
        pixels[..copied].copy_from_slice(&input[..copied]);
        return Ok(Unfiltered { pixels, warnings });
    }

    for row in 0..height as usize {
        let input_start = row * row_len;
        if input_start >= input.len() {
            break;
        }
        let filter = input[input_start];
        if filter > 4 {
            let message = format!("unknown PNG filter {filter} on eZIP row {row}");
            if mode == DecodeMode::Strict {
                return Err(Error::new(ErrorKind::InvalidFilter, message).on_row(row as u32));
            }
            warnings.push(Warning::new(WarningKind::UnknownFilter, message).on_row(row as u32));
        }
        let available = input.len().saturating_sub(input_start + 1).min(stride);
        let first_in_block = row % block_rows as usize == 0;
        for column in 0..available {
            let raw = input[input_start + 1 + column];
            let output_index = row * stride + column;
            let left = if column >= bytes_per_pixel {
                pixels[output_index - bytes_per_pixel]
            } else {
                0
            };
            let up = if first_in_block {
                0
            } else {
                pixels[output_index - stride]
            };
            let up_left = if first_in_block || column < bytes_per_pixel {
                0
            } else {
                pixels[output_index - stride - bytes_per_pixel]
            };
            pixels[output_index] = match filter {
                0 => raw,
                1 => raw.wrapping_add(left),
                2 => raw.wrapping_add(up),
                3 => raw.wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => raw.wrapping_add(paeth(left, up, up_left)),
                _ => raw,
            };
        }
    }
    Ok(Unfiltered { pixels, warnings })
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let left = i32::from(left);
    let up = i32::from(up);
    let up_left = i32::from(up_left);
    let prediction = left + up - up_left;
    let left_distance = (prediction - left).abs();
    let up_distance = (prediction - up).abs();
    let diagonal_distance = (prediction - up_left).abs();
    if left_distance <= up_distance && left_distance <= diagonal_distance {
        left as u8
    } else if up_distance <= diagonal_distance {
        up as u8
    } else {
        up_left as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverses_all_png_filter_types() {
        let input = [
            0, 10, 20, 30, 1, 10, 10, 10, 2, 5, 5, 5, 3, 5, 5, 5, 4, 5, 5, 5,
        ];
        let decoded = unfilter(&input, 3, 5, 1, 32, true, DecodeMode::Strict).unwrap();
        assert_eq!(
            decoded.pixels,
            [10, 20, 30, 10, 20, 30, 15, 25, 35, 12, 23, 34, 17, 28, 39]
        );
    }

    #[test]
    fn resets_previous_row_at_block_boundaries() {
        let input = [0, 10, 20, 2, 1, 1, 2, 30, 40, 2, 1, 1];
        let decoded = unfilter(&input, 2, 4, 1, 2, true, DecodeMode::Strict).unwrap();
        assert_eq!(decoded.pixels, [10, 20, 11, 21, 30, 40, 31, 41]);
    }

    #[test]
    fn diagnostic_mode_zero_fills_partial_rows_and_ignores_unknown_filters() {
        let decoded = unfilter(&[9, 1, 2], 2, 2, 1, 32, true, DecodeMode::Diagnostic).unwrap();
        assert_eq!(decoded.pixels, [1, 2, 0, 0]);
        assert_eq!(decoded.warnings.len(), 2);
        assert_eq!(decoded.warnings[0].kind(), WarningKind::PartialData);
        assert_eq!(decoded.warnings[1].kind(), WarningKind::UnknownFilter);
    }

    #[test]
    fn rejects_over_subscribed_huffman_lengths() {
        let error = HuffmanTree::from_lengths(&[1, 1, 1]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidCompression);
        assert!(error.message().contains("over-subscribed"));
        assert!(HuffmanTree::from_lengths(&[1, 1]).is_ok());
    }
}
