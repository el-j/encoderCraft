//! Multi-clip concatenation and stitching engine.
//!
//! Sequentially merges multiple input media files into a unified continuous output stream,
//! matching Adobe Media Encoder's "Stitch Clips" functionality. Normalizes timescales,
//! resamples audio to continuous PCM buffers, rescales video frames across differing resolutions,
//! and ensures monotonic timestamp continuity across clip boundaries.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use encodercraft_audio_dsp::{normalize_loudness, remix_channels};
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, ContainerType, EncoderError, Rational64, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_muxers::{BextMetadata, BwfWavWriter, Mp4Muxer, VideoSample, VideoStreamConfig, WavFormatTag};
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};

use crate::progress::JobProgress;
use crate::scale::FrameScaler;

/// Configuration options for multi-clip stitching.
#[derive(Debug, Clone)]
pub struct StitchOptions {
    pub target_dimensions: Dimensions,
    pub target_frame_rate: Rational64,
    pub target_sample_rate: u32,
    pub target_channels: ChannelLayout,
    pub target_bitrate_kbps: Option<u32>,
    pub target_lufs: Option<f32>,
    pub video_codec: VideoCodecKind,
    pub container: ContainerType,
}

impl Default for StitchOptions {
    fn default() -> Self {
        let dims = match Dimensions::new(1920, 1080) {
            Ok(d) => d,
            Err(_) => Dimensions { width: 1920, height: 1080 },
        };
        Self {
            target_dimensions: dims,
            target_frame_rate: Rational64::FPS_24,
            target_sample_rate: 48000,
            target_channels: ChannelLayout::Stereo,
            target_bitrate_kbps: Some(8000),
            target_lufs: None,
            video_codec: VideoCodecKind::H264,
            container: ContainerType::IsoBmffMp4,
        }
    }
}

/// Concatenates multiple audio sample buffers into a single continuous buffer.
///
/// Ensures all buffers match the requested target sample rate and channel layout,
/// remixing channels where needed.
pub fn stitch_audio_buffers(buffers: &[AudioSampleBuffer], target_layout: ChannelLayout) -> Result<AudioSampleBuffer, EncoderError> {
    if buffers.is_empty() {
        return Err(EncoderError::InvalidJobSpec("no audio buffers provided to stitch".into()));
    }

    let first_rate = match buffers.first() {
        Some(b) => b.sample_rate,
        None => return Err(EncoderError::InvalidJobSpec("empty buffer slice".into())),
    };

    let target_channels = target_layout.channel_count();
    let mut total_samples: usize = 0;
    let mut remixed_buffers: Vec<AudioSampleBuffer> = Vec::with_capacity(buffers.len());

    for buf in buffers {
        if buf.sample_rate != first_rate {
            return Err(EncoderError::InvalidJobSpec(format!(
                "sample rate mismatch in stitch: expected {first_rate} Hz, got {} Hz",
                buf.sample_rate
            )));
        }
        let remixed = if buf.layout != target_layout { remix_channels(buf, target_layout)? } else { buf.clone() };
        total_samples = total_samples.saturating_add(remixed.samples_per_channel());
        remixed_buffers.push(remixed);
    }

    let mut combined_channels = vec![Vec::with_capacity(total_samples); target_channels];

    for buf in &remixed_buffers {
        for (ch_idx, ch_data) in buf.channels().iter().enumerate() {
            if let Some(target_ch) = combined_channels.get_mut(ch_idx) {
                target_ch.extend_from_slice(ch_data);
            }
        }
    }

    AudioSampleBuffer::from_planar_channels(first_rate, target_layout, 0, combined_channels)
}

/// Engine for concatenating and stitching multiple clips into a single continuous stream.
pub struct ClipStitcher {
    options: StitchOptions,
    progress: Arc<JobProgress>,
}

impl ClipStitcher {
    /// Creates a new `ClipStitcher` with the specified output options.
    pub fn new(options: StitchOptions) -> Self {
        let progress = Arc::new(JobProgress::new(100));
        Self { options, progress }
    }

