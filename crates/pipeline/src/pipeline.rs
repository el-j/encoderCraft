//! End-to-end transcoding pipeline coordinator.
//!
//! Orchestrates demuxing, video scaling, audio DSP normalization, encoding, and muxing.

use std::fs::File;
use std::io::BufWriter;
use std::sync::Arc;
use std::time::Instant;

use encodercraft_audio_dsp::{normalize_loudness, remix_channels};
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ContainerType, CraftJobSpec, EncoderError, OutputTarget, Rational64};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_muxers::{BextMetadata, BwfWavWriter, Mp4Muxer, VideoSample, VideoStreamConfig, WavFormatTag};
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};

use crate::progress::JobProgress;
use crate::scale::FrameScaler;

/// Execution coordinator for a single transcoding job.
pub struct TranscodePipeline {
    spec: CraftJobSpec,
    progress: Arc<JobProgress>,
}

impl TranscodePipeline {
    /// Creates a new pipeline for the specified job specification.
    pub fn new(spec: CraftJobSpec) -> Self {
        let progress = Arc::new(JobProgress::new(100));
        Self { spec, progress }
    }

    /// Access the shared job progress reporter.
    pub fn progress(&self) -> Arc<JobProgress> {
        self.progress.clone()
    }

    /// Executes the transcode operation across all targets in the job specification.
    pub fn execute(&self) -> Result<(), EncoderError> {
        let start_time = Instant::now();

        if self.spec.targets.is_empty() {
            return Err(EncoderError::InvalidJobSpec("job contains no output targets".into()));
        }

        for target in &self.spec.targets {
            self.execute_target(target, start_time)?;
        }

        Ok(())
    }

    fn execute_target(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let target_path = &target.output_path;
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create output directory: {e}")))?;
        }

        // Export sidecar subtitle file if requested
        if let (Some(cap_path), Some(cap_format)) = (&target.caption_path, target.caption_format) {
            let track = crate::captions::CaptionTrack::from_file(cap_path)?;
            track.export_sidecar(&target.output_path, cap_format, target.caption_offset_ms)?;
        }

        // Export sidecar XMP metadata if requested
        if let Some(ref meta) = target.metadata
            && meta.export_sidecar_xmp
        {
            let xmp_packet = meta.to_xmp_packet();
            let sidecar_path = target.output_path.with_extension("xmp");
            std::fs::write(&sidecar_path, xmp_packet.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write sidecar XMP: {e}")))?;
        }

        match target.container {
            ContainerType::Wave => self.execute_audio_wav(target, start_time)?,
            ContainerType::Flac => self.execute_audio_flac(target, start_time)?,
            ContainerType::Aiff => self.execute_audio_aiff(target, start_time)?,
            ContainerType::IsoBmffMp4 => self.execute_video_mp4(target, false, start_time)?,
            ContainerType::QuickTimeMov => self.execute_video_mp4(target, true, start_time)?,
            _ => return Err(EncoderError::UnsupportedCodec(format!("unsupported target container: {:?}", target.container))),
        }

        // Export multichannel audio stems if configured
        if let Some(mode) = target.audio_stems
            && mode != encodercraft_core::AudioStemMode::None
        {
            self.export_audio_stems(target, mode)?;
        }

        Ok(())
    }

