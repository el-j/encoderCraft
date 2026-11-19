//! APV (Advanced Professional Video, RFC 9924) encoder wrapper backed by `craft-video`.

use craft_video::apv::{ChromaFormat, ColorInfo, Encoder, EncoderConfig, Frame, Profile};
use encodercraft_core::{ColorTag, EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv422_16;

/// APV (RFC 9924) intra-frame video encoder.
pub struct ApvEncoderWrapper {
    encoder: Encoder,
    fps: Rational64,
    profile: Profile,
    frame_index: i64,
}

impl ApvEncoderWrapper {
    pub fn new(width: u32, height: u32, fps: Rational64) -> Result<Self, EncoderError> {
        Self::with_params(width, height, fps, &EncodeParams::default())
    }

    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        let profile = match params.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("422-12") => Profile::P422_12,
            Some("444-10") => Profile::P444_10,
            Some("444-12") => Profile::P444_12,
            Some("422-10") | None => Profile::P422_10,
            Some(other) => return Err(EncoderError::UnsupportedCodec(format!("unsupported APV profile `{other}`"))),
        };
        if !width.is_multiple_of(2) || width == 0 || height == 0 {
            return Err(EncoderError::InvalidJobSpec(format!("APV requires even non-zero width and non-zero height, got {width}x{height}")));
        }

        let mut cfg = EncoderConfig::new(profile, width, height);
        cfg.chroma = ChromaFormat::Yuv422;
        cfg.bit_depth = if matches!(profile, Profile::P422_12 | Profile::P444_12) { 12 } else { 10 };
        cfg.color = match params.color {
            Some(ColorTag::Pq) => ColorInfo::BT2020_PQ,
            Some(ColorTag::Hlg) => ColorInfo::BT2020_HLG,
            _ => ColorInfo::BT709,
        };
        if let Some(kbps) = params.bitrate_kbps {
            let num = fps.num.max(1) as u64;
            let den = fps.den.max(1) as u64;
            let bytes = (u64::from(kbps) * 1000 / 8).saturating_mul(den) / num;
            cfg.target_frame_bytes = bytes.clamp(512, 64_000_000) as u32;
        }

        let encoder = Encoder::with_config(cfg).map_err(|e| EncoderError::Internal(format!("APV encoder init failed: {e}")))?;
        Ok(Self { encoder, fps, profile, frame_index: 0 })
    }
}

impl VideoEncoder for ApvEncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv422_16(frame)?;
        let bit_depth = self.encoder.config().bit_depth;
        let shift = bit_depth.saturating_sub(10);
        let shift_plane = |src: &[u16]| -> Vec<u16> { if shift == 0 { src.to_vec() } else { src.iter().map(|&v| v << shift).collect() } };
        let mut apv_frame = Frame::new(yuv.width, yuv.height, ChromaFormat::Yuv422, bit_depth, false);
        apv_frame.profile_idc = self.profile.idc();
        apv_frame.y = shift_plane(&yuv.y);
        apv_frame.cb = shift_plane(&yuv.u);
        apv_frame.cr = shift_plane(&yuv.v);
        apv_frame.color = self.encoder.config().color;

        let data = self.encoder.encode(&apv_frame).map_err(|e| EncoderError::Internal(format!("APV encode failed: {e}")))?;
        let pts = if frame.pts != 0 || self.frame_index == 0 { frame.pts } else { self.frame_index * self.fps.den.max(1) };
        self.frame_index += 1;

        Ok(vec![EncodedPacket { data, pts, dts: pts, is_keyframe: true, duration_ticks: self.fps.den.max(1) as u32 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }

    fn config_record(&self) -> Vec<u8> {
        self.encoder.decoder_config_record()
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        Some(*b"apv1")
    }
}
