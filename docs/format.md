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
| 1 | eZIP without alpha |
| 2 | eZIP with alpha |
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
