//! Encode-then-decode conformance: every file the pipeline writes must decode back to real
//! pictures with the right size, rate, length and audio.

use std::fs::File;
use std::path::{Path, PathBuf};

use encodercraft_audio_dsp::AacEncoderWrapper;
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{
    AudioSampleBuffer, ChannelLayout, ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, Rational64, SourceDescriptor, StartPolicy,
    VideoFrame,
};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{AudioSample, AudioStreamConfig, Mp4Muxer, VideoSample, VideoStreamConfig};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_shared_stream::{SharedStreamHeader, SharedStreamKind, SharedStreamPublisher};
use encodercraft_source::MediaReader;
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};
use tempfile::tempdir;

const W: u32 = 128;
const H: u32 = 72;
const FRAMES: u64 = 30;

fn fps() -> Rational64 {
    Rational64 { num: 30_000, den: 1_001 }
}

/// A moving gradient so that frames differ from each other and are never flat.
fn test_frame(i: u64) -> VideoFrame {
    let dims = Dimensions::new(W, H).unwrap();
    let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, fps()).unwrap();
    let (plane, _) = frame.plane_mut(0).unwrap();
    for (n, px) in plane.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (n as u32 % W) as u64;
        let y = (n as u32 / W) as u64;
        px.copy_from_slice(&[((x * 2 + i * 6) % 256) as u8, ((y * 3 + i * 2) % 256) as u8, 180, 255]);
    }
    frame
}

/// Writes a real H.264 + AAC MP4 with `FRAMES` frames at 29.97 fps and one second of 440 Hz audio.
fn write_av_source(path: &Path) {
    let mut encoder = create_video_encoder(VideoCodecKind::H264, W, H, fps(), None).unwrap();
    let mut mux = Mp4Muxer::new(File::create(path).unwrap(), false).unwrap();
    let v = mux
        .add_video_stream(&VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(W, H).unwrap(),
            frame_rate: fps(),
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: encoder.config_record(),
        })
        .unwrap();
    for i in 0..FRAMES {
        let mut frame = test_frame(i);
        frame.pts = i as i64 * 1_001;
        for pkt in encoder.encode_frame(&frame).unwrap() {
            mux.write_video_sample(
                v,
                VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1_001, is_keyframe: pkt.is_keyframe },
            )
            .unwrap();
        }
    }
    for pkt in encoder.flush().unwrap() {
        mux.write_video_sample(v, VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1_001, is_keyframe: pkt.is_keyframe })
            .unwrap();
    }

    let mut pcm = AudioSampleBuffer::new_silent(48_000, ChannelLayout::Stereo, 48_000, 0).unwrap();
    for c in 0..2 {
        for (i, s) in pcm.channel_mut(c).unwrap().iter_mut().enumerate() {
            *s = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
        }
    }
    let mut aac = AacEncoderWrapper::new(48_000, 2, 128_000).unwrap();
    let a = mux
        .add_audio_stream(&AudioStreamConfig {
            codec_fourcc: *b"mp4a",
            sample_rate: 48_000,
            layout: ChannelLayout::Stereo,
            bits_per_sample: 16,
            codec_private: aac.audio_specific_config(),
        })
        .unwrap();
    let mut packets = aac.encode_buffer(&pcm).unwrap();
    packets.extend(aac.flush().unwrap());
    for (i, pkt) in packets.iter().enumerate() {
        mux.write_audio_sample(a, AudioSample { data: pkt, pts: i as i64 * 1024, duration_ticks: 1024 }).unwrap();
    }
    mux.finish(false).unwrap();
}

fn job(source: &Path, target: OutputTarget) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "conformance".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![target],
    }
}

fn target(path: &Path, container: ContainerType, preset: &str) -> OutputTarget {
    OutputTarget { target_id: "t".into(), output_path: path.to_path_buf(), preset_name: preset.into(), container, ..Default::default() }
}

fn mean_luma_spread(frame: &VideoFrame) -> (u8, u8) {
    let (plane, _) = frame.plane(0).unwrap();
    let mut lo = 255u8;
    let mut hi = 0u8;
    for px in plane.as_chunks::<4>().0 {
        let l = ((u32::from(px[0]) * 3 + u32::from(px[1]) * 6 + u32::from(px[2])) / 10) as u8;
        lo = lo.min(l);
        hi = hi.max(l);
    }
    (lo, hi)
}

