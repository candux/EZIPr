# EZIPr

EZIPr is a Rust library and command-line program for decoding and encoding
SiFli eZIP and PIXEL image resources. The command-line program reads and writes
PNG and APNG, imports GIF animations, and accepts TOML animation manifests.

Decoding supports static PIXEL resources, standard eZIP, shared-Huffman eZIP,
and eZIP-A animations. Encoding supports PIXEL resources, standard eZIP, and
eZIP-A. The RGB565 encoder provides grid-stable ordered dithering and optional
size-optimized compression.

Animated resources expose stored frame rectangles and a sequential compositor
that applies blending and disposal. Animation encoding accepts frame
rectangles, timing, disposal, blending, and repeat metadata.

The library accepts caller-owned RGB or RGBA buffers and returns owned encoded
resources or decoded images. A minimal static round trip looks like this:

```rust
use ezipr::{DecodedImage, Decoder, Encoder, ImageView, PixelFormat};

fn round_trip(rgb: &[u8]) -> ezipr::Result<DecodedImage> {
    let image = ImageView::new(8, 8, PixelFormat::Rgb8, 8 * 3, rgb)?;
    let encoded = Encoder::default().encode(image)?;
    Decoder::new(encoded.as_bytes())?.decode_frame(0, PixelFormat::Rgba8)
}
```

Install the optional command-line program from the repository:

```console
cargo install --path . --features cli

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

Ordered dithering replaces some gradient banding with fine pixel-level noise.
For typical UI elements, `balanced` output is about 30% larger than `none` and
can be nearly three times larger for a synthetic 128x128 gray gradient. A flat
color already on the RGB565 reconstruction grid has no size penalty. The
`reference` mode exists for conversion compatibility; `balanced` is recommended
for new resources.

RGB565 resources made with different dithering settings can therefore decode
to slightly different colors even when both files are valid. RGB888 does not
require this quantization, and the dithering option has no effect on RGB888 or
ARGB888 output.

## Size-optimized encoding

Use `ezipr encode input.png output.bin --smallest` to search for a smaller eZIP
resource. It compares adaptive filtering, each fixed PNG row filter, and
filterless storage at every miniz compression level, then tries Zopfli on the
best filtered representation. The smallest result is deterministic and never
larger than the equivalent normal encode.

For animations, each frame is optimized and the selected filter mode is used
consistently across the resource. `--no-filters` restricts the search to
filterless data. PIXEL resources are uncompressed and do not support
`--smallest`.

A typical 1.6-megapixel RGB565 image takes about 0.3 seconds with normal
encoding and 40 seconds with `--smallest`. The resulting resource is typically
15% to 18% smaller, with no change to decoding speed or compatibility.

Library users enable `CompressionStrategy::Smallest` with the `smallest` Cargo
feature. The `cli` feature includes it automatically; normal encoding and
decoding do not require Zopfli.

## License

EZIPr is licensed under the MIT License.
