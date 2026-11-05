use std::fs::File;
use std::path::{Path, PathBuf};

use encodercraft_audio_dsp::AacEncoderWrapper;
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{
    AudioSampleBuffer, ChannelLayout, ClientInfo, ContainerType, CraftJobSpec, FrameRateMode, JobPriority, OutputTarget, Rational64, Rotation,
    SourceDescriptor, StartPolicy,
};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{AudioSample, AudioStreamConfig, Mp4Muxer, VideoSample, VideoStreamConfig};
use encodercraft_pipeline::{PipelineLimits, TranscodePipeline};
use encodercraft_source::MediaReader;
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};
use tempfile::tempdir;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 72;
const FPS: Rational64 = Rational64 { num: 24, den: 1 };

fn frame(index: u64) -> encodercraft_core::VideoFrame {
    let dims = Dimensions::new(WIDTH, HEIGHT).unwrap();
    let mut frame = encodercraft_core::VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, index as i64, 1, FPS).unwrap();
    let value = if index.is_multiple_of(2) { 24 } else { 232 };
    for pixel in frame.data_mut().as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[value, value, value, 255]);
    }
    frame
}

fn write_source(path: &Path, frames: u64, with_audio: bool) {
    let mut encoder = create_video_encoder(VideoCodecKind::H264, WIDTH, HEIGHT, FPS, None).unwrap();
    let mut muxer = Mp4Muxer::new(File::create(path).unwrap(), false).unwrap();
    let video_track = muxer
        .add_video_stream(&VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(WIDTH, HEIGHT).unwrap(),
            frame_rate: FPS,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: encoder.config_record(),
        })
        .unwrap();
    for index in 0..frames {
        for packet in encoder.encode_frame(&frame(index)).unwrap() {
            muxer
                .write_video_sample(
                    video_track,
                    VideoSample { data: &packet.data, pts: packet.pts, dts: packet.dts, duration_ticks: 1, is_keyframe: packet.is_keyframe },
                )
                .unwrap();
        }
    }
    for packet in encoder.flush().unwrap() {
        muxer
            .write_video_sample(
                video_track,
                VideoSample { data: &packet.data, pts: packet.pts, dts: packet.dts, duration_ticks: 1, is_keyframe: packet.is_keyframe },
            )
            .unwrap();
    }

    if with_audio {
        let samples = usize::try_from(frames).unwrap() * 48_000 / 24;
        let mut pcm = AudioSampleBuffer::new_silent(48_000, ChannelLayout::Stereo, samples, 0).unwrap();
        for channel in 0..2 {
            for (index, sample) in pcm.channel_mut(channel).unwrap().iter_mut().enumerate() {
                *sample = (index as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.4;
            }
        }
        let mut aac = AacEncoderWrapper::new(48_000, 2, 128_000).unwrap();
        let audio_track = muxer
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
        for (index, packet) in packets.iter().enumerate() {
            muxer.write_audio_sample(audio_track, AudioSample { data: packet, pts: index as i64 * 1024, duration_ticks: 1024 }).unwrap();
        }
    }
    muxer.finish(false).unwrap();
}

fn job(source: &Path, target: OutputTarget) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "streaming-test".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![target],
    }
}

fn target(path: &Path) -> OutputTarget {
    OutputTarget {
        target_id: "target".into(),
        output_path: path.to_path_buf(),
        preset_name: "H.264".into(),
        container: ContainerType::IsoBmffMp4,
        ..Default::default()
    }
}

fn partial_outputs(dir: &Path, output_name: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            name.starts_with(&format!(".{output_name}.")) && name.ends_with(".partial")
        })
        .collect()
}

