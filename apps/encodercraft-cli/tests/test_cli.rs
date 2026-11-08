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
    assert!(stdout.contains("--rotate"));
    assert!(stdout.contains("--frame-memory-mib"));
    assert!(stdout.contains("--normalization-memory-mib"));
}

#[test]
fn test_cli_rejects_invalid_transform_and_memory_options() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    for (flag, value, message) in [
        ("--rotate", "45", "invalid rotation"),
        ("--scale-mode", "warp", "invalid scale mode"),
        ("--frame-rate-mode", "interpolate", "invalid frame-rate mode"),
        ("--frame-memory-mib", "0", "between 1 and 65536 MiB"),
        ("--normalization-memory-mib", "many", "positive integer MiB"),
    ] {
        let output = Command::new(bin).args(["encode", "input.mov", flag, value]).output().expect("run cli");
        assert!(!output.status.success(), "{flag} {value} unexpectedly succeeded");
        assert!(String::from_utf8_lossy(&output.stderr).contains(message), "{flag} {value}: {}", String::from_utf8_lossy(&output.stderr));
    }

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

#[test]
fn test_cli_unattended_lossy_conversion_requires_a_saved_allow() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/source/tests/fixtures/prores.mov");
    let output_path = dir.path().join("unapproved.mp4");
    let output = Command::new(bin)
        .args(["encode"])
        .arg(&source)
        .args(["-p", "YouTube 1080p Full HD", "-o"])
        .arg(&output_path)
        .env("XDG_CONFIG_HOME", dir.path().join("xdg"))
        .env("HOME", dir.path().join("home"))
        .env("APPDATA", dir.path().join("appdata"))
        .output()
        .expect("run encode");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Lossy conversion requires approval"), "{stderr}");
    assert!(!output_path.exists());
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

#[test]
fn test_cli_numbered_image_sequence_encodes_to_video_at_the_given_rate() {
    use encodercraft_source::MediaReader;
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = std::env::temp_dir().join(format!("encodercraft-cli-seq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    for n in 1..=10u8 {
        write_bmp(&dir.join(format!("frame_{n:03}.bmp")), n.wrapping_mul(20));
    }
    let out = dir.join("seq.mp4");
    let output = Command::new(bin)
        .args(["encode"])
        .arg(dir.join("frame_%03d.bmp"))
        .args(["-p", "Match Source - High Bitrate", "--fps", "25", "-o"])
        .arg(&out)
        .output()
        .expect("run encode");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let reader = MediaReader::open(&out).expect("decode output");
    let video = reader.probe().video.clone().expect("video stream");
    assert_eq!(video.frame_rate.num as f64 / video.frame_rate.den as f64, 25.0);
    assert_eq!(reader.frame_count(), 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_cli_rejects_a_bad_frame_rate() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let output = Command::new(bin).args(["encode", "x.mov", "--fps", "banana"]).output().expect("run encode");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid frame rate"));
}

#[test]
fn test_cli_convert_image_to_png_and_rejects_invalid_pairs() {
    use encodercraft_source::MediaReader;
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src.bmp");
    write_bmp(&src, 25);
    let want = MediaReader::open(&src).expect("open source").frame_at_index(0).expect("decode source").data().to_vec();

    let out = dir.path().join("out.png");
    let run = Command::new(bin).arg("convert").arg(&src).args(["--to", "png", "-o"]).arg(&out).output().expect("run convert");
    assert!(run.status.success(), "convert failed: {}", String::from_utf8_lossy(&run.stderr));
    let got = MediaReader::open(&out).expect("open png").frame_at_index(0).expect("decode png").data().to_vec();
    assert_eq!(got, want, "lossless PNG must keep the pixels");

    // default output name follows the target extension
    let default = Command::new(bin).current_dir(dir.path()).arg("convert").arg(&src).args(["--to", "tif"]).output().expect("run convert");
    assert!(default.status.success(), "{}", String::from_utf8_lossy(&default.stderr));
    assert!(dir.path().join("src_encoded.tif").exists());

    // image -> audio is not a valid conversion and must not create output
    let bad_out = dir.path().join("bad.wav");
    let bad = Command::new(bin).arg("convert").arg(&src).args(["--to", "wav", "-o"]).arg(&bad_out).output().expect("run convert");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("valid targets"));
    assert!(!bad_out.exists());

    // mismatched -o extension, unknown format and missing --to
    let mismatch = Command::new(bin).arg("convert").arg(&src).args(["--to", "png", "-o"]).arg(dir.path().join("x.jpg")).output().expect("run");
    assert!(!mismatch.status.success());
    assert!(!Command::new(bin).arg("convert").arg(&src).args(["--to", "bogus"]).output().expect("run").status.success());
    assert!(!Command::new(bin).arg("convert").arg(&src).output().expect("run").status.success());
}

#[test]
fn test_cli_convert_session_to_soundcraft_and_flac() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("tone.wav");
    let samples: Vec<u8> = (0..48_000u32)
        .flat_map(|i| {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 12_000.0) as i16;
            [v.to_le_bytes(), v.to_le_bytes()].concat()
        })
        .collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&48_000u32.to_le_bytes());
    w.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    w.extend_from_slice(&samples);
    std::fs::write(&wav, w).expect("write wav");
    let sesx = dir.path().join("mix.sesx");
    std::fs::write(
        &sesx,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<sesx version="1.0" audioRate="48000" bitDepth="24" sessionName="T">
  <masterTrack volume="0.00" pan="0.00"/>
  <tracks><audioTrack id="1" name="Tone" volume="0.00" pan="0.00" muted="false" solo="false"><trackClips>
    <audioClip id="1" name="a" startPoint="0" endPoint="48000" sourceIn="0" gain="0.00"><filePath>tone.wav</filePath></audioClip>
  </trackClips></audioTrack></tracks>
</sesx>"#,
    )
    .expect("write sesx");

    let sc = dir.path().join("mix.soundcraft");
    let run = Command::new(bin).arg("convert").arg(&sesx).args(["--to", "soundcraft", "-o"]).arg(&sc).output().expect("run");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(std::fs::read(&sc).expect("read").starts_with(b"\x89CRAFT\x00\x01S"));

    let flac = dir.path().join("mix.flac");
    let run = Command::new(bin).arg("convert").arg(&sc).args(["--to", "flac", "-o"]).arg(&flac).output().expect("run");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(std::fs::read(&flac).expect("read").starts_with(b"fLaC"));

    // sessions are audio documents: video targets are rejected
    let bad = Command::new(bin).arg("convert").arg(&sesx).args(["--to", "mp4"]).current_dir(dir.path()).output().expect("run");
    assert!(!bad.status.success());
}

