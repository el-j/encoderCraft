#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![no_main]

use encodercraft_audio_dsp::decode_audio;
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    // libFuzzer aborts on any panic. `decode_audio` converts panics from the third-party
    // demuxers into errors, which is the behaviour under test, so the abort hook is silenced.
    static QUIET: std::sync::Once = std::sync::Once::new();
    QUIET.call_once(|| std::panic::set_hook(Box::new(|_| {})));
    if data.len() <= MAX_INPUT_BYTES {
        let _ = decode_audio(data);
    }
});