fn assert_real_video(path: &Path, frames: u64, with_audio: bool) {
    let reader = MediaReader::open(path).unwrap();
    let probe = reader.probe();
    let video = probe.video.as_ref().unwrap();
    assert_eq!((video.width, video.height), (W, H));
    let rate = video.frame_rate.num as f64 / video.frame_rate.den as f64;
    assert!((rate - fps().as_f64()).abs() < 0.05, "frame rate {rate}");
    // Matroska stores millisecond timestamps, so its derived length may be one frame off.
    let slack = u64::from(path.extension().is_some_and(|e| e == "mkv" || e == "webm"));
    assert!(reader.frame_count().abs_diff(frames) <= slack, "frame count {} of {}, expected {frames}", reader.frame_count(), path.display());
    assert!((probe.duration_seconds - frames as f64 / fps().as_f64()).abs() < 0.1, "duration {}", probe.duration_seconds);
    assert_eq!(!probe.audio.is_empty(), with_audio);
    if with_audio {
        let audio_stream = probe.audio.first().unwrap();
        let first = i64::from(audio_stream.sample_rate / 10);
        let sample_count = usize::try_from(audio_stream.sample_rate / 50).unwrap();
        let audio = reader.read_audio_range(first, sample_count, audio_stream.sample_rate).unwrap();
        let peak = audio.channels().iter().flat_map(|channel| channel.iter()).fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak > 0.001, "audio stream in {} decoded as silence", path.display());
    }

    for index in [0, frames / 2, frames - 1] {
        let frame = reader.frame_at_index(index).unwrap();
        let (lo, hi) = mean_luma_spread(&frame);
        assert!(hi - lo > 40, "frame {index} of {} is flat (luma {lo}..{hi})", path.display());
    }
}

fn run(dir: &Path, name: &str, container: ContainerType, preset: &str, source: &Path) -> PathBuf {
    let out = dir.join(name);
    TranscodePipeline::new(job(source, target(&out, container, preset))).execute().unwrap();
    out
}

#[test]
fn av_source_round_trips_through_every_video_container() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_av_source(&source);
    assert_real_video(&source, FRAMES, true);

    for (name, container, preset) in [
        ("h264.mp4", ContainerType::IsoBmffMp4, "H.264"),
        ("hevc.mp4", ContainerType::IsoBmffMp4, "HEVC"),
        ("h264.mkv", ContainerType::MatroskaMkv, "H.264"),
    ] {
        let out = run(dir.path(), name, container, preset, &source);
        assert_real_video(&out, FRAMES, true);
    }
}

#[test]
fn av1_and_vp9_outputs_decode_to_real_pictures() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_av_source(&source);
    for (name, container, preset, audio) in [("av1.mp4", ContainerType::IsoBmffMp4, "AV1", true), ("vp9.webm", ContainerType::WebM, "VP9", true)] {
        let out = run(dir.path(), name, container, preset, &source);
        assert_real_video(&out, FRAMES, audio);
    }
}

#[test]
fn trim_keeps_only_the_requested_window() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_av_source(&source);
    let out = dir.path().join("trim.mp4");
    let mut spec = job(&source, target(&out, ContainerType::IsoBmffMp4, "H.264"));
    spec.source = SourceDescriptor::File { path: source.clone(), start_tick: Some(10), duration_ticks: Some(12), timebase: None };
    TranscodePipeline::new(spec).execute().unwrap();
    assert_real_video(&out, 12, true);
}

#[test]
fn custom_size_and_frame_rate_are_honoured() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_av_source(&source);
    let out = dir.path().join("small.mp4");
    let mut t = target(&out, ContainerType::IsoBmffMp4, "H.264");
    t.custom_width = Some(64);
    t.custom_height = Some(36);
    t.custom_frame_rate = Some(Rational64::FPS_24);
    TranscodePipeline::new(job(&source, t)).execute().unwrap();
    let reader = MediaReader::open(&out).unwrap();
    let video = reader.probe().video.clone().unwrap();
    assert_eq!((video.width, video.height), (64, 36));
    assert_eq!(video.frame_rate, Rational64::FPS_24);
    // 30 frames at 29.97 fps last ~1.001 s, i.e. 24 frames at 24 fps.
    assert_eq!(reader.frame_count(), 24);
}

