//! HEVC (H.265) video encoder wrapper backed by `craft-hevcenc`.

use craft_hevcenc::{Encoder, EncoderConfig, Frame, RateControl};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::frame_to_yuv420_16;

/// HEVC (H.265) video encoder wrapper.
pub struct HevcEncoderWrapper {
    inner: Encoder,
}

impl HevcEncoderWrapper {
    /// Creates a new HEVC encoder instance.
    pub fn new(width: u32, height: u32, fps: Rational64, bitrate_kbps: Option<u32>) -> Result<Self, EncoderError> {
        let fps_num = if fps.num > 0 { fps.num as u32 } else { 30 };
        let fps_den = if fps.den > 0 { fps.den as u32 } else { 1 };
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        if let Some(kbps) = bitrate_kbps {
            cfg.rate = RateControl::Bitrate { kbps };
        } else {
            cfg.rate = RateControl::ConstantQp(28);
        }
        let inner = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("failed to initialize HEVC encoder: {e}")))?;
        Ok(Self { inner })
    }

    /// Parameter sets (VPS, SPS, PPS).
    pub fn parameter_sets(&self) -> [Vec<u8>; 3] {
        self.inner.parameter_sets()
    }

    /// `hvcC` decoder configuration record.
    pub fn hvcc(&self) -> Vec<u8> {
        self.inner.hvcc()
    }
}

impl VideoEncoder for HevcEncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_16(frame)?;
        let input = Frame { y: &yuv.y, u: &yuv.u, v: &yuv.v, y_stride: yuv.y_stride, uv_stride: yuv.uv_stride };
        let pkt = self.inner.encode(&input);

        Ok(vec![EncodedPacket { data: pkt.data, pts: frame.pts, dts: frame.pts, is_keyframe: pkt.keyframe, duration_ticks: 1000 }])
    }

    fn config_record(&self) -> Vec<u8> {
        self.inner.hvcc()
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }
}
