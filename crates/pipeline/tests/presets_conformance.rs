//! Every video preset in the registry, and every container x codec pair, is encoded from a real
//! source and the output is decoded again to check size, frame count, duration and fidelity.

use std::fs::File;
use std::path::Path;

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{
    ClientInfo, ColorTag, ContainerType, CraftJobSpec, JobPriority, OutputTarget, Rational64, SourceDescriptor, StartPolicy, VideoEncodeSettings,
    VideoFrame,
};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{Mp4Muxer, VideoSample, VideoStreamConfig};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_presets::PresetRegistry;
use encodercraft_source::MediaReader;
use encodercraft_video_codecs::capabilities::codecs_for_container;
use encodercraft_video_codecs::{EncodeParams, VideoCodecKind, create_video_encoder, create_video_encoder_with};
use tempfile::tempdir;

const W: u32 = 128;
const H: u32 = 72;
const FRAMES: u64 = 12;

fn fps() -> Rational64 {
    Rational64 { num: 30_000, den: 1_001 }
}

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

fn write_source(path: &Path) {
    let mut encoder = create_video_encoder(VideoCodecKind::H264, W, H, fps(), Some(3000)).unwrap();
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
    let mut packets = Vec::new();
    for i in 0..FRAMES {
        let mut frame = test_frame(i);
        frame.pts = i as i64 * 1_001;
        packets.extend(encoder.encode_frame(&frame).unwrap());
    }
    packets.extend(encoder.flush().unwrap());
    for pkt in packets {
        mux.write_video_sample(v, VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1_001, is_keyframe: pkt.is_keyframe })
            .unwrap();
    }
    mux.finish(false).unwrap();
}

fn job(source: &Path, target: OutputTarget) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "preset-conformance".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![target],
    }
}

fn small_target(path: &Path, container: ContainerType, preset: &str) -> OutputTarget {
    OutputTarget {
        target_id: "t".into(),
        output_path: path.to_path_buf(),
        preset_name: preset.into(),
        container,
        custom_width: Some(W),
        custom_height: Some(H),
        ..Default::default()
    }
}

fn luma(frame: &VideoFrame) -> Vec<u8> {
    let (plane, _) = frame.plane(0).unwrap();
    plane
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])).round().clamp(0.0, 255.0) as u8)
        .collect()
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum::<f64>() / a.len().max(1) as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// Decodes `out` and checks dimensions, frame count, duration and PSNR against the decoded source.
fn assert_matches_source(label: &str, source: &Path, out: &Path) {
    let src = MediaReader::open(source).unwrap();
    let reader = MediaReader::open(out).unwrap_or_else(|e| panic!("{label}: output does not open: {e}"));
    let probe = reader.probe();
    let video = probe.video.as_ref().unwrap_or_else(|| panic!("{label}: no video stream"));
    assert_eq!((video.width, video.height), (W, H), "{label}: dimensions");
    assert_eq!(reader.frame_count(), FRAMES, "{label}: frame count");
    let expected = FRAMES as f64 * 1_001.0 / 30_000.0;
    assert!((probe.duration_seconds - expected).abs() <= 1_001.0 / 30_000.0 + 1e-3, "{label}: duration {} vs {expected}", probe.duration_seconds);
    for index in [0, FRAMES - 1] {
        let got = reader.frame_at_index(index).unwrap_or_else(|e| panic!("{label}: decode frame {index}: {e}"));
        let want = src.frame_at_index(index).unwrap();
        let p = psnr(&luma(&got), &luma(&want));
        assert!(p >= 22.0, "{label}: frame {index} PSNR {p:.1} dB");
    }
}

fn run(spec: CraftJobSpec) -> Result<(), encodercraft_core::EncoderError> {
    TranscodePipeline::new(spec).execute()
}

#[test]
fn every_video_preset_encodes_and_decodes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source);
    let registry = PresetRegistry::default();
    let mut checked = 0;
    for preset in registry.list().iter().filter(|p| p.video.is_some() && !matches!(p.container, ContainerType::AnimatedGif)) {
        let ext = match preset.container {
            ContainerType::IsoBmffMp4 => "mp4",
            ContainerType::QuickTimeMov => "mov",
            ContainerType::MatroskaMkv => "mkv",
            ContainerType::WebM => "webm",
            ContainerType::MxfOp1a | ContainerType::MxfOpAtom => "mxf",
            other => panic!("unexpected video container {other:?} in preset {}", preset.name),
        };
        let out = dir.path().join(format!("preset_{checked}.{ext}"));
        let mut target = small_target(&out, preset.container, &preset.name);
        registry.apply_to_target(&mut target);
        assert!(target.video.codec.is_some(), "{}: preset settings were not applied", preset.name);
        target.target_bitrate_kbps = target.target_bitrate_kbps.map(|b| b.min(4_000));
        run(job(&source, target)).unwrap_or_else(|e| panic!("{}: encode failed: {e}", preset.name));
        assert_matches_source(&preset.name, &source, &out);
        checked += 1;
    }
    assert!(checked >= 8, "only {checked} presets were exercised");
}

