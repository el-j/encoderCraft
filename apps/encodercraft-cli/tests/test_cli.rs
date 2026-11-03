//! Integration tests verifying encoderCraft headless CLI subcommands and batch processing.

use std::process::Command;

#[test]
fn test_cli_help_and_presets() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");

    // 1. --help output
    let output = Command::new(bin).arg("--help").output().expect("failed to run cli");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("encoderCraft CLI"));
    assert!(stdout.contains("encode"));
    assert!(stdout.contains("probe"));
    assert!(stdout.contains("presets"));
    assert!(stdout.contains("watch"));

    // 2. presets output
    let p_output = Command::new(bin).arg("presets").output().expect("failed to run presets");
    assert!(p_output.status.success());
    let p_stdout = String::from_utf8_lossy(&p_output.stdout);
    assert!(p_stdout.contains("YouTube"));
    assert!(p_stdout.contains("Match Source"));
}

#[test]
fn test_cli_version_matches_manifest() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    for flag in ["--version", "-V"] {
        let output = Command::new(bin).arg(flag).output().expect("failed to run cli");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), format!("encodercraft-cli {}", env!("CARGO_PKG_VERSION")));
    }
}

fn write_bmp(path: &std::path::Path, shade: u8) {
    let (w, h) = (16u32, 16u32);
    let row = (w * 3).div_ceil(4) * 4;
    let data = row * h;
    let mut b = Vec::new();
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(54 + data).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&data.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    for y in 0..h {
        for x in 0..row {
            b.push(shade.wrapping_add((x + y) as u8));
        }
    }
    std::fs::write(path, b).expect("write bmp");
}

#[test]
fn test_cli_encode_batch_runs_every_input_through_the_queue() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = std::env::temp_dir().join(format!("encodercraft-cli-batch-{}", std::process::id()));
    let out = dir.join("out");
    std::fs::create_dir_all(&out).expect("mkdir");
    let inputs: Vec<_> = (0..3u8)
        .map(|i| {
            let p = dir.join(format!("clip{i}.bmp"));
            write_bmp(&p, i * 40);
            p
        })
        .collect();

    let output = Command::new(bin).arg("encode").args(&inputs).arg("-o").arg(&out).output().expect("failed to run encode");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "encode failed: {stderr}");
    for i in 0..3 {
        let f = out.join(format!("clip{i}_encoded.mp4"));
        let len = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        assert!(len > 0, "missing output {}", f.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_cli_encode_reports_failure_with_nonzero_exit() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let output = Command::new(bin).args(["encode", "/nonexistent/in.mov"]).output().expect("failed to run encode");
    assert!(!output.status.success());
}

/// Converts a known picture to every still preset through the CLI and checks the result by
/// decoding it again: the guard against grey or empty output.
#[test]
fn test_cli_still_presets_roundtrip_real_pixels() {
    use encodercraft_source::MediaReader;
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = std::env::temp_dir().join(format!("encodercraft-cli-stills-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let src = dir.join("src.bmp");
    write_bmp(&src, 10);
    let source = MediaReader::open(&src).expect("open source").frame_at_index(0).expect("decode source");
    let source_px = source.data().to_vec();

    for (preset, ext, lossless) in [
        ("Still Image PNG (Lossless)", "png", true),
        ("Windows Bitmap BMP (Uncompressed)", "bmp", true),
        ("Still Image WebP (Lossless)", "webp", true),
        ("Still Image JPEG", "jpg", false),
    ] {
        let produced = [dir.join(format!("out.{ext}"))];
        let output = Command::new(bin).args(["encode"]).arg(&src).args(["-p", preset, "-o"]).arg(&produced[0]).output().expect("run encode");
        assert!(output.status.success(), "{preset}: {}", String::from_utf8_lossy(&output.stderr));
        let decoded = MediaReader::open(&produced[0]).expect("decode output").frame_at_index(0).expect("decode frame");
        assert_eq!(decoded.data().len(), source_px.len(), "{preset}: frame size");
        let total: u64 = decoded.data().iter().zip(&source_px).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
        let mean = total as f64 / source_px.len() as f64;
        let limit = if lossless { 0.5 } else { 12.0 };
        assert!(mean <= limit, "{preset}: mean pixel error {mean} exceeds {limit}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
