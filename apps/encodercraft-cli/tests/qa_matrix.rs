//! Generated-media QA matrix: deterministic sources made by FFmpeg are converted through the
//! CLI and the queue, and the outputs are judged by an independent decoder (ffprobe/ffmpeg):
//! frame count, frame rate, size, audio presence and picture similarity.
//!
//! This is not a substitute for checking actual phone or screen recordings. The matrix needs
//! `ffmpeg` and `ffprobe` on `PATH`; it is skipped (loudly) when they are absent.

use std::path::{Path, PathBuf};
use std::process::Command;

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ClientInfo, CraftJobSpec, JobPriority, Rational64, SourceDescriptor, StartPolicy, VideoFrame};
use encodercraft_engine::QueueManager;
use encodercraft_geom::Dimensions;
use encodercraft_image_codecs::{StillFormat, StillOptions, encode_still};
use encodercraft_presets::{PresetRegistry, evaluate_preset};

fn tool(name: &str) -> bool {
    Command::new(name).arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn full_qa_required() -> bool {
    std::env::var_os("ENCODERCRAFT_REQUIRE_FULL_QA").is_some()
}

fn require_media_tools() -> bool {
    let available = tool("ffmpeg") && tool("ffprobe");
    assert!(available || !full_qa_required(), "full real-media QA requires ffmpeg and ffprobe on PATH");
    if !available {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
    }
    available
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

fn probe(path: &Path, entries: &str, extra: &[&str]) -> Result<String, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(extra)
        .args(["-show_entries", entries, "-of", "compact=p=0"])
        .arg(path)
        .output()
        .map_err(|error| format!("could not run ffprobe: {error}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn field(line: &str, key: &str) -> Option<String> {
    line.split('|').find_map(|kv| kv.strip_prefix(&format!("{key}=")).map(str::to_owned))
}

/// Mean PSNR between two videos after scaling both to 160x90.
fn psnr(a: &Path, b: &Path) -> Result<f64, String> {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(a)
        .arg("-i")
        .arg(b)
        .args(["-lavfi", "[0:v]scale=160:90,setsar=1[a];[1:v]scale=160:90,setsar=1[b];[a][b]psnr", "-f", "null", "-"])
        .output()
        .map_err(|error| format!("could not run ffmpeg PSNR comparison: {error}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    String::from_utf8_lossy(&out.stderr)
        .split("average:")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "ffmpeg did not report a PSNR average".into())
}

fn audio_mean_volume(path: &Path) -> Result<f64, String> {
    let out = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(path)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .map_err(|error| format!("could not run ffmpeg audio analysis: {error}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    String::from_utf8_lossy(&out.stderr)
        .split("mean_volume:")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "ffmpeg did not report a mean audio volume".into())
}

fn convert(bin: &str, src: &Path, preset: &str, out: &Path, config_root: &Path) -> Result<(), String> {
    let o = Command::new(bin)
        .arg("encode")
        .arg(src)
        .args(["-p", preset, "-o"])
        .arg(out)
        .env("XDG_CONFIG_HOME", config_root.join("xdg"))
        .env("HOME", config_root.join("home"))
        .env("APPDATA", config_root.join("appdata"))
        .output()
        .map_err(|e| e.to_string())?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).into_owned()) }
}

fn seed_conversion_approvals(sources: &[PathBuf], presets: &[(&str, &str, &str, Option<(u32, u32)>)], config_root: &Path, output_dir: &Path) {
    let preferences_path = if cfg!(windows) {
        config_root.join("appdata/encoderCraft/preferences.json")
    } else if cfg!(target_os = "macos") {
        config_root.join("home/Library/Application Support/encoderCraft/preferences.json")
    } else {
        config_root.join("xdg/encoderCraft/preferences.json")
    };
    let queue = QueueManager::new_in_memory();
    queue.set_conversion_preferences_path(preferences_path);
    let registry = PresetRegistry::default();

    for (source_index, source_path) in sources.iter().enumerate() {
        let media_info = encodercraft_source::probe_media_info(source_path).expect("probe QA media source");
        for (preset_index, (name, ext, _, _)) in presets.iter().enumerate() {
            let preset = registry.find_by_name(name).expect("known factory preset");
            let target = evaluate_preset(preset, &media_info, output_dir.join(format!("approval-{source_index}-{preset_index}.{ext}")))
                .expect("evaluate QA preset");
            let spec = CraftJobSpec {
                version: "1.0".into(),
                job_id: format!("qa-approval-{source_index}-{preset_index}"),
                client_info: ClientInfo { app_name: "QA".into(), app_version: "test".into(), pid: std::process::id(), user_notes: None },
                priority: JobPriority::Normal,
                start_policy: StartPolicy::AutoStart,
                source: SourceDescriptor::File { path: source_path.clone(), start_tick: None, duration_ticks: None, timebase: None },
                targets: vec![target],
            };
            for conversion in queue.conversion_review(&spec).expect("assess QA conversion") {
                queue.save_conversion_decision(conversion.key, true).expect("save QA approval");
            }
        }
    }
}

fn make_audio_source(dir: &Path, file: &'static str, codec: &str) -> Option<PathBuf> {
    let path = dir.join(file);
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=2", "-ac", "2", "-c:a", codec])
        .arg(&path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    ok.then_some(path)
}

fn make_image_source(dir: &Path, file: &'static str, codec: &str) -> Option<PathBuf> {
    let path = dir.join(file);
    let ok = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x32:rate=1:duration=1",
            "-frames:v",
            "1",
            "-c:v",
            codec,
            "-pix_fmt",
            "rgb24",
        ])
        .arg(&path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    ok.then_some(path)
}

fn make_webp_source(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("webp.webp");
    let dimensions = Dimensions::new(64, 32).map_err(|error| error.to_string())?;
    let mut frame = VideoFrame::new_allocated(dimensions, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)
        .map_err(|error| error.to_string())?;
    for (index, pixel) in frame.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = u8::try_from(index % 64).map_err(|error| error.to_string())?;
        let y = u8::try_from(index / 64).map_err(|error| error.to_string())?;
        pixel.copy_from_slice(&[x.saturating_mul(4), y.saturating_mul(8), 128, 255]);
    }
    let bytes = encode_still(&frame, StillFormat::WebP, &StillOptions { webp_lossless: true, ..StillOptions::default() })
        .map_err(|error| error.to_string())?;
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn duration_seconds(path: &Path) -> Result<f64, String> {
    probe(path, "format=duration", &[])?
        .lines()
        .next()
        .and_then(|line| field(line, "duration"))
        .and_then(|duration| duration.parse().ok())
        .ok_or_else(|| format!("ffprobe did not report a duration for {}", path.display()))
}

#[test]
fn video_matrix_outputs_are_judged_by_an_independent_decoder() {
    if !require_media_tools() {
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
    let expected_sources = ["h264.mov", "hevc.mp4", "vp9.webm", "prores.mov", "mpeg2.ts"];
    let missing_sources: Vec<_> = expected_sources.iter().filter(|name| !sources.iter().any(|source| source.file == **name)).collect();
    assert!(missing_sources.is_empty() || !full_qa_required(), "FFmpeg could not create required QA fixtures: {missing_sources:?}");
    assert!(!sources.is_empty(), "ffmpeg could not create any fixture");

    // (preset, output extension, expected video codec, expected size or None for source size)
    let presets: [(&str, &str, &str, Option<(u32, u32)>); 4] = [
        ("Match Source - High Bitrate", "mp4", "h264", Some((320, 180))),
        ("YouTube 1080p Full HD", "mp4", "h264", Some((1920, 1080))),
        ("Apple ProRes 422 HQ", "mov", "prores", Some((320, 180))),
        ("Avid DNxHR HQ 1080p", "mov", "dnxhd", Some((1920, 1080))),
    ];

    let config_root = dir.path().join("cli-config");
    let source_paths: Vec<_> = sources.iter().map(|source| dir.path().join(source.file)).collect();
    seed_conversion_approvals(&source_paths, &presets, &config_root, dir.path());
    let mut failures = Vec::new();
    for s in &sources {
        let src = dir.path().join(s.file);
        let media_info = encodercraft_source::probe_media_info(&src).expect("probe QA media source");
        for (preset, ext, codec, size) in presets {
            let out: PathBuf = dir.path().join(format!("{}-{}.{ext}", s.file, preset.replace([' ', '/'], "_")));
            let label = format!("{} -> {preset}", s.file);
            if let Err(e) = convert(bin, &src, preset, &out, &config_root) {
                failures.push(format!("{label}: conversion failed: {e}"));
                continue;
            }
            let v = match probe(&out, "stream=codec_name,width,height,r_frame_rate,nb_read_frames", &["-count_frames", "-select_streams", "v"]) {
                Ok(probe) => probe,
                Err(error) => {
                    failures.push(format!("{label}: independent video probe failed: {error}"));
                    continue;
                }
            };
            let v = v.lines().next().unwrap_or_default();
            let expected_codec =
                if preset == "Match Source - High Bitrate" && media_info.video_streams.first().is_some_and(|stream| stream.bit_depth > 8) {
                    "hevc"
                } else {
                    codec
                };
            if field(v, "codec_name").as_deref() != Some(expected_codec) {
                failures.push(format!("{label}: codec {:?}, expected {expected_codec}", field(v, "codec_name")));
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
            if s.audio {
                match probe(&out, "stream=codec_name,sample_rate,channels", &["-select_streams", "a"]) {
                    Ok(audio_probe) if !audio_probe.trim().is_empty() => {
                        let audio_stream = audio_probe.lines().next().unwrap_or_default();
                        let expected_audio_codec = if matches!(preset, "Apple ProRes 422 HQ" | "Avid DNxHR HQ 1080p") { "pcm_s24le" } else { "aac" };
                        if field(audio_stream, "codec_name").as_deref() != Some(expected_audio_codec) {
                            failures.push(format!("{label}: audio codec {:?}, expected {expected_audio_codec}", field(audio_stream, "codec_name")));
                        }
                        let expected_rate = if preset == "Match Source - High Bitrate" { "44100" } else { "48000" };
                        if field(audio_stream, "sample_rate").as_deref() != Some(expected_rate) {
                            failures.push(format!("{label}: audio sample rate {:?}, expected {expected_rate}", field(audio_stream, "sample_rate")));
                        }
                        let expected_channels =
                            if matches!(preset, "Match Source - High Bitrate" | "Apple ProRes 422 HQ" | "Avid DNxHR HQ 1080p") { "1" } else { "2" };
                        if field(audio_stream, "channels").as_deref() != Some(expected_channels) {
                            failures
                                .push(format!("{label}: audio channel count {:?}, expected {expected_channels}", field(audio_stream, "channels")));
                        }
                    }
                    Ok(_) => failures.push(format!("{label}: audio stream missing")),
                    Err(error) => failures.push(format!("{label}: independent audio probe failed: {error}")),
                }
            }
            match psnr(&out, &src) {
                Ok(p) if p >= 18.0 => {}
                Ok(p) => failures.push(format!("{label}: picture differs from the source (PSNR {p:.1} dB)")),
                Err(error) => failures.push(format!("{label}: independent picture comparison failed: {error}")),
            }
            match duration_seconds(&out) {
                Ok(duration) if (duration - 2.0).abs() <= 0.1 => {}
                Ok(duration) => failures.push(format!("{label}: output duration {duration:.3}s, expected 2.000s")),
                Err(error) => failures.push(format!("{label}: {error}")),
            }
        }
    }
    assert!(failures.is_empty(), "QA matrix failures:\n{}", failures.join("\n"));
}

#[test]
fn audio_matrix_preserves_duration_channels_and_audible_signal() {
    if !require_media_tools() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let sources: Vec<_> = [
        make_audio_source(dir.path(), "source.wav", "pcm_s16le"),
        make_audio_source(dir.path(), "source.mp3", "libmp3lame"),
        make_audio_source(dir.path(), "source.flac", "flac"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let expected_sources = ["source.wav", "source.mp3", "source.flac"];
    let missing_sources: Vec<_> = expected_sources
        .iter()
        .filter(|name| !sources.iter().any(|source| source.file_name().and_then(|name| name.to_str()) == Some(**name)))
        .collect();
    assert!(missing_sources.is_empty() || !full_qa_required(), "FFmpeg could not create required audio fixtures: {missing_sources:?}");
    assert!(!sources.is_empty(), "ffmpeg could not create any audio fixture");

    let presets = [
        ("Broadcast Wave BWF (-23 LUFS)", "wav", "pcm_s24le", None),
        ("Podcast Master (-16 LUFS)", "wav", "pcm_s16le", None),
        ("Lossless FLAC Master (24-bit 48kHz)", "flac", "flac", None),
    ];
    let config_root = dir.path().join("audio-cli-config");
    seed_conversion_approvals(&sources, &presets, &config_root, dir.path());

    let mut failures = Vec::new();
    for source in &sources {
        for (preset, ext, expected_codec, _) in presets {
            let output = dir
                .path()
                .join(format!("{}-{preset}.{ext}", source.file_stem().and_then(|stem| stem.to_str()).unwrap_or("source").replace([' ', '/'], "_")));
            let label = format!("{} -> {preset}", source.display());
            if let Err(error) = convert(env!("CARGO_BIN_EXE_encodercraft-cli"), source, preset, &output, &config_root) {
                failures.push(format!("{label}: conversion failed: {error}"));
                continue;
            }
            let audio = match probe(&output, "stream=codec_name,sample_rate,channels", &["-select_streams", "a"]) {
                Ok(audio) => audio,
                Err(error) => {
                    failures.push(format!("{label}: independent audio probe failed: {error}"));
                    continue;
                }
            };
            let audio = audio.lines().next().unwrap_or_default();
            if field(audio, "codec_name").as_deref() != Some(expected_codec) {
                failures.push(format!("{label}: audio codec {:?}, expected {expected_codec}", field(audio, "codec_name")));
            }
            if field(audio, "channels").as_deref() != Some("2") {
                failures.push(format!("{label}: expected stereo audio, got {:?}", field(audio, "channels")));
            }
            let expected_rate = if preset.starts_with("Podcast") { "44100" } else { "48000" };
            if field(audio, "sample_rate").as_deref() != Some(expected_rate) {
                failures.push(format!("{label}: sample rate {:?}, expected {expected_rate}", field(audio, "sample_rate")));
            }
            match duration_seconds(&output) {
                Ok(duration) if (duration - 2.0).abs() <= 0.1 => {}
                Ok(duration) => failures.push(format!("{label}: output duration {duration:.3}s, expected 2.000s")),
                Err(error) => failures.push(format!("{label}: {error}")),
            }
            match audio_mean_volume(&output) {
                Ok(volume) if volume > -60.0 => {}
                Ok(volume) => failures.push(format!("{label}: output is unexpectedly silent ({volume:.1} dB mean)")),
                Err(error) => failures.push(format!("{label}: independent audio analysis failed: {error}")),
            }
        }
    }
    assert!(failures.is_empty(), "audio QA matrix failures:\n{}", failures.join("\n"));
}

#[test]
fn still_image_matrix_roundtrips_with_independent_decode_and_similarity() {
    if !require_media_tools() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let mut sources: Vec<_> = [
        make_image_source(dir.path(), "source.png", "png"),
        make_image_source(dir.path(), "source.jpg", "mjpeg"),
        make_image_source(dir.path(), "source.tif", "tiff"),
        make_image_source(dir.path(), "source.exr", "exr"),
    ]
    .into_iter()
    .flatten()
    .collect();
    sources.push(make_webp_source(dir.path()).expect("create WebP test source"));
    let expected_sources = ["source.png", "source.jpg", "source.tif", "source.exr", "webp.webp"];
    let missing_sources: Vec<_> = expected_sources
        .iter()
        .filter(|name| !sources.iter().any(|source| source.file_name().and_then(|name| name.to_str()) == Some(**name)))
        .collect();
    assert!(missing_sources.is_empty() || !full_qa_required(), "FFmpeg could not create required image fixtures: {missing_sources:?}");
    assert_eq!(sources.len(), expected_sources.len(), "could not create all still-image fixtures");

    let presets = [
        ("Still Image PNG (Lossless)", "png", "png", Some((64, 32))),
        ("Still Image JPEG", "jpg", "mjpeg", Some((64, 32))),
        ("Broadcast TIFF (16-bit RGB)", "tif", "tiff", Some((64, 32))),
        ("VFX OpenEXR (Half Float)", "exr", "exr", Some((64, 32))),
        ("Still Image WebP (Lossless)", "webp", "webp", Some((64, 32))),
    ];
    let config_root = dir.path().join("image-cli-config");
    seed_conversion_approvals(&sources, &presets, &config_root, dir.path());

    let mut failures = Vec::new();
    for source in &sources {
        for (preset, ext, expected_codec, size) in presets {
            let output = dir
                .path()
                .join(format!("{}-{preset}.{ext}", source.file_stem().and_then(|stem| stem.to_str()).unwrap_or("source").replace([' ', '/'], "_")));
            let label = format!("{} -> {preset}", source.display());
            if let Err(error) = convert(env!("CARGO_BIN_EXE_encodercraft-cli"), source, preset, &output, &config_root) {
                failures.push(format!("{label}: conversion failed: {error}"));
                continue;
            }
            let video = match probe(&output, "stream=codec_name,width,height", &["-select_streams", "v"]) {
                Ok(video) => video,
                Err(error) => {
                    failures.push(format!("{label}: independent image probe failed: {error}"));
                    continue;
                }
            };
            let video = video.lines().next().unwrap_or_default();
            if field(video, "codec_name").as_deref() != Some(expected_codec) {
                failures.push(format!("{label}: image codec {:?}, expected {expected_codec}", field(video, "codec_name")));
            }
            if let Some((width, height)) = size
                && (field(video, "width") != Some(width.to_string()) || field(video, "height") != Some(height.to_string()))
            {
                failures.push(format!("{label}: dimensions {:?}x{:?}, expected {width}x{height}", field(video, "width"), field(video, "height")));
            }
            match psnr(&output, source) {
                Ok(value) if value >= 14.0 => {}
                Ok(value) => failures.push(format!("{label}: image differs from the source (PSNR {value:.1} dB)")),
                Err(error) => failures.push(format!("{label}: independent image comparison failed: {error}")),
            }
        }
    }
    assert!(failures.is_empty(), "still-image QA matrix failures:\n{}", failures.join("\n"));
}

#[test]
fn batch_of_mixed_sources_finishes_every_item_from_one_start() {
    if !require_media_tools() {
        return;
    }
    let bin = env!("CARGO_BIN_EXE_encodercraft-cli");
    let dir = tempfile::tempdir().expect("tempdir");
    let a = make_source(dir.path(), "a.mov", 24, &["-c:v", "libx264", "-pix_fmt", "yuv420p"], false);
    let b = make_source(dir.path(), "b.webm", 30, &["-c:v", "libvpx-vp9"], false);
    let (Some(a), Some(b)) = (a, b) else {
        assert!(!full_qa_required(), "FFmpeg could not create the required mixed-batch fixtures");
        eprintln!("SKIPPED: encoders unavailable");
        return;
    };
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).expect("mkdir");
    let o = Command::new(bin).arg("encode").arg(dir.path().join(a.file)).arg(dir.path().join(b.file)).arg("-o").arg(&out).output().expect("run");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    for (stem, frames) in [("a", a.frames), ("b", b.frames)] {
        let f = out.join(format!("{stem}_encoded.mp4"));
        let v = probe(&f, "stream=nb_read_frames", &["-count_frames", "-select_streams", "v"]).expect("independent batch output probe");
        assert_eq!(field(v.lines().next().unwrap_or_default(), "nb_read_frames"), Some(frames.to_string()), "{stem}");
    }
}
