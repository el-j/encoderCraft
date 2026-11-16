//! Material eXchange Format (SMPTE ST 377-1 MXF OP1a and OP-Atom) multiplexer.
//!
//! Powered by `filmcraft-mxf`, providing frame-wrapped broadcast OP1a multiplexing
//! (Avid DNxHD/DNxHR VC-3, Apple ProRes RDD 44, H.264/AVC ST 381-3, and Broadcast Wave PCM ST 382)
//! as well as clip-wrapped OP-Atom essence containers for Avid Media Composer workflows.

use std::io::{Seek, Write};

use encodercraft_core::{AudioSampleBuffer, EncoderError};
use filmcraft_mxf::{
    CodedKind, ColorSpace, FrameInfo, MxfWriter, PackageIds, Pattern, PictureCoding, PictureDesc, Rational, SoundDesc, StartTimecode, WriterConfig,
    encode_pcm,
};

use crate::muxer::{AudioStreamConfig, VideoSample, VideoStreamConfig};

/// High-level SMPTE ST 377-1 MXF multiplexer supporting OP1a and OP-Atom.
pub struct MxfMuxer<W: Write + Seek> {
    writer: Option<MxfWriter<W>>,
}

impl<W: Write + Seek> MxfMuxer<W> {
    /// Creates a new MXF OP1a (broadcast frame-wrapped) or OP-Atom (clip-wrapped) multiplexer.
    pub fn new(
        sink: W,
        is_op_atom: bool,
        package_name: &str,
        video_cfg: Option<&VideoStreamConfig>,
        audio_cfgs: &[AudioStreamConfig],
    ) -> Result<Self, EncoderError> {
        let pattern = if is_op_atom { Pattern::OpAtom } else { Pattern::Op1a };

        let edit_rate = if let Some(v) = video_cfg {
            let num = i32::try_from(v.frame_rate.num.max(1)).unwrap_or(24);
            let den = i32::try_from(v.frame_rate.den.max(1)).unwrap_or(1);
            Rational::new(num, den)
        } else if let Some(a) = audio_cfgs.first() {
            let sr = i32::try_from(a.sample_rate.max(1)).unwrap_or(48_000);
            Rational::new(sr, 1)
        } else {
            return Err(EncoderError::InvalidJobSpec("MXF muxer requires at least one video or audio track".into()));
        };

        let ids = PackageIds::from_seed(&format!("encodercraft:{package_name}"), package_name);
        let mut cfg = WriterConfig::new(pattern, edit_rate, ids);
        cfg.company = "ArtCraft".into();
        cfg.product = "encoderCraft".into();
        cfg.timecode = Some(StartTimecode { frames: 0, rate: edit_rate, drop_frame: false });

        if let Some(v) = video_cfg {
            let coding = match &v.codec_fourcc {
                b"AVdn" | b"AVdh" => {
                    let cid = if v.bit_depth >= 10 { 1271 } else { 1272 };
                    PictureCoding::Vc3 { cid }
                }
                b"apco" => PictureCoding::ProRes { profile: 1 },
                b"apcs" => PictureCoding::ProRes { profile: 2 },
                b"apcn" => PictureCoding::ProRes { profile: 3 },
                b"apch" => PictureCoding::ProRes { profile: 4 },
                b"ap4h" => PictureCoding::ProRes { profile: 5 },
                b"ap4x" => PictureCoding::ProRes { profile: 6 },
                _ => PictureCoding::Avc { profile_idc: 100, intra: false },
            };

            let mut pic = PictureDesc::new(coding, v.dimensions.width, v.dimensions.height);
            pic.color = ColorSpace::Rec709;
            cfg.picture = Some(pic);
        }

        if !is_op_atom || video_cfg.is_none() {
            for a in audio_cfgs {
                let channels = (a.layout.channel_count() as u32).max(1);
                let bits = u32::from(a.bits_per_sample).clamp(8, 32);
                cfg.sound.push(SoundDesc { sample_rate: a.sample_rate.max(1), channels, bits });
            }
        }

        let writer = MxfWriter::new(sink, cfg).map_err(|e| EncoderError::Io(format!("failed to initialize MXF writer: {e}")))?;
        Ok(Self { writer: Some(writer) })
    }

    /// Writes an encoded video frame packet into the MXF picture track.
    pub fn write_video_sample(&mut self, sample: VideoSample<'_>) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("MXF writer already finished".into()))?;
        let info =
            FrameInfo { key: sample.is_keyframe, kind: if sample.is_keyframe { CodedKind::Intra } else { CodedKind::Predicted }, display: None };
        writer.push_picture(sample.data, info).map_err(|e| EncoderError::Io(format!("failed to write MXF video frame: {e}")))
    }

    /// Writes interleaved little-endian PCM bytes into the specified sound track index.
    pub fn write_pcm_audio(&mut self, track_index: usize, pcm_bytes: &[u8]) -> Result<(), EncoderError> {
        let writer = self.writer.as_mut().ok_or_else(|| EncoderError::Internal("MXF writer already finished".into()))?;
        writer.push_sound(track_index, pcm_bytes).map_err(|e| EncoderError::Io(format!("failed to write MXF PCM audio: {e}")))
    }

    /// Quantizes and interleaves an [`AudioSampleBuffer`] into little-endian PCM and writes it to `track_index`.
    pub fn write_audio_buffer(&mut self, track_index: usize, buffer: &AudioSampleBuffer, bits_per_sample: u32) -> Result<(), EncoderError> {
        let pcm = encode_pcm(buffer.channels(), bits_per_sample.clamp(8, 32));
        self.write_pcm_audio(track_index, &pcm)
    }

    /// Finalizes the MXF partitions, index table segments, random index pack, and header metadata.
    pub fn finish(mut self) -> Result<W, EncoderError> {
        let writer = self.writer.take().ok_or_else(|| EncoderError::Internal("MXF writer already finished".into()))?;
        writer.finish().map_err(|e| EncoderError::Io(format!("failed to finalize MXF file: {e}")))
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
    fn test_mxf_op1a_dnxhr_and_pcm_roundtrip() {
        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"AVdh",
            dimensions: Dimensions::new(1920, 1080).unwrap(),
            frame_rate: Rational64::FPS_24,
            pixel_format: PixelFormat::Yuv422p10,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 10,
            codec_private: Vec::new(),
            spatial: None,
        };
        let a_cfg = AudioStreamConfig {
            codec_fourcc: *b"sowt",
            sample_rate: 48_000,
            layout: ChannelLayout::Stereo,
            bits_per_sample: 24,
            codec_private: Vec::new(),
        };

        let cursor = Cursor::new(Vec::new());
        let mut muxer = MxfMuxer::new(cursor, false, "BroadcastMaster", Some(&v_cfg), &[a_cfg]).unwrap();

        let fake_dnx_frame = vec![0u8; 4096];
        muxer.write_video_sample(VideoSample { data: &fake_dnx_frame, pts: 0, dts: 0, duration_ticks: 1, is_keyframe: true }).unwrap();

        let audio_buf = AudioSampleBuffer::new_silent(48_000, ChannelLayout::Stereo, 2000, 0).unwrap();
        muxer.write_audio_buffer(0, &audio_buf, 24).unwrap();

        let finished = muxer.finish().unwrap();
        let bytes = finished.into_inner();
        assert!(filmcraft_mxf::sniff(&bytes));

        let parsed = filmcraft_mxf::open(bytes.as_slice()).unwrap();
        assert_eq!(parsed.tracks.len(), 2);
    }
}
