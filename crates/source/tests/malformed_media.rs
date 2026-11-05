//! Corpus of truncated, corrupt and hostile files: opening and decoding must return an error or a
//! frame, never panic, hang or allocate without bound.

use encodercraft_source::{MAX_AUDIO_DECODE_BYTES, MediaReader};
use std::path::Path;

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend_from_slice(&13u32.to_be_bytes());
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&width.to_be_bytes());
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v
}

fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let mut mp4 = vec![0, 0, 0, 24];
    mp4.extend_from_slice(b"ftypisom");
    mp4.extend_from_slice(&[0; 12]);
    mp4.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    mp4.extend_from_slice(b"moov");
    let mut riff = b"RIFF".to_vec();
    riff.extend_from_slice(&u32::MAX.to_le_bytes());
    riff.extend_from_slice(b"WAVEfmt ");
    riff.extend_from_slice(&u32::MAX.to_le_bytes());
    vec![
        ("empty", Vec::new()),
        ("one_byte", vec![0]),
        ("noise", (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect()),
        ("png_header_only", png_header(16, 16)),
        ("png_huge_dimensions", png_header(u32::MAX, u32::MAX)),
        ("png_zero_dimensions", png_header(0, 0)),
        ("mp4_oversized_box", mp4),
        ("wav_oversized_chunk", riff),
        ("jpeg_soi_only", vec![0xff, 0xd8, 0xff, 0xd9]),
        ("ts_sync_bytes", [0x47u8, 0, 0, 0].repeat(512)),
    ]
}

fn tiny_pcm_wav() -> Vec<u8> {
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&38u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8_000u32.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&2u32.to_le_bytes());
    wav.extend_from_slice(&[0, 0]);
    wav
}

#[test]
fn malformed_files_never_panic_for_any_extension() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["mp4", "mov", "mkv", "webm", "ts", "wav", "mp3", "flac", "png", "jpg", "tif", "webp", "exr", "bin"] {
        for (name, bytes) in corpus() {
            let path = dir.path().join(format!("{name}.{ext}"));
            std::fs::write(&path, &bytes).unwrap();
            if let Ok(reader) = MediaReader::open(&path) {
                let _ = reader.frame_at_index(0);
                let _ = reader.read_all_audio(48_000);
            }
        }
    }
}

#[test]
fn oversized_audio_sample_count_is_rejected_before_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.wav");
    std::fs::write(&path, tiny_pcm_wav()).unwrap();
    let reader = MediaReader::open(&path).unwrap();
    let sample_count = MAX_AUDIO_DECODE_BYTES / 4 + 1;

    let error = reader.read_audio_range(0, sample_count, 8_000).unwrap_err();
    assert!(error.to_string().contains("over the"));
}

#[test]
fn missing_and_directory_paths_are_errors() {
    assert!(MediaReader::open(Path::new("/definitely/not/here.mov")).is_err());
    let dir = tempfile::tempdir().unwrap();
    assert!(MediaReader::open(dir.path()).is_err());
}

proptest::proptest! {
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096), ext in proptest::sample::select(vec!["mp4", "mov", "mkv", "wav", "png", "jpg", "webp", "tif"])) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("fuzz.{ext}"));
        std::fs::write(&path, &bytes).unwrap();
        if let Ok(reader) = MediaReader::open(&path) {
            let _ = reader.frame_at_index(0);
        }
    }
}