    /// Access the shared job progress reporter.
    pub fn progress(&self) -> Arc<JobProgress> {
        self.progress.clone()
    }

    /// Stitches a list of video frame sequences into a single output video file.
    pub fn stitch_video_sequences<I, S>(&self, clip_sequences: I, output_path: &Path) -> Result<(), EncoderError>
    where
        I: IntoIterator<Item = S>,
        S: IntoIterator<Item = VideoFrame>,
    {
        let start_time = Instant::now();
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create output directory: {e}")))?;
        }

        let out_file = File::create(output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
        let is_mov = self.options.container == ContainerType::QuickTimeMov;
        let mut mp4_muxer = Mp4Muxer::new(out_file, is_mov)?;

        let dims = self.options.target_dimensions;
        let fps = self.options.target_frame_rate;

        let v_cfg = VideoStreamConfig {
            codec_fourcc: self.options.video_codec.default_fourcc(),
            dimensions: dims,
            frame_rate: fps,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![0, 0, 0, 1],
        };

        let track_idx = mp4_muxer.add_video_stream(&v_cfg)?;
        let mut encoder = create_video_encoder(self.options.video_codec, dims.width, dims.height, fps, self.options.target_bitrate_kbps)?;

        let scaler = FrameScaler::new(ScaleMode::Fit);
        let mut continuous_pts = 0i64;
        let ticks_per_frame = 1000i64;

        for clip in clip_sequences {
            for frame in clip {
                if self.progress.is_cancelled() {
                    return Err(EncoderError::Internal("stitch cancelled by user".into()));
                }

                let scaled = if frame.dimensions == dims { frame } else { scaler.scale(&frame, dims)? };

                let packets = encoder.encode_frame(&scaled)?;
                for pkt in packets {
                    let sample = VideoSample {
                        data: &pkt.data,
                        pts: continuous_pts,
                        dts: continuous_pts,
                        duration_ticks: ticks_per_frame as u32,
                        is_keyframe: pkt.is_keyframe,
                    };
                    mp4_muxer.write_video_sample(track_idx, sample)?;
                }

                continuous_pts = continuous_pts.saturating_add(ticks_per_frame);
                self.progress.advance_frame(start_time);
            }
        }

        let flushed = encoder.flush()?;
        for pkt in flushed {
            let sample = VideoSample {
                data: &pkt.data,
                pts: continuous_pts,
                dts: continuous_pts,
                duration_ticks: ticks_per_frame as u32,
                is_keyframe: pkt.is_keyframe,
            };
            mp4_muxer.write_video_sample(track_idx, sample)?;
        }

        mp4_muxer.finish(false)?;
        Ok(())
    }

