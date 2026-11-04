//! Decode conformance for every supported video codec, using real streams
//! produced by independent encoders (see `tests/fixtures/README.md`).

use encodercraft_source::MediaReader;
use std::path::PathBuf;

const W: usize = 96;
const H: usize = 64;
const FRAMES: u64 = 15;
/// Source frames 0 and 14 of the reference clip, as 8-bit luma.
const REFERENCE: &[u8] = include_bytes!("fixtures/reference_luma.raw");

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn luma(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let (r, g, b) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            (0.2126 * r + 0.7152 * g + 0.0722 * b).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum::<f64>() / a.len().max(1) as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

fn check(name: &str, min_psnr: f64) {
    let reader = MediaReader::open(&fixture(name)).unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
    let video = reader.probe().video.clone().unwrap_or_else(|| panic!("{name}: no video stream"));
    assert_eq!((video.width as usize, video.height as usize), (W, H), "{name}: dimensions");
    assert_eq!(reader.frame_count(), FRAMES, "{name}: frame count");
    for (slot, index) in [(0usize, 0u64), (1, FRAMES - 1)] {
        let frame = reader.frame_at_index(index).unwrap_or_else(|e| panic!("{name}: frame {index}: {e}"));
        assert_eq!((frame.dimensions.width as usize, frame.dimensions.height as usize), (W, H), "{name}: frame size");
        let got = luma(frame.data());
        let want = REFERENCE.get(slot * W * H..(slot + 1) * W * H).unwrap_or(&[]);
        let p = psnr(&got, want);
        assert!(p >= min_psnr, "{name}: frame {index} PSNR {p:.1} dB below {min_psnr} dB");
    }
}

#[test]
fn h264_decodes() {
    check("h264.mp4", 20.0);
}

#[test]
fn hevc_decodes() {
    check("hevc.mp4", 20.0);
}

#[test]
fn vp8_decodes() {
    check("vp8.webm", 20.0);
}

#[test]
fn vp9_decodes() {
    check("vp9.webm", 20.0);
}

#[test]
fn av1_decodes() {
    check("av1.mp4", 20.0);
}

#[test]
fn prores_decodes() {
    check("prores.mov", 20.0);
}

#[test]
fn mpeg2_program_stream_decodes() {
    check("mpeg2.mpg", 20.0);
}

#[test]
fn mpeg2_transport_stream_decodes() {
    check("mpeg2.ts", 20.0);
}
