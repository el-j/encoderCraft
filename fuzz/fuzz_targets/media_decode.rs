#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![no_main]

use encodercraft_source::MediaReader;
use libfuzzer_sys::fuzz_target;
use std::io::Write;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_AUDIO_FRAMES: usize = 4096;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let extension = match data.first().copied().unwrap_or(0) % 6 {
        0 => "mp4",
        1 => "mov",
        2 => "webm",
        3 => "wav",
        4 => "png",
        _ => "webp",
    };
    let Ok(directory) = tempfile::tempdir() else {
        return;
    };
    let path = directory.path().join(format!("fuzz.{extension}"));
    let Ok(mut file) = std::fs::File::create(&path) else {
        return;
    };
    if file.write_all(data).is_err() {
        return;
    }
    drop(file);

    if let Ok(reader) = MediaReader::open(&path) {
        let _ = reader.frame_at_index(0);
        let _ = reader.audio(0, MAX_AUDIO_FRAMES, 48_000);
    }
});
