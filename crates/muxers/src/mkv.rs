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
    pub fn add_audio_stream(&mut self, cfg: &AudioStreamConfig) -> usize {
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

        self.tracks.push(spec);
        self.tracks.len() - 1
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

        let pts_ns = sample.pts.max(0) * 1_000_000; // Assuming millisecond ticks or timescale
        let dur_ns = Some((sample.duration_ticks as u64) * 1_000_000);

        writer
            .write_frame(track_idx, pts_ns, sample.is_keyframe, sample.data, dur_ns)
            .map_err(|e| EncoderError::Io(format!("failed to write mkv frame: {e}")))
    }

    /// Writes an encoded audio sample into the MKV container.
    pub fn write_audio_sample(&mut self, track_idx: usize, sample: AudioSample) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("mkv writer already finished".into()))?;

        let pts_ns = sample.pts.max(0) * 1_000_000;
        let dur_ns = Some((sample.duration_ticks as u64) * 1_000_000);

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
