//! Milestone 52 Upstream Codec Consolidation and AME Parity Matrix Tests.
//!
//! Validates:
//! 1. Video Codecs (APV RFC 9924, MPEG-2, MPEG-4 Part 2 SP, FFV1 v3) via `craft-video`
//! 2. Audio DSP (AC-3, E-AC-3, ALAC, QOA) via `craft-audio`
//! 3. Containers (AVI, MXF, MP4, MKV) via `craft-media`
//! 4. Captions (11 subtitle formats) via `craft-captions`
//! 5. Color CMS & ISO 21496-1 HDR Gain Maps via `craft-cms` and `craft-gainmap`
//! 6. Timeline Interchange (EDL, OTIO, AAF, OMF) via `craft-timeline` and `craft-aaf`

use std::fs::File;
use std::path::{Path, PathBuf};

use encodercraft_color::{ColorSpaceTag, PixelFormat, apply_hdr_gainmap_to_rgba8};
use encodercraft_core::{
    AudioEncodeSettings, ChannelLayout, ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, Rational64, SourceDescriptor,
    StartPolicy, VideoEncodeSettings, VideoFrame,
};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{Mp4Muxer, VideoSample, VideoStreamConfig};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_pipeline::captions::CaptionTrack;
use encodercraft_pipeline::project::TimelineProject;
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};
use tempfile::tempdir;

const W: u32 = 128;
const H: u32 = 72;
const FRAMES: u64 = 6;

fn fps() -> Rational64 {
    Rational64 { num: 24, den: 1 }
}

fn build_job(source: PathBuf, targets: Vec<OutputTarget>) -> CraftJobSpec {
    CraftJobSpec {
        version: "1.0".into(),
        job_id: "m52-test-job".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: source, start_tick: None, duration_ticks: None, timebase: None },
        targets,
    }
}

fn write_synthetic_mp4(path: &Path) {
    let mut encoder = create_video_encoder(VideoCodecKind::H264, W, H, fps(), Some(2000)).unwrap();
    let mut mux = Mp4Muxer::new(File::create(path).unwrap(), false).unwrap();
    let stream_idx = mux
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

    let dims = Dimensions::new(W, H).unwrap();
    for i in 0..FRAMES {
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Yuv420p, ColorSpaceTag::REC709, i as i64, 1, fps()).unwrap();
        frame.data_mut().fill((i * 30 + 16) as u8);
        for pkt in encoder.encode_frame(&frame).unwrap() {
            mux.write_video_sample(
                stream_idx,
                VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1, is_keyframe: pkt.is_keyframe },
            )
            .unwrap();
        }
    }
    for pkt in encoder.flush().unwrap() {
        mux.write_video_sample(
            stream_idx,
            VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1, is_keyframe: pkt.is_keyframe },
        )
        .unwrap();
    }
    mux.finish(false).unwrap();
}

fn write_synthetic_wav(path: &Path) {
    let mut pcm_bytes = Vec::new();
    for i in 0..22050 {
        let sample = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin() * 16384.0) as i16;
        pcm_bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let mut wav_data = Vec::new();
    wav_data.extend_from_slice(b"RIFF");
    wav_data.extend_from_slice(&((pcm_bytes.len() + 36) as u32).to_le_bytes());
    wav_data.extend_from_slice(b"WAVEfmt ");
    wav_data.extend_from_slice(&16u32.to_le_bytes());
    wav_data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav_data.extend_from_slice(&1u16.to_le_bytes()); // 1 channel
    wav_data.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
    wav_data.extend_from_slice(&(44100u32 * 2).to_le_bytes()); // byte rate
    wav_data.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav_data.extend_from_slice(b"data");
    wav_data.extend_from_slice(&(pcm_bytes.len() as u32).to_le_bytes());
    wav_data.extend_from_slice(&pcm_bytes);
    std::fs::write(path, wav_data).unwrap();
}

