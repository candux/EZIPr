#![no_main]

use ezipr::{DecodeLimits, DecodeMode, DecodeOptions, Decoder, PixelFormat, ResourceKind};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for mode in [DecodeMode::Strict, DecodeMode::Diagnostic] {
        let options = DecodeOptions::new().mode(mode).limits(
            DecodeLimits::new()
                .max_dimensions(512, 512)
                .max_frames(64)
                .max_decoded_bytes(4 * 1024 * 1024),
        );
        if let Ok(decoder) = Decoder::with_options(data, options) {
            if decoder.info().kind() == ResourceKind::Animation {
                if let Ok(mut compositor) = decoder.compositor(PixelFormat::Rgba8) {
                    while let Ok(Some(_)) = compositor.next_frame() {}
                }
                let frame_count = decoder.info().frame_count();
                if frame_count != 0 {
                    let index = data.last().copied().unwrap_or(0) as usize % frame_count;
                    let _ = decoder.decode_composited_frame(index, PixelFormat::Rgb8);
                }
            }
        }
    }
});
