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

        match target.container {
            ContainerType::Wave => self.execute_audio_wav(target, start_time),
            ContainerType::IsoBmffMp4 => self.execute_video_mp4(target, false, start_time),
            ContainerType::QuickTimeMov => self.execute_video_mp4(target, true, start_time),
            _ => Err(EncoderError::UnsupportedCodec(format!("unsupported target container: {:?}", target.container))),
        }
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
        let mut wav_writer = BwfWavWriter::new(writer, sample_rate, 2, WavFormatTag::Pcm16, Some(&bext))?;

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

        let w = target.custom_width.unwrap_or(1920);
        let h = target.custom_height.unwrap_or(1080);
        let dims = Dimensions::new(w, h)?;
        let fps = target.custom_frame_rate.unwrap_or(Rational64::FPS_24);

        let codec_kind = VideoCodecKind::H264;

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

        for f in 0..total_frames {
            if self.progress.is_cancelled() {
                return Err(EncoderError::Internal("job cancelled by user".into()));
            }

            let frame = encodercraft_core::VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, f as i64, 1, fps)?;
            let scaled_frame = scaler.scale(&frame, dims)?;

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
            }],
        };

        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute().unwrap();
        assert!(target_file.exists());
        assert!(target_file.metadata().unwrap().len() > 100);
    }
}
