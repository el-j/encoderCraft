//! AV1 video encoder wrapper backed by `craft-av1enc`.

use craft_av1enc::{Encoder, EncoderConfig, Frame, RateControl};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv420_16;
use encodercraft_core::RateMode;

/// AV1 video encoder wrapper.
pub struct Av1EncoderWrapper {
    inner: Encoder,
}

impl Av1EncoderWrapper {
    /// Creates a new AV1 encoder instance.
    pub fn new(width: u32, height: u32, fps: Rational64, bitrate_kbps: Option<u32>) -> Result<Self, EncoderError> {
        Self::with_params(width, height, fps, &EncodeParams { bitrate_kbps, ..EncodeParams::default() })
    }

    /// Creates an AV1 encoder honouring rate mode and GOP length; B-frames, HDR tags and profiles are rejected.
    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        params.reject_unsupported("AV1", false, false, false)?;
        let fps_num = if fps.num > 0 { fps.num as u32 } else { 30 };
        let fps_den = if fps.den > 0 { fps.den as u32 } else { 1 };
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        cfg.rate = match (params.rate_mode, params.bitrate_kbps) {
            (RateMode::ConstantQuality, _) | (_, None) => RateControl::ConstantQ(100),
            (_, Some(kbps)) => RateControl::Bitrate { kbps },
        };
        if let Some(interval) = params.keyframe_interval {
            cfg.keyint = interval.max(1);
        }
        let inner = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("failed to initialize AV1 encoder: {e}")))?;
        Ok(Self { inner })
    }

    /// Sequence header OBU.
    pub fn sequence_header_obu(&self) -> Vec<u8> {
        self.inner.sequence_header_obu()
    }

    /// `av1C` decoder configuration record for MP4/WebM containers.
    pub fn av1c(&self) -> Vec<u8> {
        self.inner.av1c()
    }
}

impl VideoEncoder for Av1EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_16(frame)?;
        let input = Frame { y: &yuv.y, u: &yuv.u, v: &yuv.v, y_stride: yuv.y_stride, uv_stride: yuv.uv_stride };
        let pkt = self.inner.encode(&input);

        Ok(vec![EncodedPacket { data: pkt.data, pts: frame.pts, dts: frame.pts, is_keyframe: pkt.keyframe, duration_ticks: 1000 }])
    }

    fn config_record(&self) -> Vec<u8> {
        self.inner.av1c()
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }
}
