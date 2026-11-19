//! Clean-room Audio Video Interleave (AVI) multiplexer.
//!
//! Backed by `craft_media::avi::AviWriter`, supporting video (MPEG-4 SP, H.264, FFV1, MPEG-2)
//! and audio (PCM, MP3, AC-3) with OpenDML large file indexing.

use std::io::{Seek, Write};

use craft_media::avi::{AudioSpec, AudioTiming, AviWriter, MuxOptions, StreamSpec, VideoSpec, WaveFormat};
use encodercraft_core::EncoderError;

use crate::muxer::{AudioSample, AudioStreamConfig, VideoSample, VideoStreamConfig};

/// High-level AVI container multiplexer with OpenDML extension indexing.
pub struct AviMuxer<W: Write + Seek> {
    writer: Option<AviWriter<W>>,
}

/// Builder for constructing an [`AviMuxer`].
pub struct AviMuxerBuilder<W: Write + Seek> {
    sink: W,
    specs: Vec<StreamSpec>,
    opts: MuxOptions,
}

impl<W: Write + Seek> AviMuxerBuilder<W> {
    /// Creates a new builder writing to `sink`.
    pub fn new(sink: W) -> Self {
        Self { sink, specs: Vec::new(), opts: MuxOptions::default() }
    }

    /// Configures custom mux options (e.g. riff_limit).
    pub fn with_options(mut self, opts: MuxOptions) -> Self {
        self.opts = opts;
        self
    }

    /// Adds a video stream to the AVI container.
    pub fn add_video_stream(&mut self, cfg: &VideoStreamConfig) -> usize {
        let rate = u32::try_from(cfg.frame_rate.num.max(1)).unwrap_or(24);
        let scale = u32::try_from(cfg.frame_rate.den.max(1)).unwrap_or(1);
        let mut v_spec = VideoSpec::new(cfg.codec_fourcc, cfg.dimensions.width, cfg.dimensions.height, rate, scale);
        if !cfg.codec_private.is_empty() {
            v_spec.format.extra = cfg.codec_private.clone();
        }
        self.specs.push(StreamSpec::Video(v_spec));
        self.specs.len() - 1
    }

    /// Adds an audio stream to the AVI container.
    pub fn add_audio_stream(&mut self, cfg: &AudioStreamConfig) -> usize {
        let channels = (cfg.layout.channel_count() as u16).max(1);
        let bits = cfg.bits_per_sample.max(16);
        let format_tag = match &cfg.codec_fourcc {
            b"mp3 " | b".mp3" => 0x55,
            b"ac-3" | b"ac3 " => 0x2000,
            _ => 1, // PCM
        };

        let timing = match format_tag {
            0x55 => AudioTiming::Packets { samples_per_packet: 1152 },
            0x2000 => AudioTiming::Packets { samples_per_packet: 1536 },
            _ => AudioTiming::Bytes,
        };

        let block_align = channels.saturating_mul(bits.div_ceil(8));
        let avg_bytes_per_sec = cfg.sample_rate.saturating_mul(u32::from(block_align));

        let a_spec = AudioSpec {
            format: WaveFormat {
                format_tag,
                channels,
                sample_rate: cfg.sample_rate.max(8000),
                avg_bytes_per_sec,
                block_align,
                bits_per_sample: bits,
                extra: cfg.codec_private.clone(),
            },
            timing,
            name: None,
        };

        self.specs.push(StreamSpec::Audio(a_spec));
        self.specs.len() - 1
    }

    /// Builds the active [`AviMuxer`].
    pub fn build(self) -> Result<AviMuxer<W>, EncoderError> {
        let writer =
            AviWriter::new(self.sink, self.specs, self.opts).map_err(|e| EncoderError::Io(format!("failed to initialize AVI writer: {e}")))?;
        Ok(AviMuxer { writer: Some(writer) })
    }
}

impl<W: Write + Seek> AviMuxer<W> {
    /// Writes an elementary video sample packet.
    pub fn write_video_sample(&mut self, stream_index: usize, sample: VideoSample<'_>) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("AVI writer already finished".into()))?;
        writer
            .write_chunk(stream_index, sample.data, sample.is_keyframe)
            .map_err(|e| EncoderError::Io(format!("failed to write AVI video chunk: {e}")))
    }

    /// Writes an elementary audio sample packet.
    pub fn write_audio_sample(&mut self, stream_index: usize, sample: AudioSample<'_>) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("AVI writer already finished".into()))?;
        writer.write_chunk(stream_index, sample.data, true).map_err(|e| EncoderError::Io(format!("failed to write AVI audio chunk: {e}")))
    }

    /// Writes raw audio bytes.
    pub fn write_audio_bytes(&mut self, stream_index: usize, data: &[u8]) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("AVI writer already finished".into()))?;
        writer.write_chunk(stream_index, data, true).map_err(|e| EncoderError::Io(format!("failed to write AVI audio chunk: {e}")))
    }

    /// Finalizes the AVI file writing index tables and updating headers.
    pub fn finish(mut self) -> Result<W, EncoderError> {
        let writer = self.writer.take().ok_or_else(|| EncoderError::Internal("AVI writer already finished".into()))?;
        writer.finish().map_err(|e| EncoderError::Io(format!("failed to finish AVI file: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_core::{ChannelLayout, Rational64};
    use encodercraft_geom::Dimensions;
    use std::io::Cursor;

    #[test]
    fn test_avi_muxer_video_and_audio() {
        let cursor = Cursor::new(Vec::new());
        let mut builder = AviMuxerBuilder::new(cursor);

        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"FMP4",
            dimensions: Dimensions::new(640, 480).unwrap(),
            frame_rate: Rational64::FPS_24,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: Vec::new(),
            spatial: None,
        };
        let v_idx = builder.add_video_stream(&v_cfg);
        assert_eq!(v_idx, 0);

        let a_cfg = AudioStreamConfig {
            codec_fourcc: *b"lpcm",
            sample_rate: 48000,
            layout: ChannelLayout::Stereo,
            bits_per_sample: 16,
            codec_private: Vec::new(),
        };
        let a_idx = builder.add_audio_stream(&a_cfg);
        assert_eq!(a_idx, 1);

        let mut muxer = builder.build().unwrap();

        let frame = [0u8; 100];
        muxer.write_video_sample(v_idx, VideoSample { data: &frame, pts: 0, dts: 0, duration_ticks: 1, is_keyframe: true }).unwrap();

        let pcm = [0u8; 400];
        muxer.write_audio_bytes(a_idx, &pcm).unwrap();

        let finished = muxer.finish().unwrap();
        let bytes = finished.into_inner();
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"RIFF"));
        assert_eq!(&bytes[8..12], b"AVI ");
    }
}