#[test]
fn every_container_codec_pair_encodes_and_decodes() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source);
    let containers = [
        (ContainerType::IsoBmffMp4, "mp4"),
        (ContainerType::QuickTimeMov, "mov"),
        (ContainerType::MatroskaMkv, "mkv"),
        (ContainerType::WebM, "webm"),
        (ContainerType::MxfOp1a, "mxf"),
    ];
    for (container, ext) in containers {
        for (n, codec) in codecs_for_container(container).iter().enumerate() {
            let out = dir.path().join(format!("{container:?}_{n}.{ext}"));
            let mut target = small_target(&out, container, "Match Source - High Bitrate");
            target.target_bitrate_kbps = Some(2_000);
            target.video = VideoEncodeSettings { codec: Some(format!("{codec:?}").to_ascii_lowercase()), ..Default::default() };
            if matches!(codec, VideoCodecKind::DnxHd) {
                target.video.codec = Some("dnxhr".into());
            }
            let label = format!("{container:?}+{codec:?}");
            run(job(&source, target)).unwrap_or_else(|e| panic!("{label}: encode failed: {e}"));
            assert_matches_source(&label, &source, &out);
        }
    }
}

#[test]
fn invalid_container_codec_pairs_are_rejected_before_writing() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source);
    for (container, codec) in [(ContainerType::IsoBmffMp4, "prores"), (ContainerType::WebM, "h264"), (ContainerType::QuickTimeMov, "vp9")] {
        let out = dir.path().join(format!("bad_{codec}.out"));
        let mut target = small_target(&out, container, "Match Source - High Bitrate");
        target.video.codec = Some(codec.into());
        let err = run(job(&source, target)).unwrap_err();
        assert!(err.to_string().contains("cannot be stored"), "{container:?}+{codec}: {err}");
        assert!(!out.exists(), "{container:?}+{codec}: output was written");
    }
}

#[test]
fn prores_hq_preset_writes_the_hq_sample_entry() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source);
    let registry = PresetRegistry::default();
    let out = dir.path().join("hq.mov");
    let mut target = small_target(&out, ContainerType::QuickTimeMov, "Apple ProRes 422 HQ");
    registry.apply_to_target(&mut target);
    run(job(&source, target)).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.windows(4).any(|w| w == b"apch"), "HQ sample entry apch missing");
    assert!(!bytes.windows(4).any(|w| w == b"apcn"), "standard-profile sample entry written for an HQ preset");
}

#[test]
fn mp4_output_is_faststart() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_source(&source);
    let out = dir.path().join("fast.mp4");
    run(job(&source, small_target(&out, ContainerType::IsoBmffMp4, "Match Source - High Bitrate"))).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let (mut pos, mut moov, mut mdat) = (0usize, None, None);
    while pos + 8 <= bytes.len() {
        let size = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
        match &bytes[pos + 4..pos + 8] {
            b"moov" => moov = Some(pos),
            b"mdat" => mdat = Some(pos),
            _ => {}
        }
        if size < 8 {
            break;
        }
        pos += size;
    }
    assert!(moov.unwrap() < mdat.unwrap(), "moov must precede mdat for progressive playback");
}

#[test]
fn h264_params_are_applied() {
    let encode = |params: &EncodeParams| {
        let mut enc = create_video_encoder_with(VideoCodecKind::H264, W, H, fps(), params).unwrap();
        let mut packets = Vec::new();
        for i in 0..20 {
            let mut frame = test_frame(i);
            frame.pts = i as i64;
            packets.extend(enc.encode_frame(&frame).unwrap());
        }
        packets.extend(enc.flush().unwrap());
        (packets, enc.config_record())
    };
    let (packets, _) = encode(&EncodeParams { keyframe_interval: Some(5), bframes: Some(0), bitrate_kbps: Some(1_000), ..Default::default() });
    let keys: Vec<i64> = packets.iter().filter(|p| p.is_keyframe).map(|p| p.pts).collect();
    assert_eq!(keys, vec![0, 5, 10, 15], "keyframe interval");
    assert!(packets.windows(2).all(|w| w[0].dts <= w[1].dts));

    let (_, avcc_709) = encode(&EncodeParams { color: Some(ColorTag::Bt709), ..Default::default() });
    let (_, avcc_hlg) = encode(&EncodeParams { color: Some(ColorTag::Hlg), ..Default::default() });
    assert_ne!(avcc_709, avcc_hlg, "HLG tag must change the SPS colour description");
    let (_, avcc_baseline) = encode(&EncodeParams { profile: Some("baseline".into()), ..Default::default() });
    assert_eq!(avcc_baseline.get(1), Some(&66), "baseline profile_idc");
}

#[test]
fn unsupported_parameters_are_rejected_not_ignored() {
    let hdr = EncodeParams { color: Some(ColorTag::Pq), ..Default::default() };
    assert!(create_video_encoder_with(VideoCodecKind::Hevc, W, H, fps(), &hdr).is_err());
    let bframes = EncodeParams { bframes: Some(2), ..Default::default() };
    assert!(create_video_encoder_with(VideoCodecKind::Vp9, W, H, fps(), &bframes).is_err());
    let profile = EncodeParams { profile: Some("hq".into()), ..Default::default() };
    assert!(create_video_encoder_with(VideoCodecKind::H264, W, H, fps(), &profile).is_err());
    assert!(create_video_encoder_with(VideoCodecKind::ProRes, W, H, fps(), &profile).is_ok());
}