#[test]
fn test_m52_video_codecs_apv_mpeg2_mpeg4_ffv1_transcode() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    write_synthetic_mp4(&source);

    // 1. APV 10-bit in MOV
    let apv_out = dir.path().join("output_apv.mov");
    let apv_target = OutputTarget {
        target_id: "apv-target".into(),
        output_path: apv_out.clone(),
        preset_name: "Advanced Professional Video (APV 422 10-bit)".into(),
        container: ContainerType::QuickTimeMov,
        export_video: true,
        export_audio: false,
        video: VideoEncodeSettings { codec: Some("apv".into()), pixel_format: Some(PixelFormat::Yuv422p10), ..Default::default() },
        ..Default::default()
    };

    // 2. MPEG-2 in TS
    let mpeg2_out = dir.path().join("output_mpeg2.ts");
    let mpeg2_target = OutputTarget {
        target_id: "mpeg2-target".into(),
        output_path: mpeg2_out.clone(),
        preset_name: "MPEG-2 Broadcast Master".into(),
        container: ContainerType::MpegTs,
        export_video: true,
        export_audio: false,
        video: VideoEncodeSettings { codec: Some("mpeg2".into()), pixel_format: Some(PixelFormat::Yuv420p), ..Default::default() },
        ..Default::default()
    };

    // 3. MPEG-4 SP in AVI
    let avi_out = dir.path().join("output_mpeg4.avi");
    let avi_target = OutputTarget {
        target_id: "avi-target".into(),
        output_path: avi_out.clone(),
        preset_name: "AVI Video (MPEG-4 SP)".into(),
        container: ContainerType::Avi,
        export_video: true,
        export_audio: false,
        video: VideoEncodeSettings { codec: Some("mpeg4sp".into()), pixel_format: Some(PixelFormat::Yuv420p), ..Default::default() },
        ..Default::default()
    };

    // 4. FFV1 Lossless in MKV
    let ffv1_out = dir.path().join("output_ffv1.mkv");
    let ffv1_target = OutputTarget {
        target_id: "ffv1-target".into(),
        output_path: ffv1_out.clone(),
        preset_name: "FFV1 v3 Lossless Archival".into(),
        container: ContainerType::MatroskaMkv,
        export_video: true,
        export_audio: false,
        video: VideoEncodeSettings { codec: Some("ffv1".into()), pixel_format: Some(PixelFormat::Rgb8), ..Default::default() },
        ..Default::default()
    };

    let spec = build_job(source, vec![apv_target, mpeg2_target, avi_target, ffv1_target]);
    TranscodePipeline::new(spec).execute().unwrap();

    assert!(apv_out.exists() && std::fs::metadata(&apv_out).unwrap().len() > 100);
    assert!(mpeg2_out.exists() && std::fs::metadata(&mpeg2_out).unwrap().len() > 100);
    assert!(avi_out.exists() && std::fs::metadata(&avi_out).unwrap().len() > 100);
    assert!(ffv1_out.exists() && std::fs::metadata(&ffv1_out).unwrap().len() > 100);
}

#[test]
fn test_m52_audio_dsp_ac3_eac3_alac_qoa_transcode() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("audio_source.wav");
    write_synthetic_wav(&source);

    let ac3_out = dir.path().join("out.ac3");
    let eac3_out = dir.path().join("out.eac3");
    let alac_out = dir.path().join("out.alac");
    let qoa_out = dir.path().join("out.qoa");

    let ac3_target = OutputTarget {
        target_id: "ac3-target".into(),
        output_path: ac3_out.clone(),
        preset_name: "Dolby Digital AC-3 Audio".into(),
        container: ContainerType::Ac3,
        export_video: false,
        export_audio: true,
        audio: Some(AudioEncodeSettings {
            codec: Some("ac3".into()),
            sample_rate: Some(48000),
            bitrate_kbps: Some(384),
            layout: Some(ChannelLayout::Stereo),
            ..Default::default()
        }),
        ..Default::default()
    };

    let eac3_target = OutputTarget {
        target_id: "eac3-target".into(),
        output_path: eac3_out.clone(),
        preset_name: "Dolby Digital Plus E-AC-3 Audio".into(),
        container: ContainerType::Eac3,
        export_video: false,
        export_audio: true,
        audio: Some(AudioEncodeSettings {
            codec: Some("eac3".into()),
            sample_rate: Some(48000),
            bitrate_kbps: Some(448),
            layout: Some(ChannelLayout::Stereo),
            ..Default::default()
        }),
        ..Default::default()
    };

    let alac_target = OutputTarget {
        target_id: "alac-target".into(),
        output_path: alac_out.clone(),
        preset_name: "Apple Lossless ALAC Audio".into(),
        container: ContainerType::Alac,
        export_video: false,
        export_audio: true,
        audio: Some(AudioEncodeSettings {
            codec: Some("alac".into()),
            sample_rate: Some(48000),
            bit_depth: Some(16),
            layout: Some(ChannelLayout::Stereo),
            ..Default::default()
        }),
        ..Default::default()
    };

    let qoa_target = OutputTarget {
        target_id: "qoa-target".into(),
        output_path: qoa_out.clone(),
        preset_name: "Quite OK Audio QOA".into(),
        container: ContainerType::Qoa,
        export_video: false,
        export_audio: true,
        audio: Some(AudioEncodeSettings {
            codec: Some("qoa".into()),
            sample_rate: Some(48000),
            layout: Some(ChannelLayout::Stereo),
            ..Default::default()
        }),
        ..Default::default()
    };

    let spec = build_job(source, vec![ac3_target, eac3_target, alac_target, qoa_target]);
    TranscodePipeline::new(spec).execute().unwrap();

    assert!(ac3_out.exists() && std::fs::metadata(&ac3_out).unwrap().len() > 100);
    assert!(eac3_out.exists() && std::fs::metadata(&eac3_out).unwrap().len() > 100);
    assert!(alac_out.exists() && std::fs::metadata(&alac_out).unwrap().len() > 100);
    assert!(qoa_out.exists() && std::fs::metadata(&qoa_out).unwrap().len() > 100);
}

