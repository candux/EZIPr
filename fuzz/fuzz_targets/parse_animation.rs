#![no_main]

use ezipr::{DecodeLimits, DecodeMode, DecodeOptions, Decoder, PixelFormat, ResourceKind};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for mode in [DecodeMode::Strict, DecodeMode::Diagnostic] {
        let options = DecodeOptions::new().mode(mode).limits(limits());
        if let Ok(decoder) = Decoder::with_options(data, options) {
            if decoder.info().kind() == ResourceKind::Animation {
                for index in 0..decoder.info().frame_count() {
                    let _ = decoder.frame_info(index);
                    let _ = decoder.decode_frame(index, PixelFormat::Rgba8);
                }
            }
        }
    }
});

fn limits() -> DecodeLimits {
    DecodeLimits::new()
        .max_dimensions(512, 512)
        .max_frames(64)
        .max_decoded_bytes(4 * 1024 * 1024)
}
