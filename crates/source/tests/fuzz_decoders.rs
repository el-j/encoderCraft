//! Hostile-input tests for every decoder entry point: corrupted or truncated
//! streams must produce an error or a frame, never a panic.

use encodercraft_source::MediaReader;
use proptest::prelude::*;
use std::path::PathBuf;

const FIXTURES: &[&str] = &["h264.mp4", "hevc.mp4", "vp8.webm", "vp9.webm", "av1.mp4", "prores.mov", "mpeg2.mpg", "mpeg2.ts"];

fn load(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap_or_default()
}

fn decode_all(name: &str, bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    if let Ok(reader) = MediaReader::open(&path) {
        let count = reader.frame_count().min(4);
        for i in 0..count {
            let _ = reader.frame_at_index(i);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn truncated_streams_never_panic(which in 0..FIXTURES.len(), keep in 0.0f64..1.0) {
        let name = FIXTURES[which];
        let bytes = load(name);
        let cut = (bytes.len() as f64 * keep) as usize;
        decode_all(name, bytes.get(..cut).unwrap_or(&[]));
    }

    #[test]
    fn bit_flipped_streams_never_panic(which in 0..FIXTURES.len(), flips in proptest::collection::vec((0.0f64..1.0, any::<u8>()), 1..16)) {
        let name = FIXTURES[which];
        let mut bytes = load(name);
        let len = bytes.len();
        for (pos, value) in flips {
            let at = ((len as f64) * pos) as usize;
            if let Some(b) = bytes.get_mut(at) {
                *b ^= value | 1;
            }
        }
        decode_all(name, &bytes);
    }
}
