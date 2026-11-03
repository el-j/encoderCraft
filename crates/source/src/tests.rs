use super::*;
use std::io::Write;

fn bmp_2x2(pixels_bgr: [[u8; 3]; 4]) -> Vec<u8> {
    // 24-bit bottom-up BMP, rows padded to 4 bytes (2 px * 3 = 6 -> 8).
    let mut data = Vec::new();
    for row in [[pixels_bgr[2], pixels_bgr[3]], [pixels_bgr[0], pixels_bgr[1]]] {
        for px in row {
            data.extend_from_slice(&px);
        }
        data.extend_from_slice(&[0, 0]);
    }
    let size = 54 + data.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&2i32.to_le_bytes());
    out.extend_from_slice(&2i32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(&data);
    out
}

fn wav_mono_16(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(bytes).unwrap();
    path
}

#[test]
fn bmp_still_decodes_real_pixels_not_grey() {
    let dir = tempfile::tempdir().unwrap();
    // BGR order: red, green, blue, white (top-left to bottom-right).
    let path = write(dir.path(), "px.bmp", &bmp_2x2([[0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 255]]));
    let reader = MediaReader::open(&path).unwrap();
    assert_eq!(reader.probe().kind, SourceKind::Still);
    assert_eq!(reader.frame_count(), 1);
    let frame = reader.frame_at_index(0).unwrap();
    assert_eq!((frame.dimensions.width, frame.dimensions.height), (2, 2));
    let px = frame.data();
    assert_eq!(&px[0..4], &[255, 0, 0, 255], "top-left must be red");
    assert_eq!(&px[4..8], &[0, 255, 0, 255], "top-right must be green");
    assert_eq!(&px[8..12], &[0, 0, 255, 255], "bottom-left must be blue");
    assert_eq!(&px[12..16], &[255, 255, 255, 255], "bottom-right must be white");
}

#[test]
fn wav_audio_roundtrips_samples() {
    let dir = tempfile::tempdir().unwrap();
    let samples: Vec<i16> = (0..4800).map(|i| ((i % 100) as i16 - 50) * 300).collect();
    let path = write(dir.path(), "tone.wav", &wav_mono_16(48_000, &samples));
    let reader = MediaReader::open(&path).unwrap();
    assert_eq!(reader.probe().kind, SourceKind::AudioOnly);
    assert_eq!(reader.frame_count(), 0);
    let audio = reader.read_all_audio(48_000).unwrap();
    assert_eq!(audio.channel_count(), 1);
    assert_eq!(audio.samples_per_channel(), 4800);
    let ch = audio.channel(0).unwrap();
    let expected = f32::from(samples[10]) / 32768.0;
    assert!((ch[10] - expected).abs() < 1e-3, "sample 10 was {} expected {expected}", ch[10]);
}

#[test]
fn audio_only_source_has_no_video_frames() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a.wav", &wav_mono_16(48_000, &[0; 480]));
    let reader = MediaReader::open(&path).unwrap();
    assert!(matches!(reader.frame_at_index(0), Err(EncoderError::InvalidContainer(_))));
}

#[test]
fn still_has_no_audio_stream() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "px.bmp", &bmp_2x2([[1, 2, 3]; 4]));
    let reader = MediaReader::open(&path).unwrap();
    assert!(reader.audio(0, 100, 48_000).is_err());
    assert!(reader.read_all_audio(48_000).is_err());
}

#[test]
fn out_of_range_frame_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "px.bmp", &bmp_2x2([[1, 2, 3]; 4]));
    let reader = MediaReader::open(&path).unwrap();
    assert!(reader.frame_at_index(1).is_err());
}

#[test]
fn garbage_file_is_unsupported_not_synthesised() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "junk.mov", &[0x13; 4096]);
    assert!(MediaReader::open(&path).is_err());
}

#[test]
fn missing_file_reports_io_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(MediaReader::open(&dir.path().join("nope.mp4")), Err(EncoderError::Io(_))));
}

#[test]
fn truncated_png_header_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "cut.png", &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]);
    assert!(MediaReader::open(&path).is_err());
}

#[test]
fn channel_counts_map_to_supported_layouts() {
    assert_eq!(layout_for(1), ChannelLayout::Mono);
    assert_eq!(layout_for(2), ChannelLayout::Stereo);
    assert_eq!(layout_for(3), ChannelLayout::Surround51);
    assert_eq!(layout_for(6), ChannelLayout::Surround51);
    assert_eq!(layout_for(7), ChannelLayout::Surround71);
    assert_eq!(layout_for(32), ChannelLayout::Surround71);
}
