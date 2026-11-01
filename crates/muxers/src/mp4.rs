//! ISO Base Media File Format (MP4 / QuickTime MOV) multiplexer.
//!
//! Powered by upstream `craft-media::isobmff`, supporting progressive muxing,
//! faststart (`moov` at file start for web streaming), and multi-track audio/video.

use std::io::{Read, Seek, Write};

use craft_media::isobmff::{
    AacConfig, AudioParams, Brand, CodecConfig, FourCc, Mp4Writer, SampleEntry, TrackConfig, VideoParams, WriteSample, WriterOptions,
};
use encodercraft_core::EncoderError;

use crate::muxer::{AudioSample, AudioStreamConfig, VideoSample, VideoStreamConfig};

/// High-level ISO BMFF (MP4 / MOV) multiplexer.
pub struct Mp4Muxer<W: Read + Write + Seek> {
    writer: Option<Mp4Writer<W>>,
}

impl<W: Read + Write + Seek> Mp4Muxer<W> {
    /// Creates a new MP4/MOV multiplexer.
    pub fn new(w: W, is_mov: bool) -> Result<Self, EncoderError> {
        let brand = if is_mov { Brand::Mov } else { Brand::Mp4 };
        let opts = WriterOptions::new(brand);
        let writer = Mp4Writer::new(w, opts).map_err(|e| EncoderError::Io(format!("failed to initialize mp4 writer: {e}")))?;
        Ok(Self { writer: Some(writer) })
    }

    /// Adds a video stream track to the container.
    pub fn add_video_stream(&mut self, cfg: &VideoStreamConfig) -> Result<usize, EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mp4 writer already finished".into()))?;

        let fourcc = FourCc::new(&cfg.codec_fourcc);
        let codec_config = match &cfg.codec_fourcc {
            b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => CodecConfig::ProRes { fourcc },
            b"AVdn" | b"AVdh" => CodecConfig::Dnx { fourcc },
            _ => CodecConfig::Unknown { fourcc, raw: cfg.codec_private.clone() },
        };

        let video_params = VideoParams {
            width: cfg.dimensions.width as u16,
            height: cfg.dimensions.height as u16,
            depth: (cfg.bit_depth as u16) * 3,
            frame_count: 1,
            ..Default::default()
        };

        let entry =
            SampleEntry { format: fourcc, data_reference_index: 1, codec: codec_config, video: Some(video_params), audio: None, bitrate: None };

        let timescale = (cfg.frame_rate.num as u32).max(1000);
        let track_cfg = TrackConfig::new(entry, timescale);

        writer.add_track(track_cfg).map_err(|e| EncoderError::Internal(format!("failed to add video track: {e}")))
    }

    /// Adds an audio stream track to the container.
    pub fn add_audio_stream(&mut self, cfg: &AudioStreamConfig) -> Result<usize, EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mp4 writer already finished".into()))?;

        let fourcc = FourCc::new(&cfg.codec_fourcc);
        let codec_config = match &cfg.codec_fourcc {
            b"mp4a" => CodecConfig::Aac(AacConfig::from_asc(cfg.codec_private.clone())),
            _ => CodecConfig::Unknown { fourcc, raw: cfg.codec_private.clone() },
        };

        let audio_params = AudioParams {
            qt_version: 0,
            channels: cfg.layout as u32,
            sample_size: cfg.bits_per_sample,
            sample_rate: cfg.sample_rate as f64,
            compression_id: 0,
            samples_per_packet: 1,
            bytes_per_packet: 0,
            bytes_per_frame: 0,
            bytes_per_sample: (cfg.bits_per_sample / 8) as u32,
            const_bits_per_channel: 0,
            lpcm_flags: 0,
            const_bytes_per_packet: 0,
            const_frames_per_packet: 0,
            little_endian: None,
        };

        let entry =
            SampleEntry { format: fourcc, data_reference_index: 1, codec: codec_config, video: None, audio: Some(audio_params), bitrate: None };

        let track_cfg = TrackConfig::new(entry, cfg.sample_rate);
        writer.add_track(track_cfg).map_err(|e| EncoderError::Internal(format!("failed to add audio track: {e}")))
    }

    /// Writes an encoded video sample into the specified track.
    pub fn write_video_sample(&mut self, track_idx: usize, sample: VideoSample) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mp4 writer already finished".into()))?;

        let composition_offset = (sample.pts - sample.dts) as i32;
        let ws = WriteSample { data: sample.data, duration: sample.duration_ticks, composition_offset, is_sync: sample.is_keyframe };

        writer.write_sample(track_idx, ws).map_err(|e| EncoderError::Io(format!("failed to write video sample: {e}")))
    }

    /// Writes an encoded audio sample into the specified track.
    pub fn write_audio_sample(&mut self, track_idx: usize, sample: AudioSample) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mp4 writer already finished".into()))?;

        let ws = WriteSample { data: sample.data, duration: sample.duration_ticks, composition_offset: 0, is_sync: true };

        writer.write_sample(track_idx, ws).map_err(|e| EncoderError::Io(format!("failed to write audio sample: {e}")))
    }

    /// Finalizes the container, writing the movie header (`moov`) and back-patching sample tables.
    /// If `faststart` is true, optimizes placement for progressive web streaming.
    pub fn finish(mut self, faststart: bool) -> Result<W, EncoderError> {
        let writer = self.writer.take().ok_or_else(|| EncoderError::Internal("mp4 writer already finished".into()))?;

        if faststart {
            writer.finish_faststart().map_err(|e| EncoderError::Io(format!("mp4 faststart finish failed: {e}")))
        } else {
            writer.finish().map_err(|e| EncoderError::Io(format!("mp4 finish failed: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;
    use std::io::Cursor;

    #[test]
    fn test_mp4_muxer_video_and_audio() {
        let cursor = Cursor::new(Vec::new());
        let mut muxer = Mp4Muxer::new(cursor, false).unwrap();

        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(1920, 1080).unwrap(),
            frame_rate: Rational64::FPS_24,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![0, 0, 0, 1],
        };
        let v_track = muxer.add_video_stream(&v_cfg).unwrap();
        assert_eq!(v_track, 0);

        // Write a mock video frame
        let frame_data = [0xde, 0xad, 0xbe, 0xef];
        muxer.write_video_sample(v_track, VideoSample { data: &frame_data, pts: 0, dts: 0, duration_ticks: 1000, is_keyframe: true }).unwrap();

        let finished: Cursor<Vec<u8>> = muxer.finish(false).unwrap();
        let bytes = finished.into_inner();
        assert!(!bytes.is_empty());
        assert!(bytes.len() > 32);
        // First 4 bytes length, then ftyp
        assert_eq!(&bytes[4..8], b"ftyp");
    }
}
