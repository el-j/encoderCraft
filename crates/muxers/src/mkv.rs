//! Matroska (MKV) and WebM multiplexer.
//!
//! Powered by upstream `craft-media::matroska::mux::MkvWriter`, providing
//! EBML cluster chunking, Cues index generation, and SeekHead tables.

use std::io::{Seek, Write};

use craft_media::matroska::{MkvWriter, MuxOptions, TrackKind, TrackSpec};
use encodercraft_core::EncoderError;

use crate::muxer::{AudioSample, AudioStreamConfig, VideoSample, VideoStreamConfig};

/// Matroska (MKV) and WebM container builder and multiplexer.
pub struct MkvMuxer<W: Write + Seek> {
    writer: Option<MkvWriter<W>>,
}

/// Builder for initializing an MKV/WebM multiplexer with its full track manifest.
pub struct MkvMuxerBuilder<W: Write + Seek> {
    writer: W,
    is_webm: bool,
    tracks: Vec<TrackSpec>,
}

impl<W: Write + Seek> MkvMuxerBuilder<W> {
    /// Creates a new builder for MKV (false) or WebM (true).
    pub fn new(writer: W, is_webm: bool) -> Self {
        Self { writer, is_webm, tracks: Vec::new() }
    }

    /// Declares a video stream track.
    pub fn add_video_stream(&mut self, cfg: &VideoStreamConfig) -> usize {
        let codec_id = match &cfg.codec_fourcc {
            b"vp09" | b"VP90" => "V_VP9",
            b"av01" | b"AV01" => "V_AV1",
            b"avc1" | b"h264" => "V_MPEG4/ISO/AVC",
            b"hev1" | b"hvc1" => "V_MPEGH/ISO/HEVC",
            _ => "V_UNCOMPRESSED",
        };

        let mut spec = TrackSpec::new(TrackKind::Video, codec_id);
        spec.video_size = Some((cfg.dimensions.width, cfg.dimensions.height));
        spec.codec_private = cfg.codec_private.clone();

        let fps = cfg.frame_rate;
        if fps.num > 0 {
            let dur_ns = (1_000_000_000u64 * (fps.den as u64)) / (fps.num as u64);
            spec.default_duration_ns = Some(dur_ns);
        }

        self.tracks.push(spec);
        self.tracks.len() - 1
    }

    /// Declares an audio stream track.
    pub fn add_audio_stream(&mut self, cfg: &AudioStreamConfig) -> Result<usize, EncoderError> {
        let codec_id = match &cfg.codec_fourcc {
            b"Opus" | b"opus" => "A_OPUS",
            b"vorb" => "A_VORBIS",
            b"mp4a" | b"aac " => "A_AAC",
            b"flac" => "A_FLAC",
            _ => "A_PCM/INT/LIT",
        };

        let mut spec = TrackSpec::new(TrackKind::Audio, codec_id);
        spec.audio = Some((cfg.sample_rate as f64, cfg.layout as u64, Some(cfg.bits_per_sample as u64)));
        spec.codec_private = cfg.codec_private.clone();
        if codec_id == "A_OPUS" {
            let header = cfg
                .codec_private
                .get(..19)
                .filter(|header| header.starts_with(b"OpusHead"))
                .ok_or_else(|| EncoderError::InvalidJobSpec("Opus track is missing a valid OpusHead header".into()))?;
            let channel_count = u8::try_from(cfg.layout.channel_count())
                .map_err(|_| EncoderError::InvalidJobSpec("Opus channel count exceeds the header limit".into()))?;
            if cfg.sample_rate != 48_000
                || !matches!(cfg.layout, encodercraft_core::ChannelLayout::Mono | encodercraft_core::ChannelLayout::Stereo)
                || header.get(8).copied() != Some(1)
                || header.get(9).copied() != Some(channel_count)
            {
                return Err(EncoderError::InvalidJobSpec("Opus tracks require matching mono/stereo layout at 48 kHz".into()));
            }
            let preskip = u16::from_le_bytes([
                *header.get(10).ok_or_else(|| EncoderError::InvalidJobSpec("OpusHead is missing its pre-skip".into()))?,
                *header.get(11).ok_or_else(|| EncoderError::InvalidJobSpec("OpusHead is missing its pre-skip".into()))?,
            ]);
            spec.codec_delay_ns = u64::from(preskip).saturating_mul(1_000_000_000) / 48_000;
            spec.seek_pre_roll_ns = 80_000_000;
        }

        self.tracks.push(spec);
        Ok(self.tracks.len() - 1)
    }

