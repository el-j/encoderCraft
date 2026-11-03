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
    // WebM carries no audio until an Opus encoder is available.
    for (name, container, preset, audio) in [("av1.mp4", ContainerType::IsoBmffMp4, "AV1", true), ("vp9.webm", ContainerType::WebM, "VP9", false)] {
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
