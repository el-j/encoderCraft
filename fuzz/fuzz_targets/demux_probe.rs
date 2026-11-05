#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![no_main]

use encodercraft_demux::{decode_wav_samples, parse_aiff_header, parse_wav_header, probe_bytes, probe_jpeg, probe_png, probe_webp, sniff_container};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_AUDIO_SAMPLES: usize = 1_000_000;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let _ = sniff_container(data);
    let _ = probe_bytes(data);
    let _ = parse_aiff_header(data);
    let _ = probe_jpeg(data);
    let _ = probe_png(data);
    let _ = probe_webp(data);
    if let Ok(header) = parse_wav_header(data) {
        let _ = decode_wav_samples(&header, data, MAX_AUDIO_SAMPLES);
    }
});