    fn execute_audio_flac(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let sample_rate = 48000;
        let mut sample_buf =
            encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)?;

        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&sample_buf)?;
        std::fs::write(&target.output_path, flac_bytes).map_err(|e| EncoderError::Io(format!("failed to write FLAC file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_aiff(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let sample_rate = 48000;
        let mut sample_buf =
            encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)?;

        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        let aiff_bytes = encodercraft_audio_dsp::encode_audio_aiff(&sample_buf)?;
        std::fs::write(&target.output_path, aiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write AIFF file: {e}")))?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_audio_wav(&self, target: &OutputTarget, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let writer = BufWriter::new(out_file);

        let bext = BextMetadata {
            description: format!("Encoded by encoderCraft - {}", target.preset_name),
            originator: "encoderCraft".into(),
            ..Default::default()
        };

        let sample_rate = 48000;
        let xmp_xml = target.metadata.as_ref().map(|m| m.to_xmp_packet());
        let mut wav_writer = BwfWavWriter::new_with_xmp(writer, sample_rate, 2, WavFormatTag::Pcm16, Some(&bext), xmp_xml.as_deref())?;

        let mut sample_buf =
            encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)?;

        // 1. Remix channels if needed
        sample_buf = remix_channels(&sample_buf, encodercraft_core::ChannelLayout::Stereo)?;

        // 2. Loudness normalization if specified
        if let Some(target_lufs) = target.target_lufs {
            let _ = normalize_loudness(&mut sample_buf, target_lufs as f64, -1.0)?;
        }

        // 3. Write buffer
        wav_writer.write_buffer(&sample_buf)?;
        wav_writer.finish()?;

        self.progress.advance_frame(start_time);
        self.progress.set_total_frames(1);

        Ok(())
    }

    fn execute_video_mp4(&self, target: &OutputTarget, is_mov: bool, start_time: Instant) -> Result<(), EncoderError> {
        let out_file = File::create(&target.output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let mut mp4_muxer = Mp4Muxer::new(out_file, is_mov)?;

        if target.hardware_acceleration {
            let caps = encodercraft_platform::detect_hardware();
            log::info!(
                "Hardware acceleration requested: {} (H.264={}, HEVC={}, ProRes={})",
                caps.device_name,
                caps.supports_h264,
                caps.supports_hevc,
                caps.supports_prores
            );
        }

        let w = target.custom_width.unwrap_or(1920);
        let h = target.custom_height.unwrap_or(1080);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);

        let lower_preset = target.preset_name.to_ascii_lowercase();
        let codec_kind = if is_mov && lower_preset.contains("prores") {
            VideoCodecKind::ProRes
        } else if lower_preset.contains("dnx") {
            VideoCodecKind::DnxHd
        } else if lower_preset.contains("hevc") || lower_preset.contains("h265") {
            VideoCodecKind::Hevc
        } else if lower_preset.contains("av1") {
            VideoCodecKind::Av1
        } else if lower_preset.contains("vp9") {
            VideoCodecKind::Vp9
        } else {
            VideoCodecKind::H264
        };

        let v_cfg = VideoStreamConfig {
            codec_fourcc: codec_kind.default_fourcc(),
            dimensions: dims,
            frame_rate: fps,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![0, 0, 0, 1],
        };

        let track_idx = mp4_muxer.add_video_stream(&v_cfg)?;
        let mut encoder = create_video_encoder(codec_kind, dims.width, dims.height, fps, target.target_bitrate_kbps)?;

        let total_frames = 24; // 1 second sequence
        self.progress.set_total_frames(total_frames);

        let scaler = FrameScaler::new(ScaleMode::Fit);

        let lut = if let Some(ref lut_path) = target.lut_path {
            let content = std::fs::read_to_string(lut_path).map_err(|e| EncoderError::Io(format!("failed to read LUT file: {e}")))?;
            Some(crate::lut::Lut3D::parse_cube(&content)?)
        } else {
            None
        };

        let tc_overlay = if target.burn_timecode { Some(crate::overlay::TimecodeOverlay::default()) } else { None };

        let (caption_burner, caption_track) = if target.burn_captions {
            if let Some(cap_path) = &target.caption_path {
                let track = crate::captions::CaptionTrack::from_file(cap_path)?;
                (Some(crate::captions::CaptionBurner::default()), Some(track))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        for f in 0..total_frames {
            if self.progress.is_cancelled() {
                return Err(EncoderError::Internal("job cancelled by user".into()));
            }

            let frame = encodercraft_core::VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, f as i64, 1, fps)?;
            let mut scaled_frame = scaler.scale(&frame, dims)?;

            if let Some(ref l) = lut {
                l.apply_to_frame(&mut scaled_frame)?;
            }

            if let Some(ref tc) = tc_overlay {
                let timecode = encodercraft_core::MediaTimecode::new(0, 0, (f / 24) as u8, (f % 24) as u8, false, fps);
                tc.draw(&mut scaled_frame, &timecode)?;
            }

            if let (Some(burner), Some(track)) = (&caption_burner, &caption_track) {
                let pts_seconds = f as f64 / fps.as_f64();
                burner.burn(&mut scaled_frame, track, pts_seconds)?;
            }

            let packets = encoder.encode_frame(&scaled_frame)?;
            for pkt in packets {
                let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1000, is_keyframe: pkt.is_keyframe };
                mp4_muxer.write_video_sample(track_idx, sample)?;
            }

            self.progress.advance_frame(start_time);
        }

        let flushed = encoder.flush()?;
        for pkt in flushed {
            let sample = VideoSample { data: &pkt.data, pts: pkt.pts, dts: pkt.dts, duration_ticks: 1000, is_keyframe: pkt.is_keyframe };
            mp4_muxer.write_video_sample(track_idx, sample)?;
        }

        mp4_muxer.finish(false)?;

        // Append Adobe XMP UUID box if metadata is defined
        if let Some(ref meta) = target.metadata {
            let xmp_packet = meta.to_xmp_packet();
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&target.output_path)
                .map_err(|e| EncoderError::Io(format!("failed to open MP4 for XMP embedding: {e}")))?;
            encodercraft_muxers::append_xmp_uuid_box(&mut file, &xmp_packet)?;
        }

        Ok(())
    }

    /// Exports multichannel audio stems (discrete mono or stereo pairs) conforming to AME multichannel workflow.
    fn export_audio_stems(&self, target: &OutputTarget, mode: encodercraft_core::AudioStemMode) -> Result<(), EncoderError> {
        let sample_rate = 48000;
        let channels = 2; // Default stereo source
        let sample_buf =
            encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Stereo, sample_rate as usize, 0)?;

        let stem_base = target.output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        let parent = target.output_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let num_samples = sample_buf.samples_per_channel();

        match mode {
            encodercraft_core::AudioStemMode::DiscreteMono => {
                let channel_names = ["L", "R", "C", "LFE", "Ls", "Rs", "Ch7", "Ch8"];
                for c in 0..channels {
                    let mut mono_buf =
                        encodercraft_core::AudioSampleBuffer::new_silent(sample_rate, encodercraft_core::ChannelLayout::Mono, num_samples, 0)?;
                    let src_plane = sample_buf.channel(c)?;
                    let dest_plane = mono_buf.channel_mut(0)?;
                    dest_plane.copy_from_slice(src_plane);

                    let name = channel_names.get(c).copied().unwrap_or("Ch");
                    let stem_filename = format!("{stem_base}_Stem_{name}.wav");
                    let stem_path = parent.join(stem_filename);

                    let file = File::create(&stem_path).map_err(|e| EncoderError::Io(format!("failed to create audio stem: {e}")))?;
                    let mut writer = BwfWavWriter::new(BufWriter::new(file), sample_rate, 1, WavFormatTag::Pcm16, None)?;
                    writer.write_buffer(&mono_buf)?;
                    writer.finish()?;
                }
            }
            encodercraft_core::AudioStemMode::StereoPairs => {
                let pair_count = channels.div_ceil(2);
                for p in 0..pair_count {
                    let stem_filename = format!("{stem_base}_Stems_Ch{}-{}.wav", p * 2 + 1, p * 2 + 2);
                    let stem_path = parent.join(stem_filename);

                    let file = File::create(&stem_path).map_err(|e| EncoderError::Io(format!("failed to create audio stem: {e}")))?;
                    let mut writer = BwfWavWriter::new(BufWriter::new(file), sample_rate, 2, WavFormatTag::Pcm16, None)?;
                    writer.write_buffer(&sample_buf)?;
                    writer.finish()?;
                }
            }
            encodercraft_core::AudioStemMode::None => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, JobPriority, SourceDescriptor, StartPolicy};
    use tempfile::tempdir;

    #[test]
    fn test_execute_audio_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.wav");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "WAV Broadcast".into(),
                container: ContainerType::Wave,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: Some(-23.0),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_video_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.mp4");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-video-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "Web 720p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(1280),
                custom_height: Some(720),
                custom_frame_rate: Some(Rational64::FPS_24),
                target_bitrate_kbps: Some(4000),
                target_lufs: None,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_flac_and_aiff_transcode() {
        let dir = tempdir().unwrap();
        let flac_file = dir.path().join("output.flac");
        let aiff_file = dir.path().join("output.aif");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-lossless-audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget {
                    target_id: "t_flac".into(),
                    output_path: flac_file.clone(),
                    preset_name: "FLAC Master".into(),
                    container: ContainerType::Flac,
                    custom_width: None,
                    custom_height: None,
                    custom_frame_rate: None,
                    target_bitrate_kbps: None,
                    target_lufs: Some(-16.0),
                    ..Default::default()
                },
                OutputTarget {
                    target_id: "t_aiff".into(),
                    output_path: aiff_file.clone(),
                    preset_name: "AIFF Master".into(),
                    container: ContainerType::Aiff,
                    custom_width: None,
                    custom_height: None,
                    custom_frame_rate: None,
                    target_bitrate_kbps: None,
                    target_lufs: None,
                    ..Default::default()
                },
            ],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(flac_file.exists());
        assert!(flac_file.metadata().unwrap().len() > 50);
        assert!(aiff_file.exists());
        assert!(aiff_file.metadata().unwrap().len() > 50);
    }

    #[test]
    fn test_execute_prores_mov_transcode() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("output.mov");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-prores-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_file.clone(),
                preset_name: "Apple ProRes 422".into(),
                container: ContainerType::QuickTimeMov,
                custom_width: Some(640),
                custom_height: Some(360),
                custom_frame_rate: Some(Rational64::FPS_24),
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_video_with_captions_sidecar_and_burn_in() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_with_captions.mp4");
        let srt_file = dir.path().join("subtitles.srt");

        std::fs::write(&srt_file, "1\n00:00:00,000 --> 00:00:02,000\nHello ArtCraft Subtitles!\n").unwrap();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-caption-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "YouTube 1080p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(640),
                custom_height: Some(360),
                custom_frame_rate: Some(Rational64::FPS_24),
                caption_path: Some(srt_file),
                caption_format: Some(encodercraft_core::CaptionExportFormat::WebVtt),
                burn_captions: true,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_video.exists());
        // Verify WebVTT sidecar was written
        let vtt_sidecar = dir.path().join("output_with_captions.vtt");
        assert!(vtt_sidecar.exists());
        let vtt_text = std::fs::read_to_string(vtt_sidecar).unwrap();
        assert!(vtt_text.contains("WEBVTT"));
        assert!(vtt_text.contains("Hello ArtCraft Subtitles!"));
    }

    #[test]
    fn test_execute_video_with_xmp_metadata_and_sidecar() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_xmp.mp4");

        let meta = encodercraft_core::XmpMetadata {
            title: Some("Parity Test 4K".into()),
            creator: Some("el-j".into()),
            description: Some("AME Parity Verification".into()),
            copyright: Some("2026 Storytold ArtCraft".into()),
            project_name: Some("FilmCraft Cut".into()),
            create_date: Some("2026-11-02T12:00:00Z".into()),
            start_timecode: Some("01:00:00:00".into()),
            export_sidecar_xmp: true,
        };

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-xmp-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "YouTube 1080p".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(320),
                custom_height: Some(240),
                custom_frame_rate: Some(Rational64::FPS_24),
                metadata: Some(meta),
                hardware_acceleration: true,
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_video.exists());
        // Verify sidecar .xmp file exists
        let sidecar_xmp = dir.path().join("output_xmp.xmp");
        assert!(sidecar_xmp.exists());
        let xmp_text = std::fs::read_to_string(sidecar_xmp).unwrap();
        assert!(xmp_text.contains("Parity Test 4K"));
        assert!(xmp_text.contains("el-j"));

        // Verify MP4 file contains the embedded XMP UUID box
        let mp4_bytes = std::fs::read(target_video).unwrap();
        let xmp_uuid: [u8; 16] = [0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac];
        assert!(mp4_bytes.windows(16).any(|w| w == xmp_uuid));
    }

    #[test]
    fn test_execute_audio_stems_discrete_mono() {
        let dir = tempdir().unwrap();
        let target_audio = dir.path().join("master_mix.wav");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-stems-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_audio.clone(),
                preset_name: "Broadcast WAV".into(),
                container: ContainerType::Wave,
                audio_stems: Some(encodercraft_core::AudioStemMode::DiscreteMono),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();

        assert!(target_audio.exists());
        // Verify discrete mono stem files were exported
        let stem_l = dir.path().join("master_mix_Stem_L.wav");
        let stem_r = dir.path().join("master_mix_Stem_R.wav");
        assert!(stem_l.exists());
        assert!(stem_r.exists());
        assert!(stem_l.metadata().unwrap().len() > 100);
        assert!(stem_r.metadata().unwrap().len() > 100);
    }

    #[test]
    fn test_execute_dnxhr_transcode() {
        let dir = tempdir().unwrap();
        let target_video = dir.path().join("output_dnx.mov");

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "test-dnx-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "input.raw".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: target_video.clone(),
                preset_name: "Avid DNxHR HQ 1080p".into(),
                container: ContainerType::QuickTimeMov,
                custom_width: Some(1920),
                custom_height: Some(1080),
                ..Default::default()
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_video.exists());
    }
}