#[test]
fn undecodable_and_missing_sources_are_errors_not_placeholders() {
    let dir = tempdir().unwrap();
    let junk = dir.path().join("junk.mp4");
    std::fs::write(&junk, b"this is not a movie").unwrap();
    let out = dir.path().join("out.mp4");
    assert!(TranscodePipeline::new(job(&junk, target(&out, ContainerType::IsoBmffMp4, "H.264"))).execute().is_err());
    let missing = dir.path().join("missing.mov");
    assert!(TranscodePipeline::new(job(&missing, target(&out, ContainerType::IsoBmffMp4, "H.264"))).execute().is_err());
}

#[test]
fn writing_a_buffered_wav_uses_real_samples() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_av_source(&source);
    let out = dir.path().join("audio.wav");
    TranscodePipeline::new(job(&source, target(&out, ContainerType::Wave, "WAV"))).execute().unwrap();
    let reader = MediaReader::open(&out).unwrap();
    let audio = reader.read_all_audio(48_000).unwrap();
    let peak = audio.channel(0).unwrap().iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.2, "audio is silent (peak {peak})");
}

#[test]
fn still_images_keep_their_pixels_across_formats() {
    let dir = tempdir().unwrap();
    let bmp = dir.path().join("source.bmp");
    let frame = test_frame(3);
    std::fs::write(&bmp, encodercraft_image_codecs::encode_bmp(&frame).unwrap()).unwrap();
    let expected = MediaReader::open(&bmp).unwrap().frame_at_index(0).unwrap();

    for (name, container) in [("out.png", ContainerType::Png), ("out.tif", ContainerType::Tiff), ("out.bmp", ContainerType::Bmp)] {
        let out = run(dir.path(), name, container, "Image", &bmp);
        let decoded = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
        assert_eq!(decoded.dimensions, expected.dimensions, "{name}");
        let (a, _) = decoded.plane(0).unwrap();
        let (b, _) = expected.plane(0).unwrap();
        assert_eq!(a, b, "{name} pixels differ from the source");
        let (lo, hi) = mean_luma_spread(&decoded);
        assert!(hi - lo > 40, "{name} is flat");
    }
}

#[test]
fn lossy_still_formats_decode_close_to_the_source_and_never_grey() {
    let dir = tempdir().unwrap();
    let png = dir.path().join("source.png");
    let frame = test_frame(5);
    std::fs::write(&png, encodercraft_image_codecs::encode_png(&frame).unwrap()).unwrap();
    let (src, _) = frame.plane(0).unwrap();

    for (name, container) in [("out.jpg", ContainerType::Jpeg), ("out.webp", ContainerType::WebP)] {
        let out = run(dir.path(), name, container, "Image", &png);
        let decoded = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
        assert_eq!(decoded.dimensions, frame.dimensions, "{name}");
        let (got, _) = decoded.plane(0).unwrap();
        let total: u64 = got.iter().zip(src.iter()).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
        let mean = total as f64 / got.len() as f64;
        assert!(mean < 12.0, "{name} mean error {mean}");
        let (lo, hi) = mean_luma_spread(&decoded);
        assert!(hi - lo > 40, "{name} is flat");
    }
}

#[test]
fn truncated_still_is_an_error_and_leaves_no_output() {
    let dir = tempdir().unwrap();
    let png = dir.path().join("source.png");
    let bytes = encodercraft_image_codecs::encode_png(&test_frame(1)).unwrap();
    std::fs::write(&png, &bytes[..bytes.len() / 3]).unwrap();
    let out = dir.path().join("out.png");
    let result = TranscodePipeline::new(job(&png, target(&out, ContainerType::Png, "Image"))).execute();
    assert!(result.is_err());
    assert!(!out.exists());
}

#[test]
fn failure_names_the_target_that_failed() {
    let dir = tempdir().unwrap();
    let png = dir.path().join("source.png");
    std::fs::write(&png, encodercraft_image_codecs::encode_png(&test_frame(1)).unwrap()).unwrap();
    let mut spec = job(&png, target(&dir.path().join("ok.png"), ContainerType::Png, "Good"));
    spec.targets.push(target(&dir.path().join("ok.png").join("bad.png"), ContainerType::Png, "Bad"));
    let pipeline = TranscodePipeline::new(spec);
    let err = pipeline.execute().unwrap_err().to_string();
    assert!(err.contains("target 2 of 2") && err.contains("Bad"), "{err}");
    assert_eq!(pipeline.progress().target_index(), 1);
    assert!(dir.path().join("ok.png").exists());
}

