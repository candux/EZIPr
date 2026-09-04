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

The command-line program is optional:

```console
cargo build --features cli

ezipr info image.bin
ezipr verify image.bin
ezipr decode image.bin image.png
ezipr decode animation.bin --frames frames
ezipr encode image.png image.bin --depth rgb565
ezipr encode animation.apng animation.bin --depth rgb888
ezipr encode animation.gif animation.bin
```

For exact animation frame rectangles and control metadata, `encode` also
accepts a TOML manifest. Frame paths are resolved relative to the manifest:

```toml
width = 320
height = 240
repeat = 0
depth = "rgb565"
alpha = "auto"
block_rows = 32
filters = true
compression = 6

[[frames]]
file = "frames/background.png"
delay_numerator = 1
delay_denominator = 10

[[frames]]
file = "frames/overlay.png"
x = 24
y = 16
delay_numerator = 1
delay_denominator = 20
disposal = "background"
blend = "over"
```

A repeat value of zero means infinite playback. Disposal values are `none`,
`background`, and `previous`; blend values are `source` and `over`.

## RGB565 conversion and dithering

RGB565 stores only five red bits, six green bits, and five blue bits. EZIPr
currently converts eight-bit input by discarding the low channel bits. It does
not apply dithering.

The reference encoder dithers colors while reducing true-color PNG input to
RGB565. Consequently, EZIPr and reference-encoded resources made from the same
image can decode to slightly different RGB values even when both files are
valid. Smooth gradients may show more banding with EZIPr's direct conversion;
the dithered result replaces some of that banding with fine pixel-level noise.
The undithered pixels can also compress substantially better because they tend
to contain longer repeated patterns.

This difference is limited to reduced-precision color encoding and is not
evidence of a decoding error. RGB888 does not require this quantization and can
preserve the source RGB bytes exactly. Optional deterministic RGB565 dithering
is tracked in [TODO.md](TODO.md).