    /// Builds the active `MkvMuxer` writing the initial EBML header and Track entries.
    pub fn build(self) -> Result<MkvMuxer<W>, EncoderError> {
        let mut opts = MuxOptions::default();
        if self.is_webm {
            opts.doc_type = "webm".into();
        } else {
            opts.doc_type = "matroska".into();
        }

        let writer = MkvWriter::new(self.writer, self.tracks, opts).map_err(|e| EncoderError::Io(format!("failed to start mkv writer: {e}")))?;

        Ok(MkvMuxer { writer: Some(writer) })
    }
}

impl<W: Write + Seek> MkvMuxer<W> {
    /// Writes an encoded video sample into the MKV container.
    pub fn write_video_sample(&mut self, track_idx: usize, sample: VideoSample) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mkv writer already finished".into()))?;

        // Sample times are microsecond ticks.
        let pts_ns = sample.pts.max(0).saturating_mul(1_000);
        let dur_ns = Some(u64::from(sample.duration_ticks).saturating_mul(1_000));

        writer
            .write_frame(track_idx, pts_ns, sample.is_keyframe, sample.data, dur_ns)
            .map_err(|e| EncoderError::Io(format!("failed to write mkv frame: {e}")))
    }

    /// Writes an encoded audio sample into the MKV container.
    pub fn write_audio_sample(&mut self, track_idx: usize, sample: AudioSample) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mkv writer already finished".into()))?;

        let pts_ns = sample.pts.max(0).saturating_mul(1_000);
        let dur_ns = Some(u64::from(sample.duration_ticks).saturating_mul(1_000));

        writer
            .write_frame(track_idx, pts_ns, true, sample.data, dur_ns)
            .map_err(|e| EncoderError::Io(format!("failed to write mkv audio frame: {e}")))
    }

    /// Finalizes the MKV container, back-patching Cues index and Cluster offsets.
    pub fn finish(mut self) -> Result<W, EncoderError> {
        let writer = self.writer.take().ok_or_else(|| EncoderError::Internal("mkv writer already finished".into()))?;

        writer.finish().map_err(|e| EncoderError::Io(format!("mkv finish failed: {e}")))
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
    fn opus_audio_track_records_preskip_and_rejects_malformed_headers() {
        let mut header = vec![0; 19];
        header[..8].copy_from_slice(b"OpusHead");
        header[8] = 1;
        header[9] = 2;
        header[10..12].copy_from_slice(&120u16.to_le_bytes());
        let config = AudioStreamConfig {
            codec_fourcc: *b"Opus",
            sample_rate: 48_000,
            layout: encodercraft_core::ChannelLayout::Stereo,
            bits_per_sample: 16,
            codec_private: header,
        };
        let mut builder = MkvMuxerBuilder::new(Cursor::new(Vec::new()), true);
        assert_eq!(builder.add_audio_stream(&config).unwrap(), 0);
        assert_eq!(builder.tracks[0].codec_delay_ns, 2_500_000);
        assert_eq!(builder.tracks[0].seek_pre_roll_ns, 80_000_000);

        let mut invalid_builder = MkvMuxerBuilder::new(Cursor::new(Vec::new()), true);
        let invalid = AudioStreamConfig { codec_private: Vec::new(), ..config };
        assert!(invalid_builder.add_audio_stream(&invalid).is_err());
    }

    #[test]
    fn test_mkv_muxer_video_writing() {
        let cursor = Cursor::new(Vec::new());
        let mut builder = MkvMuxerBuilder::new(cursor, false);

        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"av01",
            dimensions: Dimensions::new(1280, 720).unwrap(),
            frame_rate: Rational64::FPS_24,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![],
            spatial: None,
        };
        let track_idx = builder.add_video_stream(&v_cfg);
        assert_eq!(track_idx, 0);

        let mut muxer = builder.build().unwrap();

        let frame = [0x01, 0x02, 0x03, 0x04];
        muxer.write_video_sample(track_idx, VideoSample { data: &frame, pts: 0, dts: 0, duration_ticks: 40, is_keyframe: true }).unwrap();

        let finished: Cursor<Vec<u8>> = muxer.finish().unwrap();
        let bytes = finished.into_inner();
        assert!(!bytes.is_empty());
        // EBML magic bytes: 0x1A, 0x45, 0xDF, 0xA3
        assert_eq!(&bytes[0..4], &[0x1A, 0x45, 0xDF, 0xA3]);
    }
}
