# EZIPr

EZIPr is a native Rust library for inspecting, decoding, and encoding SiFli
eZIP and PIXEL image resources. Static and animated images share one resource
model, while the command-line program provides PNG, APNG, GIF, and frame
manifest adapters.

Static PIXEL, standard eZIP, shared-Huffman eZIP, and eZIP-A frame decoding are
supported. Animated resources expose stored frame rectangles as well as a
stateful sequential compositor for blend and disposal handling. Native eZIP-A
encoding accepts explicit frame rectangles, timing, disposal, blend, and
repeat metadata.

The implementation is under active development. The checked-in tests define
the currently supported format surface.

The library accepts caller-owned RGB or RGBA buffers and returns owned encoded
resources or decoded images. A minimal static round trip looks like this:

```rust
use ezipr::{Decoder, Encoder, ImageView, PixelFormat};

# fn example(rgb: &[u8]) -> ezipr::Result<()> {
let image = ImageView::new(8, 8, PixelFormat::Rgb8, 8 * 3, rgb)?;
let encoded = Encoder::default().encode(image)?;
let decoded = Decoder::new(encoded.as_bytes())?
    .decode_frame(0, PixelFormat::Rgba8)?;
# let _ = decoded;
# Ok(())
# }
```
