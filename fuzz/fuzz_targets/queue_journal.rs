#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
#![no_main]

use encodercraft_engine::load_journal;
use libfuzzer_sys::fuzz_target;
use std::io::Write;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let Ok(directory) = tempfile::tempdir() else {
        return;
    };
    let path = directory.path().join("queue-journal.json");
    let Ok(mut file) = std::fs::File::create(&path) else {
        return;
    };
    if file.write_all(data).is_err() {
        return;
    }
    drop(file);
    let _ = load_journal(&path);
});
