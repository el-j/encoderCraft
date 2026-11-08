//! Every video preset in the registry, and every container x codec pair, is encoded from a real
//! source and the output is decoded again to check size, frame count, duration and fidelity.

use std::fs::File;
use std::path::Path;

use encodercraft_color::{ColorSpaceTag, PixelFormat, TransferFunction};
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
            spatial: None,
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
    match frame.format {
        PixelFormat::Rgba8 => plane
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])).round().clamp(0.0, 255.0) as u8)
            .collect(),
        PixelFormat::RgbaF32 => plane
            .as_chunks::<16>()
            .0
            .iter()
            .map(|pixel| {
                let signal = |start| {
                    let value = f32::from_ne_bytes(pixel[start..start + 4].try_into().unwrap()).clamp(0.0, 1.0);
                    if frame.color_space.transfer == TransferFunction::Linear {
                        if value < 0.018 { 4.5 * value } else { 1.099 * value.powf(0.45) - 0.099 }
                    } else {
                        value
                    }
                };
                let r = f64::from(signal(0));
                let g = f64::from(signal(4));
                let b = f64::from(signal(8));
                ((0.2126 * r + 0.7152 * g + 0.0722 * b) * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect(),
        other => panic!("test luma does not support {other:?}"),
    }
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum::<f64>() / a.len().max(1) as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// Decodes `out` and checks dimensions, frame count, duration and PSNR against the decoded source.
fn assert_matches_source(label: &str, source: &Path, out: &Path) {
    let src = MediaReader::open(source).unwrap();
    let source_probe = src.probe();
    let source_video = source_probe.video.as_ref().unwrap_or_else(|| panic!("{label}: source has no video stream"));
    let reader = MediaReader::open(out).unwrap_or_else(|e| panic!("{label}: output does not open: {e}"));
    let probe = reader.probe();
    let video = probe.video.as_ref().unwrap_or_else(|| panic!("{label}: no video stream"));
    assert_eq!((video.width, video.height), (source_video.width, source_video.height), "{label}: dimensions");
    let source_frames = src.frame_count();
    assert_eq!(reader.frame_count(), source_frames, "{label}: frame count");
    let frame_duration = source_video.frame_rate.den as f64 / source_video.frame_rate.num.max(1) as f64;
    assert!(
        (probe.duration_seconds - source_probe.duration_seconds).abs() <= frame_duration + 1e-3,
        "{label}: output duration {}",
        probe.duration_seconds
    );
    for index in [0, source_frames.saturating_sub(1)] {
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
            ContainerType::MpegTs => "ts",
            other => panic!("unexpected video container {other:?} in preset {}", preset.name),
        };
        let out = dir.path().join(format!("preset_{checked}.{ext}"));
        let mut target = small_target(&out, preset.container, &preset.name);
        registry.apply_to_target(&mut target);
        assert!(target.video.codec.is_some(), "{}: preset settings were not applied", preset.name);
        target.target_bitrate_kbps = target.target_bitrate_kbps.map(|b| b.min(4_000));
        run(job(&source, target)).unwrap_or_else(|e| panic!("{}: encode failed: {e}", preset.name));
        if matches!(preset.container, ContainerType::MpegTs) {
            let bytes = std::fs::read(&out).unwrap();
            assert!(!bytes.is_empty(), "MPEG-TS output must not be empty");
            assert_eq!(bytes.len() % 188, 0, "MPEG-TS output must be divisible by 188-byte packets");
            assert_eq!(bytes[0], 0x47, "First byte must be TS sync 0x47");
            checked += 1;
            continue;
        }
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
fn match_source_preserves_high_depth_through_the_pipeline() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../source/tests/fixtures/prores.mov");
    let source_reader = MediaReader::open(&source).unwrap();
    let source_video = source_reader.probe().video.as_ref().unwrap();
    assert!(source_video.bit_depth > 8, "fixture should exercise the high-depth path");
    assert_eq!(source_reader.frame_at_index(0).unwrap().format, PixelFormat::RgbaF32);

    let dir = tempdir().unwrap();
    let output = dir.path().join("high_depth_match_source.mp4");
    let match_source = encodercraft_presets::factory_presets().into_iter().find(|preset| preset.name == "Match Source - High Bitrate").unwrap();
    let source_info = encodercraft_demux::probe_file(&source).unwrap();
    let target = encodercraft_presets::evaluate_preset(&match_source, &source_info, output.clone()).unwrap();
    assert_eq!(target.video.codec.as_deref(), Some("hevc"));
    run(job(&source, target)).unwrap();

    let output_reader = MediaReader::open(&output).unwrap();
    let output_video = output_reader.probe().video.as_ref().unwrap();
    assert!(output_video.bit_depth > 8, "the encoded output must retain more than 8 bits per component");
    assert_eq!(output_reader.frame_at_index(0).unwrap().format, PixelFormat::RgbaF32);
    assert_matches_source("10-bit Match Source transcode", &source, &output);
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

#[test]
fn mp3_and_ogg_vorbis_pipeline_transcode() {
    let dir = tempdir().unwrap();
    let wav_path = dir.path().join("input.wav");

    // Write a mono 44.1kHz WAV source with 4410 samples (0.1s)
    let samples: Vec<i16> = (0..4410).map(|i| ((i % 100) as i16 - 50) * 300).collect();
    let mut pcm_bytes = Vec::new();
    for s in &samples {
        pcm_bytes.extend_from_slice(&s.to_le_bytes());
    }

    let mut wav_file = Vec::new();
    wav_file.extend_from_slice(b"RIFF");
    let total_size = (36 + pcm_bytes.len()) as u32;
    wav_file.extend_from_slice(&total_size.to_le_bytes());
    wav_file.extend_from_slice(b"WAVEfmt ");
    wav_file.extend_from_slice(&16u32.to_le_bytes()); // subchunk1size
    wav_file.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav_file.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wav_file.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    wav_file.extend_from_slice(&(44100u32 * 2).to_le_bytes()); // byte rate
    wav_file.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav_file.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav_file.extend_from_slice(b"data");
    wav_file.extend_from_slice(&(pcm_bytes.len() as u32).to_le_bytes());
    wav_file.extend_from_slice(&pcm_bytes);
    std::fs::write(&wav_path, &wav_file).unwrap();

    let mp3_path = dir.path().join("output.mp3");
    let ogg_path = dir.path().join("output.ogg");

    let mut mp3_target = small_target(&mp3_path, ContainerType::Mp3, "MP3");
    mp3_target.audio = Some(encodercraft_core::AudioEncodeSettings {
        codec: Some("mp3".into()),
        sample_rate: Some(44100),
        layout: Some(encodercraft_core::ChannelLayout::Mono),
        bitrate_kbps: Some(192),
        delay_ms: 0,
    });
    mp3_target.metadata =
        Some(encodercraft_core::XmpMetadata { title: Some("Ecosystem Audio".into()), creator: Some("ArtCraft".into()), ..Default::default() });

    let mut ogg_target = small_target(&ogg_path, ContainerType::OggAudio, "Ogg");
    ogg_target.audio = Some(encodercraft_core::AudioEncodeSettings {
        codec: Some("vorbis".into()),
        sample_rate: Some(44100),
        layout: Some(encodercraft_core::ChannelLayout::Mono),
        bitrate_kbps: Some(160),
        delay_ms: 0,
    });

    run(job(&wav_path, mp3_target)).unwrap();
    run(job(&wav_path, ogg_target)).unwrap();

    let mp3_data = std::fs::read(&mp3_path).unwrap();
    assert!(!mp3_data.is_empty());
    assert_eq!(&mp3_data[0..3], b"ID3");

    let ogg_data = std::fs::read(&ogg_path).unwrap();
    assert!(!ogg_data.is_empty());
    assert_eq!(&ogg_data[0..4], b"OggS");

    // Verify both are decodable by MediaReader
    let mp3_reader = MediaReader::open(&mp3_path).unwrap();
    assert_eq!(mp3_reader.frame_count(), 0);
    let mp3_audio = mp3_reader.read_all_audio(44100).unwrap();
    assert!(mp3_audio.samples_per_channel() > 0);

    let ogg_reader = MediaReader::open(&ogg_path).unwrap();
    assert_eq!(ogg_reader.frame_count(), 0);
    let ogg_audio = ogg_reader.read_all_audio(44100).unwrap();
    assert!(ogg_audio.samples_per_channel() > 0);

    // CAF (Apple Core Audio Format) pipeline execution
    let caf_path = dir.path().join("output.caf");
    let mut caf_target = small_target(&caf_path, ContainerType::Caf, "Apple CAF");
    caf_target.audio = Some(encodercraft_core::AudioEncodeSettings {
        codec: Some("lpcm".into()),
        sample_rate: Some(44100),
        layout: Some(encodercraft_core::ChannelLayout::Mono),
        bitrate_kbps: None,
        delay_ms: 0,
    });
    run(job(&wav_path, caf_target)).unwrap();
    let caf_data = std::fs::read(&caf_path).unwrap();
    assert!(!caf_data.is_empty());
    assert_eq!(&caf_data[0..4], b"caff");
    assert!(caf_data.windows(4).any(|w| w == b"desc"));
    assert!(caf_data.windows(4).any(|w| w == b"data"));

    // Opus (RFC 7845 Standalone Ogg Opus) pipeline execution
    let opus_path = dir.path().join("output.opus");
    let mut opus_target = small_target(&opus_path, ContainerType::Opus, "Opus Audio");
    opus_target.audio = Some(encodercraft_core::AudioEncodeSettings {
        codec: Some("opus".into()),
        sample_rate: Some(48000),
        layout: Some(encodercraft_core::ChannelLayout::Mono),
        bitrate_kbps: Some(128),
        delay_ms: 0,
    });
    opus_target.metadata =
        Some(encodercraft_core::XmpMetadata { title: Some("Opus Pipeline Track".into()), creator: Some("ArtCraft".into()), ..Default::default() });
    run(job(&wav_path, opus_target)).unwrap();
    let opus_data = std::fs::read(&opus_path).unwrap();
    assert!(!opus_data.is_empty());
    assert_eq!(&opus_data[0..4], b"OggS");
    assert!(opus_data.windows(8).any(|w| w == b"OpusHead"));
    assert!(opus_data.windows(8).any(|w| w == b"OpusTags"));
}

#[test]
fn dds_dpx_and_jxl_pipeline_transcode() {
    let dir = tempfile::tempdir().unwrap();
    let src_png = dir.path().join("source.png");

    let frame = test_frame(0);
    let png_bytes = encodercraft_image_codecs::encode_still(
        &frame,
        encodercraft_image_codecs::StillFormat::Png,
        &encodercraft_image_codecs::StillOptions::default(),
    )
    .unwrap();
    std::fs::write(&src_png, png_bytes).unwrap();

    let dds_path = dir.path().join("output.dds");
    let dpx_path = dir.path().join("output.dpx");
    let jxl_path = dir.path().join("output.jxl");
    let dpx_seq_pattern = dir.path().join("seq_%04d.dpx");

    let dds_target = small_target(&dds_path, ContainerType::Dds, "DirectDraw Surface DDS (BC3)");
    let dpx_target = small_target(&dpx_path, ContainerType::Dpx, "SMPTE DPX 10-bit Film Scan");
    let jxl_target = small_target(&jxl_path, ContainerType::Jxl, "JPEG XL Lossless Modular");
    let dpx_seq_target = small_target(&dpx_seq_pattern, ContainerType::Dpx, "SMPTE DPX Sequence");

    run(job(&src_png, dds_target)).unwrap();
    run(job(&src_png, dpx_target)).unwrap();
    run(job(&src_png, jxl_target)).unwrap();
    run(job(&src_png, dpx_seq_target)).unwrap();

    let dds_data = std::fs::read(&dds_path).unwrap();
    assert_eq!(&dds_data[0..4], b"DDS ");

    let dpx_data = std::fs::read(&dpx_path).unwrap();
    assert!(dpx_data.starts_with(b"SDPX") || dpx_data.starts_with(b"XPDS"));

    let jxl_data = std::fs::read(&jxl_path).unwrap();
    assert_eq!(&jxl_data[0..2], &[0xFF, 0x0A]);

    let seq_frame = dir.path().join("seq_0001.dpx");
    assert!(seq_frame.exists());
    let seq_data = std::fs::read(&seq_frame).unwrap();
    assert!(seq_data.starts_with(b"SDPX") || seq_data.starts_with(b"XPDS"));

    // Radiance HDR, DICOM, and FITS pipeline transcode
    let hdr_path = dir.path().join("output.hdr");
    let dcm_path = dir.path().join("output.dcm");
    let fits_path = dir.path().join("output.fits");

    let hdr_target = small_target(&hdr_path, ContainerType::RadianceHdr, "Radiance HDR RGBE Environment");
    let dcm_target = small_target(&dcm_path, ContainerType::Dicom, "Medical DICOM Image (Secondary Capture)");
    let fits_target = small_target(&fits_path, ContainerType::Fits, "Astronomical FITS Scientific Data");

    run(job(&src_png, hdr_target)).unwrap();
    run(job(&src_png, dcm_target)).unwrap();
    run(job(&src_png, fits_target)).unwrap();

    let hdr_data = std::fs::read(&hdr_path).unwrap();
    assert!(hdr_data.starts_with(b"#?RADIANCE") || hdr_data.starts_with(b"#?RGBE"));

    let dcm_data = std::fs::read(&dcm_path).unwrap();
    assert_eq!(&dcm_data[128..132], b"DICM");

    let fits_data = std::fs::read(&fits_path).unwrap();
    assert!(fits_data.starts_with(b"SIMPLE  ="));
}

#[test]
fn spatial_3d_fbx_abc_dae_usdc_splat_pipeline_transcode() {
    let dir = tempfile::tempdir().unwrap();
    let src_glb = dir.path().join("source.glb");

    let mut mesh = encodercraft_spatial_io::TriangleMesh::new("UnitBox");
    mesh.vertices = vec![[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5]];
    mesh.indices = vec![[0, 1, 2]];
    mesh.generate_flat_normals();

    let glb_bytes = encodercraft_spatial_io::write_glb(&mesh).unwrap();
    std::fs::write(&src_glb, glb_bytes).unwrap();

    let fbx_path = dir.path().join("out.fbx");
    let dae_path = dir.path().join("out.dae");
    let abc_path = dir.path().join("out.abc");
    let usdc_path = dir.path().join("out.usdc");
    let splat_path = dir.path().join("out.splat");

    let fbx_target = small_target(&fbx_path, ContainerType::Fbx, "Autodesk Filmbox 3D (FBX)");
    let dae_target = small_target(&dae_path, ContainerType::Collada, "Collada Digital Asset Exchange (DAE)");
    let abc_target = small_target(&abc_path, ContainerType::Alembic, "Alembic VFX Animation Cache (ABC)");
    let usdc_target = small_target(&usdc_path, ContainerType::Usdc, "Universal Scene Description Crate (USDC)");
    let splat_target = small_target(&splat_path, ContainerType::Splat, "3D Gaussian Splat Point Cloud (SPLAT)");

    run(job(&src_glb, fbx_target)).unwrap();
    run(job(&src_glb, dae_target)).unwrap();
    run(job(&src_glb, abc_target)).unwrap();
    run(job(&src_glb, usdc_target)).unwrap();
    run(job(&src_glb, splat_target)).unwrap();

    let fbx_data = std::fs::read(&fbx_path).unwrap();
    assert!(fbx_data.starts_with(b"Kaydara FBX Binary  \0"));

    let dae_data = std::fs::read_to_string(&dae_path).unwrap();
    assert!(dae_data.contains("<COLLADA"));

    let abc_data = std::fs::read(&abc_path).unwrap();
    assert!(abc_data.starts_with(b"Ogawa"));

    let usdc_data = std::fs::read(&usdc_path).unwrap();
    assert!(usdc_data.starts_with(b"PXR-USDC"));

    let splat_data = std::fs::read(&splat_path).unwrap();
    assert_eq!(splat_data.len() % 32, 0);
}

#[test]
fn publications_and_lottie_and_metafile_pipeline_transcode() {
    let dir = tempfile::tempdir().unwrap();

    // 1. Publication transcode from DesignCraft to EPUB and PPTX
    let doc_canvas = encodercraft_pipeline::document::DocumentCanvas::new("Test Publication");
    let src_designcraft = dir.path().join("source.designcraft");
    let designcraft_bytes = doc_canvas.to_designcraft_binary().unwrap();
    std::fs::write(&src_designcraft, designcraft_bytes).unwrap();

    let epub_path = dir.path().join("out.epub");
    let pptx_path = dir.path().join("out.pptx");

    let epub_target = small_target(&epub_path, ContainerType::Epub, "EPUB 3 Reflowable Publication");
    let pptx_target = small_target(&pptx_path, ContainerType::Pptx, "PowerPoint Presentation Slide Deck");

    run(job(&src_designcraft, epub_target)).unwrap();
    run(job(&src_designcraft, pptx_target)).unwrap();

    let epub_data = std::fs::read(&epub_path).unwrap();
    assert!(epub_data.starts_with(b"PK\x03\x04"));
    assert!(epub_data.windows(20).any(|w| w == b"application/epub+zip"));

    let pptx_data = std::fs::read(&pptx_path).unwrap();
    assert!(pptx_data.starts_with(b"PK\x03\x04"));
    assert!(pptx_data.windows(19).any(|w| w == b"[Content_Types].xml"));

    // 2. Vector transcode to Windows Metafile (WMF) and Lottie JSON
    let mut vec_canvas = encodercraft_image_codecs::VectorCanvas::new(800.0, 600.0);
    let mut path = encodercraft_image_codecs::VectorPath::new();
    path.move_to(0.0, 0.0);
    path.line_to(100.0, 100.0);
    vec_canvas.add_path(path);

    let src_vector = dir.path().join("source.vectorcraft");
    let vec_bytes = vec_canvas.to_vectorcraft().unwrap();
    std::fs::write(&src_vector, vec_bytes).unwrap();

    let wmf_path = dir.path().join("out.wmf");
    let lottie_path = dir.path().join("out.json");

    let wmf_target = small_target(&wmf_path, ContainerType::WindowsMetafile, "Windows Metafile (WMF)");
    let lottie_target = small_target(&lottie_path, ContainerType::LottieJson, "Lottie Motion Vector JSON");

    run(job(&src_vector, wmf_target)).unwrap();
    run(job(&src_vector, lottie_target)).unwrap();

    let wmf_data = std::fs::read(&wmf_path).unwrap();
    assert_eq!(&wmf_data[0..4], &0x9AC6CDD7u32.to_le_bytes());

    let lottie_data = std::fs::read_to_string(&lottie_path).unwrap();
    assert!(lottie_data.contains("\"v\": \"5.7.0\""));
}

#[test]
fn broadcast_mpeg_ts_and_apple_spatial_video_pipeline_transcode() {
    let dir = tempfile::tempdir().unwrap();
    let src_mp4 = dir.path().join("source.mp4");
    write_source(&src_mp4);

    // 1. MPEG-TS Broadcast transcode
    let ts_path = dir.path().join("broadcast.ts");
    let ts_target = small_target(&ts_path, ContainerType::MpegTs, "CableLabs Broadcast MPEG-TS");
    run(job(&src_mp4, ts_target)).unwrap();

    let ts_data = std::fs::read(&ts_path).unwrap();
    assert!(!ts_data.is_empty());
    assert_eq!(ts_data.len() % 188, 0, "MPEG-TS file size must be multiple of 188");
    for (idx, chunk) in ts_data.as_chunks::<188>().0.iter().enumerate() {
        assert_eq!(chunk[0], 0x47, "TS packet {idx} missing 0x47 sync byte");
    }
    // Check PAT packet (PID 0x0000 with PUSI = 1 => 0x47 0x40 0x00)
    assert!(ts_data.windows(3).any(|w| w == [0x47, 0x40, 0x00]), "Missing PAT packet in MPEG-TS");

    // 2. Apple Spatial Video (MV-HEVC)
    let spatial_path = dir.path().join("spatial.mov");
    let spatial_target = small_target(&spatial_path, ContainerType::QuickTimeMov, "Apple Spatial Video (MV-HEVC)");
    run(job(&src_mp4, spatial_target)).unwrap();

    let spatial_data = std::fs::read(&spatial_path).unwrap();
    assert!(!spatial_data.is_empty());
    assert!(spatial_data.windows(4).any(|w| w == b"svhd"), "Missing svhd spatial header box");
    assert!(spatial_data.windows(4).any(|w| w == b"st3d"), "Missing st3d stereoscopic 3D box");
    assert!(spatial_data.windows(4).any(|w| w == b"lhvC"), "Missing lhvC layered HEVC config box");
}