#[test]
fn test_m52_captions_universal_11_formats_via_craft_captions() {
    let dir = tempdir().unwrap();
    let srt_path = dir.path().join("subtitles.srt");
    let srt_content = "1\n00:00:01,000 --> 00:00:04,000\nMilestone 52 Universal Captions Test\n\n2\n00:00:05,000 --> 00:00:08,000\nBacked by storytold/craft-captions\n";
    std::fs::write(&srt_path, srt_content).unwrap();

    let track = CaptionTrack::from_file(&srt_path).unwrap();
    assert_eq!(track.cues().len(), 2);

    let formats = [
        encodercraft_core::CaptionExportFormat::Srt,
        encodercraft_core::CaptionExportFormat::WebVtt,
        encodercraft_core::CaptionExportFormat::Scc,
        encodercraft_core::CaptionExportFormat::Mcc,
        encodercraft_core::CaptionExportFormat::Stl,
        encodercraft_core::CaptionExportFormat::Ttml,
        encodercraft_core::CaptionExportFormat::Dfxp,
        encodercraft_core::CaptionExportFormat::Ass,
        encodercraft_core::CaptionExportFormat::Pjs,
        encodercraft_core::CaptionExportFormat::RealText,
        encodercraft_core::CaptionExportFormat::JacoSub,
    ];

    let media_out = dir.path().join("video.mp4");
    for fmt in formats {
        let sidecar_path = track.export_sidecar(&media_out, fmt, None).unwrap();
        assert!(sidecar_path.exists());
        let data = std::fs::read(&sidecar_path).unwrap();
        assert!(!data.is_empty(), "format {fmt:?} exported empty file");
    }
}

#[test]
fn test_m52_cms_and_hdr_gainmap_transforms() {
    let mut base = [100u8, 100, 100, 255];
    let gainmap = [255u8, 255, 255, 255];
    apply_hdr_gainmap_to_rgba8(&mut base, 1, 1, &gainmap, 1, 1, 4.0).unwrap();
    assert!(base[0] > 100);
}

#[test]
fn test_m52_editorial_timeline_interchange_edl_otio_aaf_omf() {
    let prproj_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PremiereData Version="3">
  <Sequence>
    <Name>Milestone 52 Master Cut</Name>
    <FrameWidth>1920</FrameWidth>
    <FrameHeight>1080</FrameHeight>
    <FrameRate>24</FrameRate>
    <TrackItem>
      <Name>Scene01_Take01.mov</Name>
      <Start>0</Start>
      <End>2400</End>
      <In>0</In>
      <Out>2400</Out>
      <Kind>Video</Kind>
    </TrackItem>
    <TrackItem>
      <Name>Audio_Stereo_Master.wav</Name>
      <Start>0</Start>
      <End>2400</End>
      <In>0</In>
      <Out>2400</Out>
      <Kind>Audio</Kind>
    </TrackItem>
  </Sequence>
</PremiereData>"#;

    let project = TimelineProject::from_prproj(prproj_xml.as_bytes()).unwrap();

    // 1. CMX 3600 EDL
    let edl = project.to_edl().unwrap();
    eprintln!("EDL OUTPUT:\n{edl}");
    assert!(edl.contains("TITLE: Milestone 52 Master Cut"));
    assert!(edl.contains("AX"));

    // 2. OTIO JSON
    let otio = project.to_otio_json().unwrap();
    assert!(otio.contains("OTIO_SCHEMA"));
    assert!(otio.contains("Milestone 52 Master Cut"));

    // 3. AAF Binary
    let aaf_bytes = project.to_aaf().unwrap();
    assert!(aaf_bytes.len() > 512);
    // Compound File Binary Signature: D0 CF 11 E0 A1 B1 1A E1
    assert_eq!(&aaf_bytes[0..8], &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);

    // 4. OMF Binary
    let omf_bytes = project.to_omf().unwrap();
    assert!(omf_bytes.len() > 512);
    assert!(craft_aaf::omf::sniff(&omf_bytes));
}