#[test]
fn rotation_blend_and_queue_limits_are_applied_to_real_output() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source, 24, false);

    let rotated_path = dir.path().join("rotated.mp4");
    let mut rotated_target = target(&rotated_path);
    rotated_target.transform.rotation = Rotation::Cw90;
    let rotated_pipeline = TranscodePipeline::new(job(&source, rotated_target));
    rotated_pipeline.execute().unwrap();
    let rotated_reader = MediaReader::open(&rotated_path).unwrap();
    let video = rotated_reader.probe().video.as_ref().unwrap();
    assert_eq!((video.width, video.height), (HEIGHT, WIDTH));
    let stats = rotated_pipeline.last_stream_stats().unwrap();
    assert_eq!(stats.frames, 24);
    assert!(stats.peak_in_flight <= PipelineLimits::max_in_flight(stats.depth));

    let blended_path = dir.path().join("blended.mp4");
    let mut blended_target = target(&blended_path);
    blended_target.custom_frame_rate = Some(Rational64 { num: 48, den: 1 });
    blended_target.transform.frame_rate_mode = FrameRateMode::Blend;
    let blended_pipeline = TranscodePipeline::new(job(&source, blended_target));
    blended_pipeline.execute().unwrap();
    let blended_reader = MediaReader::open(&blended_path).unwrap();
    assert_eq!(blended_reader.frame_count(), 48);
    let middle = blended_reader.frame_at_index(1).unwrap();
    let value = middle.data().first().copied().unwrap_or_default();
    assert!((70..=180).contains(&value), "the between-frame output should contain a blend, got {value}");
}

#[test]
fn cancellation_and_processing_failure_remove_partials_without_replacing_output() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source, 24, false);

    let output = dir.path().join("cancelled.mp4");
    std::fs::write(&output, b"keep existing output").unwrap();
    let pipeline = TranscodePipeline::new(job(&source, target(&output)));
    let progress = pipeline.progress();
    progress.set_paused(true);
    let worker = std::thread::spawn(move || pipeline.execute());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while partial_outputs(dir.path(), "cancelled.mp4").is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!partial_outputs(dir.path(), "cancelled.mp4").is_empty(), "pipeline never created its staged output");
    progress.cancel();
    assert!(matches!(worker.join().unwrap(), Err(encodercraft_core::EncoderError::Cancelled)));
    assert_eq!(std::fs::read(&output).unwrap(), b"keep existing output");
    assert!(partial_outputs(dir.path(), "cancelled.mp4").is_empty());

    let failed_output = dir.path().join("failed.mp4");
    std::fs::write(&failed_output, b"keep this too").unwrap();
    let mut failed_target = target(&failed_output);
    failed_target.lut_path = Some(dir.path().join("missing.cube"));
    let result = TranscodePipeline::new(job(&source, failed_target)).execute();
    assert!(result.is_err());
    assert_eq!(std::fs::read(&failed_output).unwrap(), b"keep this too");
    assert!(partial_outputs(dir.path(), "failed.mp4").is_empty());
}

#[test]
fn windowed_audio_stays_in_sync_and_normalization_obeys_the_memory_limit() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("av-source.mp4");
    write_source(&source, 288, true);

    let output = dir.path().join("windowed.mp4");
    let pipeline = TranscodePipeline::with_limits(job(&source, target(&output)), PipelineLimits { audio_bytes: 0, ..PipelineLimits::default() });
    pipeline.execute().unwrap();
    let output_reader = MediaReader::open(&output).unwrap();
    let probe = output_reader.probe();
    assert!(!probe.audio.is_empty());
    assert!((probe.duration_seconds - 12.0).abs() < 0.25, "audio/video duration drifted: {}", probe.duration_seconds);

    let limited_output = dir.path().join("limited.mp4");
    let mut limited_target = target(&limited_output);
    limited_target.target_lufs = Some(-16.0);
    let result =
        TranscodePipeline::with_limits(job(&source, limited_target), PipelineLimits { audio_bytes: 1, ..PipelineLimits::default() }).execute();
    assert!(result.is_err());
    assert!(!limited_output.exists());
    assert!(partial_outputs(dir.path(), "limited.mp4").is_empty());
}