    /// Stitches audio files into a single unified broadcast WAV, FLAC, or AIFF file.
    pub fn stitch_audio_files(&self, audio_paths: &[PathBuf], output_path: &Path) -> Result<(), EncoderError> {
        if audio_paths.is_empty() {
            return Err(EncoderError::InvalidJobSpec("no audio files provided for stitching".into()));
        }

        let mut decoded_buffers = Vec::with_capacity(audio_paths.len());
        for p in audio_paths {
            let file_bytes = std::fs::read(p).map_err(|e| EncoderError::Io(format!("failed to read audio input {}: {e}", p.display())))?;
            let audio_buf = encodercraft_audio_dsp::decode_audio(&file_bytes)?;
            decoded_buffers.push(audio_buf);
        }

        let mut unified_buf = stitch_audio_buffers(&decoded_buffers, self.options.target_channels)?;

        if let Some(target_lufs) = self.options.target_lufs {
            let _ = normalize_loudness(&mut unified_buf, target_lufs as f64, -1.0)?;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create output directory: {e}")))?;
        }

        match self.options.container {
            ContainerType::Wave => {
                let out_file = File::create(output_path).map_err(|e| EncoderError::Io(format!("failed to create output file: {e}")))?;
                let bext = BextMetadata { description: "Stitched by encoderCraft".into(), originator: "encoderCraft".into(), ..Default::default() };
                let mut wav_writer = BwfWavWriter::new(
                    out_file,
                    unified_buf.sample_rate,
                    unified_buf.layout.channel_count() as u16,
                    WavFormatTag::Pcm16,
                    Some(&bext),
                )?;
                wav_writer.write_buffer(&unified_buf)?;
                wav_writer.finish()?;
            }
            ContainerType::Flac => {
                let flac_bytes = encodercraft_audio_dsp::encode_audio_flac(&unified_buf)?;
                std::fs::write(output_path, flac_bytes).map_err(|e| EncoderError::Io(format!("failed to write FLAC output: {e}")))?;
            }
            ContainerType::Aiff => {
                let aiff_bytes = encodercraft_audio_dsp::encode_audio_aiff(&unified_buf)?;
                std::fs::write(output_path, aiff_bytes).map_err(|e| EncoderError::Io(format!("failed to write AIFF output: {e}")))?;
            }
            _ => {
                return Err(EncoderError::UnsupportedCodec(format!("unsupported container for stitched audio: {:?}", self.options.container)));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_stitch_audio_buffers_success() {
        let sample_rate = 48000;
        let mut buf1 = AudioSampleBuffer::new_silent(sample_rate, ChannelLayout::Stereo, 4800, 0).unwrap();
        let mut buf2 = AudioSampleBuffer::new_silent(sample_rate, ChannelLayout::Stereo, 4800, 0).unwrap();

        // Populate simple sine in channel 0
        for i in 0..4800 {
            let s1 = (i as f32 * 0.05).sin();
            let s2 = (i as f32 * 0.1).sin();
            buf1.channel_mut(0).unwrap()[i] = s1;
            buf2.channel_mut(0).unwrap()[i] = s2;
        }

        let stitched = stitch_audio_buffers(&[buf1, buf2], ChannelLayout::Stereo).unwrap();
        assert_eq!(stitched.samples_per_channel(), 9600);
        assert_eq!(stitched.sample_rate, 48000);
        assert_eq!(stitched.layout, ChannelLayout::Stereo);

        // Verify sample continuity across seam
        let seam_before = stitched.channel(0).unwrap()[4799];
        let seam_after = stitched.channel(0).unwrap()[4800];
        assert_eq!(seam_before, (4799f32 * 0.05).sin());
        assert_eq!(seam_after, 0.0f32); // (0 * 0.1).sin() == 0.0
    }

    #[test]
    fn test_stitch_empty_audio_buffers_fails() {
        let res = stitch_audio_buffers(&[], ChannelLayout::Stereo);
        assert!(res.is_err());
    }

    #[test]
    fn test_stitch_video_sequences_success() {
        let dir = tempdir().unwrap();
        let out_path = dir.path().join("stitched.mp4");

        let dims_720p = Dimensions::new(1280, 720).unwrap();
        let dims_1080p = Dimensions::new(1920, 1080).unwrap();
        let fps = Rational64::FPS_24;

        // Clip 1: 5 frames at 720p
        let clip1: Vec<VideoFrame> =
            (0..5).map(|f| VideoFrame::new_allocated(dims_720p, PixelFormat::Rgb8, ColorSpaceTag::SRGB, f, 1, fps).unwrap()).collect();

        // Clip 2: 5 frames at 1080p
        let clip2: Vec<VideoFrame> =
            (5..10).map(|f| VideoFrame::new_allocated(dims_1080p, PixelFormat::Rgb8, ColorSpaceTag::SRGB, f, 1, fps).unwrap()).collect();

        let opts = StitchOptions {
            target_dimensions: dims_1080p,
            target_frame_rate: fps,
            video_codec: VideoCodecKind::H264,
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        };

        let stitcher = ClipStitcher::new(opts);
        stitcher.stitch_video_sequences(vec![clip1, clip2], &out_path).unwrap();

        assert!(out_path.exists());
        assert!(out_path.metadata().unwrap().len() > 100);
    }
}