/// Largest per-channel mean error accepted for a round trip through the given format.
fn tolerance(container: ContainerType) -> f64 {
    match container {
        ContainerType::Jpeg | ContainerType::WebP => 12.0,
        ContainerType::AnimatedGif => 25.0,
        ContainerType::OpenExr => 2.0,
        _ => 0.0,
    }
}

#[test]
fn every_still_input_converts_to_every_still_output_with_matching_pixels() {
    let dir = tempdir().unwrap();
    let base = dir.path().join("base.bmp");
    std::fs::write(&base, encodercraft_image_codecs::encode_bmp(&test_frame(3)).unwrap()).unwrap();
    let reference = MediaReader::open(&base).unwrap().frame_at_index(0).unwrap();
    let (expected, _) = reference.plane(0).unwrap();
    let expected = expected.to_vec();

    let formats = [
        ("png", ContainerType::Png),
        ("tif", ContainerType::Tiff),
        ("bmp", ContainerType::Bmp),
        ("jpg", ContainerType::Jpeg),
        ("webp", ContainerType::WebP),
        ("exr", ContainerType::OpenExr),
        ("gif", ContainerType::AnimatedGif),
    ];

    for (in_ext, in_container) in formats {
        let input = run(dir.path(), &format!("in.{in_ext}"), in_container, "Image", &base);
        let input_loss = tolerance(in_container);
        for (out_ext, out_container) in formats {
            let out = run(dir.path(), &format!("{in_ext}_to.{out_ext}"), out_container, "Image", &input);
            let decoded = MediaReader::open(&out).unwrap().frame_at_index(0).unwrap();
            let label = format!("{in_ext} -> {out_ext}");
            assert_eq!(decoded.dimensions, reference.dimensions, "{label}");
            let (got, _) = decoded.plane(0).unwrap();
            assert_eq!(got.len(), expected.len(), "{label}");
            let total: u64 = got.iter().zip(expected.iter()).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
            let mean = total as f64 / got.len().max(1) as f64;
            let allowed = input_loss + tolerance(out_container);
            assert!(mean <= allowed, "{label}: mean error {mean} exceeds {allowed}");
            let (lo, hi) = mean_luma_spread(&decoded);
            assert!(hi - lo > 40, "{label} is flat (grey-image regression)");
        }
    }
}

#[test]
fn truncated_stills_of_every_format_fail_cleanly() {
    let dir = tempdir().unwrap();
    let base = dir.path().join("base.bmp");
    std::fs::write(&base, encodercraft_image_codecs::encode_bmp(&test_frame(2)).unwrap()).unwrap();

    for (ext, container) in [
        ("png", ContainerType::Png),
        ("tif", ContainerType::Tiff),
        ("bmp", ContainerType::Bmp),
        ("jpg", ContainerType::Jpeg),
        ("webp", ContainerType::WebP),
        ("exr", ContainerType::OpenExr),
        ("gif", ContainerType::AnimatedGif),
    ] {
        let good = run(dir.path(), &format!("good.{ext}"), container, "Image", &base);
        let bytes = std::fs::read(&good).unwrap();
        let cut = dir.path().join(format!("cut.{ext}"));
        std::fs::write(&cut, &bytes[..bytes.len() / 4]).unwrap();
        let out = dir.path().join(format!("cut_out_{ext}.png"));
        let result = TranscodePipeline::new(job(&cut, target(&out, ContainerType::Png, "Image"))).execute();
        assert!(result.is_err(), "{ext}: truncated file converted without error");
        assert!(!out.exists(), "{ext}: left output behind");
    }
}

fn write_sequence(dir: &Path, count: u64, skip: Option<u64>) {
    for i in 1..=count {
        if skip == Some(i) {
            continue;
        }
        std::fs::write(dir.join(format!("shot_{i:04}.png")), encodercraft_image_codecs::encode_png(&test_frame(i * 5)).unwrap()).unwrap();
    }
}

#[test]
fn numbered_image_sequence_becomes_a_video_at_the_requested_rate() {
    let dir = tempdir().unwrap();
    write_sequence(dir.path(), 12, None);
    let pattern = dir.path().join("shot_%04d.png");
    let out = dir.path().join("seq.mp4");
    let mut t = target(&out, ContainerType::IsoBmffMp4, "H.264");
    t.custom_frame_rate = Some(Rational64::FPS_25);
    TranscodePipeline::new(job(&pattern, t)).execute().unwrap();

    let reader = MediaReader::open(&out).unwrap();
    let video = reader.probe().video.clone().unwrap();
    assert_eq!((video.width, video.height), (W, H));
    assert!((video.frame_rate.num as f64 / video.frame_rate.den as f64 - 25.0).abs() < 0.05);
    assert_eq!(reader.frame_count(), 12);
    let first = mean_luma_spread(&reader.frame_at_index(0).unwrap());
    let last = mean_luma_spread(&reader.frame_at_index(11).unwrap());
    assert!(first.1 - first.0 > 40 && last.1 - last.0 > 40, "sequence frames are flat");
}

