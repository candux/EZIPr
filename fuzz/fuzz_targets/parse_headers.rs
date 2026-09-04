#![no_main]

use ezipr::{ResourceHeader, StreamHeader};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = ResourceHeader::parse(data);
    let _ = StreamHeader::parse(data);
    if data.len() >= ResourceHeader::BYTE_LEN {
        let _ = StreamHeader::parse(&data[ResourceHeader::BYTE_LEN..]);
    }
});
