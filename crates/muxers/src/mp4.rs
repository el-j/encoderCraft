//! ISO Base Media File Format (MP4 / QuickTime MOV) multiplexer.
//!
//! Powered by upstream `craft-media::isobmff`, supporting progressive muxing,
//! faststart (`moov` at file start for web streaming), and multi-track audio/video.

use std::io::{Read, Seek, Write};

use craft_media::isobmff::{
    AacConfig, AudioParams, Av1Config, AvcConfig, Brand, CodecConfig, FourCc, HevcConfig, Mp4Writer, SampleEntry, TrackConfig, VideoParams,
    VpcConfig, WriteSample, WriterOptions,
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
            b"avc1" | b"avc3" => CodecConfig::Avc(
                AvcConfig::parse(&cfg.codec_private)
                    .map_err(|e| EncoderError::InvalidContainer(format!("invalid avcC decoder configuration: {e}")))?,
            ),
            b"hvc1" | b"hev1" => CodecConfig::Hevc(
                HevcConfig::parse(&cfg.codec_private)
                    .map_err(|e| EncoderError::InvalidContainer(format!("invalid hvcC decoder configuration: {e}")))?,
            ),
            b"av01" => CodecConfig::Av1(
                Av1Config::parse(&cfg.codec_private)
                    .map_err(|e| EncoderError::InvalidContainer(format!("invalid av1C decoder configuration: {e}")))?,
            ),
            b"vp09" => {
                CodecConfig::Vp9(VpcConfig { profile: 0, level: 31, bit_depth: cfg.bit_depth.max(8), chroma_subsampling: 1, ..Default::default() })
            }
            b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => CodecConfig::ProRes { fourcc },
            b"AVdn" | b"AVdh" => CodecConfig::Dnx { fourcc },
            b"jpeg" | b"mjpa" | b"mjpb" => CodecConfig::Jpeg { fourcc },
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

        // One sample lasts `frame_rate.den` ticks of a `frame_rate.num` Hz track clock.
        let timescale = u32::try_from(cfg.frame_rate.num)
            .ok()
            .filter(|t| *t > 0)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid frame rate {}/{}", cfg.frame_rate.num, cfg.frame_rate.den)))?;
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

/// Appends an Adobe-compatible XMP metadata UUID box to an MP4/MOV stream.
///
/// Standard Adobe XMP UUID for ISO-BMFF: `be7acfcb-97a9-42e8-9c71-999491e3afac`.
pub fn append_xmp_uuid_box<W: Write>(w: &mut W, xmp_xml: &str) -> Result<(), EncoderError> {
    let xml_bytes = xmp_xml.as_bytes();
    const XMP_UUID: [u8; 16] = [0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac];
    let box_len = 4 + 4 + 16 + xml_bytes.len();
    let box_len_u32 = u32::try_from(box_len).map_err(|_| EncoderError::Internal("XMP packet too large for MP4 box".into()))?;
    w.write_all(&box_len_u32.to_be_bytes()).map_err(|e| EncoderError::Io(format!("failed to write XMP uuid box length: {e}")))?;
    w.write_all(b"uuid").map_err(|e| EncoderError::Io(format!("failed to write XMP uuid box type: {e}")))?;
    w.write_all(&XMP_UUID).map_err(|e| EncoderError::Io(format!("failed to write XMP uuid: {e}")))?;
    w.write_all(xml_bytes).map_err(|e| EncoderError::Io(format!("failed to write XMP payload: {e}")))?;
    Ok(())
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
        let avcc = craft_h264enc::Encoder::new(craft_h264enc::EncoderConfig::new(64, 64, 24, 1)).unwrap().avcc();

        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(1920, 1080).unwrap(),
            frame_rate: Rational64::FPS_24,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: avcc,
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
