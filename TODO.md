# TODO

## Size-optimized encoding

- Add an opt-in encoder mode that minimizes the complete encoded resource size.
- Include the current adaptive row-filter plan as a baseline so optimization can
  never produce a larger result.
- Generate additional legal PNG row-filter plans and select candidates using
  their actual raw-DEFLATE size instead of only the current per-row residual
  score.
- Try several high compression levels because a numerically higher level does
  not always produce the smallest stream. Use deterministic tie-breaking.
- Add an explicit maximum-compression strategy backed by the Rust `zopfli`
  crate's raw-DEFLATE output. Rank filter candidates with the faster compressor
  first, then run Zopfli only on the finalist instead of multiplying its cost
  across every filter plan.
- Compare the Zopfli result with the best faster result and retain the smaller
  complete resource. Keep this strategy opt-in because encoding can take tens
  of seconds for a multi-megabyte filtered image.
- Keep pixel order, channel order, and the on-wire representation unchanged.
- Keep `block_rows = 32` unless the caller explicitly chooses another value;
  automatically varying it needs compatibility testing on target hardware.
- Apply the same optimization to static resources and independently compressed
  animation frames.
- Add regression tests proving that optimized output decodes to the same pixels,
  is deterministic, and is no larger than the baseline candidate.

## RGB565 perceptual validation

- Compare the shared Bayer threshold against phase-shifted or luma-aware
  alternatives on neutral gray ramps using target hardware. Preserve the
  balanced mode's reconstruction-level fixed point in any alternative.
