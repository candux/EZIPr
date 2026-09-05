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
ezipr encode image.png image.bin --depth rgb565 --dither none
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
dither = "ordered"
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
uses deterministic, component-specific 8x8 ordered dithering by default when
reducing eight-bit input to RGB565 or ARGB565. Animation frames use their
absolute canvas coordinates so that the pattern does not shift at frame
rectangle boundaries. Alpha bytes are never dithered.

The default follows the reference encoder's RGB565 conversion. Callers can
select `Rgb565Dithering::None`, `--dither none`, or `dither = "none"` in a
frame manifest to discard the low channel bits directly instead. Direct
conversion can make banding more visible in smooth gradients. Ordered
dithering replaces some of that banding with fine pixel-level noise and can
increase the compressed size substantially because it breaks up repeated
colors.

RGB565 resources made with different dithering settings can therefore decode
to slightly different colors even when both files are valid. RGB888 does not
require this quantization, and the dithering option has no effect on RGB888 or
ARGB888 output.
