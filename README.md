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
ezipr encode image.png image.bin --depth rgb565 --dither reference
ezipr encode image.png image.bin --smallest
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
dither = "balanced"
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
offers three deterministic conversion modes:

| `--dither` / manifest value | Library value | Behavior |
| --- | --- | --- |
| `balanced` | `Rgb565Dithering::Balanced8x8` | Default. Quantizes against the actual 5-bit and 6-bit reconstruction levels and distributes the residual with an 8x8 ordered pattern. Every decoded RGB565 level is a fixed point, including black, white, and saturated primaries. |
| `reference` | `Rgb565Dithering::Reference8x8` | Reproduces the reference encoder's component-specific 8x8 conversion. It can brighten some pixels that already lie on the RGB565 grid, including black pixels. |
| `none` | `Rgb565Dithering::None` | Discards the low channel bits directly. This is spatially stable and decode-encode idempotent, but smooth gradients can show more banding. |

Select the same values with `--dither` on the command line or `dither` in an
animation manifest. Both ordered modes use absolute animation-canvas
coordinates so the pattern does not shift at frame rectangle boundaries.
Alpha bytes are never dithered.

Ordered dithering replaces some gradient banding with fine pixel-level noise,
but the compression cost can be material. With the current encoder, `balanced`
was 30% larger than `none` across a 19-image UI sample. A synthetic 128x128
gray gradient was nearly three times larger, while a flat color already on the
RGB565 reconstruction grid had no size penalty. Measure representative assets
when flash usage matters. The `reference` mode exists for conversion
compatibility; `balanced` is the recommended choice for new resources.

RGB565 resources made with different dithering settings can therefore decode
to slightly different colors even when both files are valid. RGB888 does not
require this quantization, and the dithering option has no effect on RGB888 or
ARGB888 output.

## Size-optimized encoding

`ezipr encode input.png output.bin --smallest` enables an exhaustive,
deterministic search over the encoder's current compression candidates. It
tries the adaptive PNG row-filter plan, plans using each single PNG filter,
and filterless storage. Every candidate is compressed with every miniz level
from 0 through 10. Zopfli is then run on the best representation found, and
its raw-DEFLATE stream is retained only when it is smaller.

For animations, every frame's filter plan and compressed stream are optimized
independently. Because eZIP-A stores one filter-mode flag for the entire
animation, filtered and filterless results are compared by their total frame
size and the smaller mode is used consistently for every frame. Use
`--no-filters` to search only filterless frame data.

The result is guaranteed not to exceed the normal configured candidate, but
it is the smallest of the candidates described above rather than a proof of
the globally smallest possible DEFLATE stream. Encoding can be much slower,
especially for large images, while decoding speed and compatibility are
unchanged. `--compression` still selects the single-pass level without
`--smallest` and supplies the baseline candidate when optimization is enabled.