#[test]
fn image_sequence_keeps_its_length_across_a_gap_and_rejects_empty_patterns() {
    let dir = tempdir().unwrap();
    write_sequence(dir.path(), 8, Some(4));
    let reader = MediaReader::open(&dir.path().join("shot_%04d.png")).unwrap();
    assert_eq!(reader.frame_count(), 8, "a missing frame must not shorten the sequence");

    let err = MediaReader::open(&dir.path().join("none_%04d.png"));
    assert!(err.is_err());
    assert!(encodercraft_source::is_sequence_pattern(&dir.path().join("a_%03d.exr")));
    assert!(!encodercraft_source::is_sequence_pattern(&dir.path().join("a_001.exr")));
}

#[test]
fn video_exports_a_numbered_still_sequence_for_the_trimmed_range() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("src.mp4");
    write_av_source(&source);
    let frames_dir = dir.path().join("frames");
    std::fs::create_dir(&frames_dir).unwrap();
    let out = frames_dir.join("f_%04d.png");

    let mut spec = job(&source, target(&out, ContainerType::Png, "Image"));
    if let SourceDescriptor::File { start_tick, duration_ticks, .. } = &mut spec.source {
        *start_tick = Some(5);
        *duration_ticks = Some(10);
    }
    TranscodePipeline::new(spec).execute().unwrap();

    let mut names: Vec<String> = std::fs::read_dir(&frames_dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names.len(), 10, "{names:?}");
    assert_eq!(names.first().map(String::as_str), Some("f_0001.png"));
    assert_eq!(names.last().map(String::as_str), Some("f_0010.png"));
    for name in [&names[0], &names[9]] {
        let frame = MediaReader::open(&frames_dir.join(name)).unwrap().frame_at_index(0).unwrap();
        let (lo, hi) = mean_luma_spread(&frame);
        assert!(hi - lo > 40, "{name} is flat");
    }
}

/// A sibling application streams raw frames and PCM over shared memory; the job encodes them.
#[test]
fn shared_memory_stream_encodes_to_a_real_mp4() {
    let dir = tempdir().unwrap();
    let endpoint = format!("encodercraft/conformance-{}", std::process::id());
    let producer_endpoint = endpoint.clone();
    let producer = std::thread::spawn(move || {
        let publisher = SharedStreamPublisher::create(&producer_endpoint, 48_000 * 2 * 4).unwrap();
        assert!(publisher.wait_for_subscriber(std::time::Duration::from_secs(30)));
        for i in 0..FRAMES {
            let frame = test_frame(i);
            let (pixels, _) = frame.plane(0).unwrap();
            publisher
                .publish_with(
                    SharedStreamHeader { kind: SharedStreamKind::VideoFrame, pts: i as i64, duration: 1, payload_bytes: pixels.len() as u64 },
                    |payload| {
                        payload.copy_from_slice(pixels);
                        Ok(())
                    },
                )
                .unwrap();
        }
        let mut pcm = Vec::with_capacity(48_000 * 2 * 4);
        for i in 0..48_000 {
            let sample = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
            pcm.extend_from_slice(&sample.to_le_bytes());
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        publisher
            .publish_with(
                SharedStreamHeader { kind: SharedStreamKind::AudioSamples, pts: 0, duration: 48_000, payload_bytes: pcm.len() as u64 },
                |payload| {
                    payload.copy_from_slice(&pcm);
                    Ok(())
                },
            )
            .unwrap();
        assert!(publisher.finish(std::time::Duration::from_secs(30)).unwrap());
    });

    let mut spec = job(Path::new("unused"), target(&dir.path().join("piped.mp4"), ContainerType::IsoBmffMp4, "H.264"));
    spec.source = SourceDescriptor::Pipe { endpoint, width: W, height: H, frame_rate: fps(), sample_rate: 48_000, channels: 2 };
    TranscodePipeline::new(spec).execute().unwrap();
    producer.join().unwrap();

    assert_real_video(&dir.path().join("piped.mp4"), FRAMES, true);
}