#[test]
fn test_cli_selects_sequence_pages_and_layers() {
    use encodercraft_pipeline::{DocumentCanvas, TimelineProject, TimelineSequence, inspect_source};
    use encodercraft_spatial_io::{CadDrawing, CadPoint3D};
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");

    // multi-sequence project: probe lists both, --sequence keeps one, unknown names fail
    let mut project = TimelineProject::new("P");
    project.add_sequence(TimelineSequence::new("Main", 1920, 1080, 25.0));
    project.add_sequence(TimelineSequence::new("Social", 1080, 1920, 30.0));
    let src = dir.path().join("show.prproj");
    std::fs::write(&src, project.to_prproj()).expect("write project");
    let probe = Command::new(bin).arg("probe").arg(&src).output().expect("run probe");
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.contains("sequence[0]: Main") && text.contains("sequence[1]: Social"), "{text}");

    let out = dir.path().join("social.prproj");
    let run = Command::new(bin).arg("encode").arg(&src).args(["--sequence", "1", "-o"]).arg(&out).output().expect("run encode");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let inv = inspect_source(&out);
    assert_eq!(inv.sequences.len(), 1);
    assert_eq!(inv.sequences[0].name, "Social");

    let bad = dir.path().join("bad.prproj");
    let run = Command::new(bin).arg("encode").arg(&src).args(["--sequence", "Missing", "-o"]).arg(&bad).output().expect("run encode");
    assert!(!run.status.success());
    assert!(!bad.exists());

    // document pages
    let mut doc = DocumentCanvas::new("Report");
    doc.pages[0].add_text(54.0, 720.0, 100.0, 20.0, "PAGE-ONE", 14.0);
    doc.add_page().add_text(54.0, 720.0, 100.0, 20.0, "PAGE-TWO", 14.0);
    let doc_src = dir.path().join("report.designcraft");
    std::fs::write(&doc_src, doc.to_designcraft_binary().expect("serialize")).expect("write doc");
    let probe = Command::new(bin).arg("probe").arg(&doc_src).output().expect("run probe");
    assert!(String::from_utf8_lossy(&probe.stdout).contains("pages: 2"));
    let out = dir.path().join("p2.pdf");
    let run = Command::new(bin).arg("convert").arg(&doc_src).args(["--to", "pdf", "--pages", "2", "-o"]).arg(&out).output().expect("run convert");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let pdf = String::from_utf8_lossy(&std::fs::read(&out).expect("read pdf")).to_string();
    assert!(pdf.contains("PAGE-TWO") && !pdf.contains("PAGE-ONE"));
    let run = Command::new(bin)
        .arg("convert")
        .arg(&doc_src)
        .args(["--to", "pdf", "--pages", "7-9", "-o"])
        .arg(dir.path().join("x.pdf"))
        .output()
        .expect("run");
    assert!(!run.status.success());
    let run = Command::new(bin).arg("convert").arg(&doc_src).args(["--to", "pdf", "--pages", "abc"]).output().expect("run");
    assert!(!run.status.success());

    // CAD layers
    let mut cad = CadDrawing::new("Plan");
    let origin = CadPoint3D::new(0.0, 0.0, 0.0);
    cad.add_line(origin, CadPoint3D::new(10.0, 0.0, 0.0), "WALLS");
    cad.add_line(origin, CadPoint3D::new(0.0, 10.0, 0.0), "FURNITURE");
    let cad_src = dir.path().join("plan.cadcraft");
    std::fs::write(&cad_src, cad.to_cadcraft_binary().expect("serialize")).expect("write cad");
    let out = dir.path().join("walls.cadcraft");
    let run = Command::new(bin).arg("encode").arg(&cad_src).args(["--layers", "WALLS", "-o"]).arg(&out).output().expect("run encode");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let back = CadDrawing::from_cadcraft_binary(&std::fs::read(&out).expect("read cad")).expect("parse cad");
    assert_eq!(back.entities.len(), 1);
}

