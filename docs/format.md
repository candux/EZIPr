# SiFli image resource format notes

This document records the evidence behind EZIPr's implementation. Fields are
marked as documented, fixture-confirmed, or inferred where that distinction is
useful.

## Resource header

Every static binary resource starts with one little-endian 32-bit word.

| Bits | Meaning | Evidence |
|---|---|---|
| 31:21 | height | documented and fixture-confirmed |
| 20:10 | width | documented and fixture-confirmed |
| 9:5 | reserved | documented |
| 4:0 | resource format | documented and fixture-confirmed |

Known format identifiers are:

| ID | Payload |
|---:|---|
| 1 | eZIP using RGB565, RGB888, or ARGB888 storage |
| 2 | eZIP using ARGB565 storage |
| 4 | PIXEL without alpha |
| 5 | PIXEL with alpha |

PIXEL data begins immediately after the resource header. Its byte length and
alpha flag distinguish RGB565, RGB888, ARGB565, and ARGB888 storage. Binary
fixtures append a little-endian CRC-32 of the pixel bytes. This trailer is
fixture-confirmed but absent from the prose format description.

## Pixel byte order

All multi-byte pixel values are little-endian. RGB888 is stored as B, G, R.
ARGB565 is stored as a little-endian RGB565 word followed by alpha. ARGB888 is
stored as B, G, R, A.

## Standard eZIP stream

Compressed resources place a 16-byte stream header after the resource header.
Multi-byte fields in this inner header are big-endian.

| Offset | Size | Meaning | Evidence |
|---:|---:|---|---|
| 0 | 4 | complete inner stream size | fixture-confirmed |
| 4 | 1 | control flags | fixture-confirmed; individual bits partly inferred |
| 5 | 1 | component/packed bit depth | fixture-confirmed |
| 6 | 1 | rows per independently filtered block | fixture-confirmed |
| 7 | 1 | flags | fixture-confirmed; meaning unknown |
| 8 | 2 | width | fixture-confirmed |
| 10 | 2 | height | fixture-confirmed |
| 12 | 1 | low nibble is filter mode | fixture-confirmed |
| 13 | 3 | reserved | fixture-confirmed |

For standard streams, the header is followed by a raw DEFLATE stream and a
big-endian Adler-32 of the decompressed bytes. Control bit `0x40` identifies a
distinct shared-Huffman/block representation.

Filter mode 1 stores pixels directly. Other observed modes add one PNG filter
byte to each row. Filters use the PNG None, Sub, Up, Average, and Paeth rules.
The preceding row is reset to zero at each `block_rows` boundary.

Control bytes use the low nibble as a color type: 2, 6, 8, and 12 mean RGB888,
ARGB888, RGB565, and ARGB565 respectively. Standard streams use
`0x10 | color_type`; shared-Huffman streams use `0x40 | color_type`. The depth
byte is 8 for RGB888 and ARGB888, 16 for packed RGB565, and 24 for ARGB565.

## Shared-Huffman stream

The shared-Huffman representation is fixture-confirmed using source images
owned by this project. Its declared inner size excludes a four-byte
little-endian CRC-32 trailer. The CRC covers the entire declared inner stream,
including its 16-byte header.

After the header are a big-endian 16-bit block-row count, a big-endian 16-bit
block count, one four-byte offset per block, and the bit stream. A single
dynamic Huffman table precedes all aligned DEFLATE-like blocks. Literal,
length, and distance coding follows DEFLATE, while each block begins on a
four-byte boundary and reuses the shared table.

EZIPr decodes this representation but emits the simpler standard raw-DEFLATE
form.

The static encoder can either write adaptive PNG row filters or filterless
scanlines. Its raw DEFLATE output is deterministic for the locked dependency
graph, though byte-for-byte equality with another encoder is neither required
nor expected.

## eZIP-A animation

Animation streams use control high nibble `0x50`. Like shared-Huffman static
streams, their declared inner size excludes a little-endian CRC-32 over the
declared inner bytes. The 16-byte stream header is followed by an optional
four-byte-per-entry palette, an eight-byte animation control, and a frame
offset table. Frame and play counts and all offsets are big-endian 32-bit
values. A play count of zero means infinite repetition.

Frame offsets are relative to the beginning of the inner stream. Each packed
30-byte frame header contains these big-endian fields:

| Size | Meaning |
|---:|---|
| 4 | sequence number |
| 4 | frame width |
| 4 | frame height |
| 4 | canvas x offset |
| 4 | canvas y offset |
| 2 | delay numerator |
| 2 | delay denominator; zero means 100 |
| 1 | disposal operation |
| 1 | blend operation |
| 2 | compressed-size high half |
| 2 | compressed-size low half |

The header is followed by a raw DEFLATE payload and a big-endian Adler-32 of
the decompressed filtered frame. Intermediate records are padded so the next
frame header begins at a four-byte-aligned offset. Disposal values 0, 1, and 2
mean none, background, and previous; blend values 0 and 1 mean source and over.

The container layout, frame fields, checksums, alignment, timing, rectangle,
disposal, and blend values are confirmed by externally encoded animation
binaries made from the project-owned controlled PNG sources recorded in the
fixture manifest. These binaries are not EZIPr encoder output. Palette
serialization remains recognized but unsupported until a fixture establishes
its byte semantics.

The animation encoder writes the same container using raw-DEFLATE frame data.
It resolves one storage format across the complete frame set, preserves caller
supplied rectangles and timing, and aligns every frame record after the first.
