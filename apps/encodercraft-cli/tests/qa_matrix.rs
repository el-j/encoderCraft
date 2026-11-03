//! Real-media QA matrix: sources made by an independent encoder (ffmpeg) are converted through the
//! CLI and the queue, and the outputs are judged by an independent decoder (ffprobe/ffmpeg):
//! frame count, frame rate, size, audio presence and picture similarity.
//!
//! The matrix needs `ffmpeg` and `ffprobe` on `PATH`; it is skipped (loudly) when they are absent.

use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(name: &str) -> bool {
    Command::new(name).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

struct Source {
    file: &'static str,
    frames: u32,
    fps: u32,
    audio: bool,
}

/// Creates one 2 s 320x180 source; `None` when this ffmpeg build lacks the encoder.
fn make_source(dir: &Path, file: &'static str, fps: u32, args: &[&str], audio: bool) -> Option<Source> {
    let path = dir.join(file);
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i"]).arg(format!("testsrc2=size=320x180:rate={fps}:duration=2"));
    if audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"]);
    }
    cmd.args(args).arg(&path);
    let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
    ok.then_some(Source { file, frames: fps * 2, fps, audio })
}

fn probe(path: &Path, entries: &str, extra: &[&str]) -> String {
    let out = Command::new("ffprobe").args(["-v", "error"]).args(extra).args(["-show_entries", entries, "-of", "compact=p=0"]).arg(path).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn field(line: &str, key: &str) -> Option<String> {
    line.split('|').find_map(|kv| kv.strip_prefix(&format!("{key}=")).map(str::to_owned))
}

/// Mean PSNR between two videos after scaling both to 160x90.
fn psnr(a: &Path, b: &Path) -> f64 {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", "[0:v]scale=160:90,setsar=1[a];[1:v]scale=160:90,setsar=1[b];[a][b]psnr", "-f", "null", "-"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
        .unwrap_or_default();
    out.split("average:").nth(1).and_then(|s| s.split_whitespace().next()).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

fn convert(bin: &str, src: &Path, preset: &str, out: &Path) -> Result<(), String> {
    let o = Command::new(bin).arg("encode").arg(src).args(["-p", preset, "-o"]).arg(out).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).into_owned()) }
}

#[test]
fn video_matrix_outputs_are_judged_by_an_independent_decoder() {
    if !(tool("ffmpeg") && tool("ffprobe")) {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let sources: Vec<Source> = [
        make_source(dir.path(), "h264.mov", 30, &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"], true),
        make_source(dir.path(), "hevc.mp4", 30, &["-c:v", "libx265", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"], false),
        make_source(dir.path(), "vp9.webm", 30, &["-c:v", "libvpx-vp9"], false),
        make_source(dir.path(), "prores.mov", 30, &["-c:v", "prores_ks"], false),
        make_source(dir.path(), "mpeg2.ts", 25, &["-c:v", "mpeg2video"], false),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(!sources.is_empty(), "ffmpeg could not create any fixture");

    // (preset, output extension, expected video codec, expected size or None for source size)
    let presets: [(&str, &str, &str, Option<(u32, u32)>); 4] = [
        ("Match Source - High Bitrate", "mp4", "h264", Some((320, 180))),
        ("YouTube 1080p Full HD", "mp4", "h264", Some((1920, 1080))),
        ("Apple ProRes 422 HQ", "mov", "prores", Some((320, 180))),
        ("Avid DNxHR HQ 1080p", "mov", "dnxhd", Some((1920, 1080))),
    ];

    let mut failures = Vec::new();
    for s in &sources {
        let src = dir.path().join(s.file);
        for (preset, ext, codec, size) in presets {
            let out: PathBuf = dir.path().join(format!("{}-{}.{ext}", s.file, preset.replace([' ', '/'], "_")));
            let label = format!("{} -> {preset}", s.file);
            if let Err(e) = convert(bin, &src, preset, &out) {
                failures.push(format!("{label}: conversion failed: {e}"));
                continue;
            }
            let v = probe(&out, "stream=codec_name,width,height,r_frame_rate,nb_read_frames", &["-count_frames", "-select_streams", "v"]);
            let v = v.lines().next().unwrap_or_default();
            if field(v, "codec_name").as_deref() != Some(codec) {
                failures.push(format!("{label}: codec {:?}, expected {codec}", field(v, "codec_name")));
            }
            let frames: u32 = field(v, "nb_read_frames").and_then(|f| f.parse().ok()).unwrap_or(0);
            if frames != s.frames {
                failures.push(format!("{label}: {frames} frames, expected {}", s.frames));
            }
            if field(v, "r_frame_rate").as_deref() != Some(&format!("{}/1", s.fps)) {
                failures.push(format!("{label}: frame rate {:?}, expected {}/1", field(v, "r_frame_rate"), s.fps));
            }
            if let Some((w, h)) = size
                && (field(v, "width") != Some(w.to_string()) || field(v, "height") != Some(h.to_string()))
            {
                failures.push(format!("{label}: size {:?}x{:?}, expected {w}x{h}", field(v, "width"), field(v, "height")));
            }
            if s.audio && !probe(&out, "stream=codec_type", &["-select_streams", "a"]).contains("audio") {
                failures.push(format!("{label}: audio stream missing"));
            }
            let p = psnr(&out, &src);
            if p < 18.0 {
                failures.push(format!("{label}: picture differs from the source (PSNR {p:.1} dB)"));
            }
        }
    }
    assert!(failures.is_empty(), "QA matrix failures:\n{}", failures.join("\n"));
}

#[test]
fn batch_of_mixed_sources_finishes_every_item_from_one_start() {
    if !(tool("ffmpeg") && tool("ffprobe")) {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let a = make_source(dir.path(), "a.mov", 24, &["-c:v", "libx264", "-pix_fmt", "yuv420p"], false);
    let b = make_source(dir.path(), "b.webm", 30, &["-c:v", "libvpx-vp9"], false);
    let (Some(a), Some(b)) = (a, b) else {
        eprintln!("SKIPPED: encoders unavailable");
        return;
    };
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).expect("mkdir");
    let o = Command::new(bin).arg("encode").arg(dir.path().join(a.file)).arg(dir.path().join(b.file)).arg("-o").arg(&out).output().expect("run");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for (stem, frames) in [("a", a.frames), ("b", b.frames)] {
        let f = out.join(format!("{stem}_encoded.mp4"));
        let v = probe(&f, "stream=nb_read_frames", &["-count_frames", "-select_streams", "v"]);
        assert_eq!(field(v.lines().next().unwrap_or_default(), "nb_read_frames"), Some(frames.to_string()), "{stem}");
    }
}