#[test]
fn test_cli_stream_test_hands_a_stream_to_stream_encode() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("streamed.mp4");
    let endpoint = format!("encodercraft/cli-test-{}", std::process::id());
    let options = ["--endpoint", endpoint.as_str(), "--size", "128x72", "--fps", "30", "--audio", "48000:2"];

    let mut encoder = Command::new(bin).arg("stream-encode").args(options).arg("-o").arg(&out).spawn().unwrap();
    let producer = Command::new(bin).arg("stream-test").args(options).args(["--seconds", "1"]).output().unwrap();
    let status = encoder.wait().unwrap();

    assert!(producer.status.success(), "producer: {}", String::from_utf8_lossy(&producer.stderr));
    assert!(status.success());
    let probe = Command::new(bin).arg("probe").arg(&out).output().unwrap();
    let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(info["video_streams"][0]["dimensions"]["width"], 128);
    assert_eq!(info["video_streams"][0]["dimensions"]["height"], 72);
    assert_eq!(info["audio_streams"].as_array().map(Vec::len), Some(1));
    let seconds = info["duration_seconds"].as_f64().unwrap();
    assert!((seconds - 1.0).abs() < 0.1, "duration {seconds}");
}

#[test]
fn test_cli_stream_commands_reject_bad_options() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    for args in [&["stream-encode"][..], &["stream-test", "--endpoint", "e"][..], &["stream-encode", "--endpoint", "e", "--size", "64x64"][..]] {
        let output = Command::new(bin).args(args).output().unwrap();
        assert!(!output.status.success(), "{args:?}");
    }
}

#[test]
fn test_cli_lut_convert() {
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().unwrap();

    let src_cube = dir.path().join("identity.cube");
    let cube_content = "\
TITLE \"Identity 2x2x2\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    std::fs::write(&src_cube, cube_content).unwrap();

    let out_3dl = dir.path().join("converted.3dl");
    let run_3dl = Command::new(bin)
        .args(["lut-convert"])
        .arg(&src_cube)
        .arg(&out_3dl)
        .args(["--size", "4", "--tetrahedral"])
        .output()
        .expect("run lut-convert");
    assert!(run_3dl.status.success(), "{}", String::from_utf8_lossy(&run_3dl.stderr));

    let d3l_text = std::fs::read_to_string(&out_3dl).unwrap();
    assert!(d3l_text.contains("Autodesk 3DL"));

    let out_look = dir.path().join("converted.look");
    let run_look = Command::new(bin).args(["lut-convert"]).arg(&out_3dl).arg(&out_look).output().expect("run lut-convert to look");
    assert!(run_look.status.success(), "{}", String::from_utf8_lossy(&run_look.stderr));

    let look_text = std::fs::read_to_string(&out_look).unwrap();
    assert!(look_text.contains("<Look version=\"1.0\">"));
}
