//! H.264 / AVC video encoder wrapper backed by `filmcraft-h264enc`.

use encodercraft_core::{EncoderError, Rational64, VideoFrame};
use filmcraft_h264enc::{Encoder, EncoderConfig, PacketFormat, RateControl, YuvFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::frame_to_yuv420_8;

/// Clean-room H.264/AVC encoder wrapper.
pub struct H264EncoderWrapper {
    inner: Encoder,
}

impl H264EncoderWrapper {
    /// Creates a new H.264 encoder instance.
    pub fn new(width: u32, height: u32, fps: Rational64, bitrate_kbps: Option<u32>) -> Result<Self, EncoderError> {
        let fps_num = if fps.num > 0 { fps.num as u32 } else { 30 };
        let fps_den = if fps.den > 0 { fps.den as u32 } else { 1 };
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        cfg.format = PacketFormat::LengthPrefixed;
        if let Some(kbps) = bitrate_kbps {
            cfg.rate = RateControl::Cbr { kbps };
        } else {
            cfg.rate = RateControl::Crf(23.0);
        }
        let inner = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("failed to initialize H.264 encoder: {e}")))?;
        Ok(Self { inner })
    }
}

impl VideoEncoder for H264EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let input = YuvFrame { y: &yuv.y, u: &yuv.u, v: &yuv.v, y_stride: yuv.y_stride, uv_stride: yuv.uv_stride };
        let packets = self.inner.encode(&input, frame.pts).map_err(|e| EncoderError::Internal(format!("H.264 encode error: {e}")))?;

        Ok(packets
            .into_iter()
            .map(|pkt| EncodedPacket { data: pkt.data, pts: pkt.pts, dts: pkt.dts, is_keyframe: pkt.keyframe, duration_ticks: 1000 })
            .collect())
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        let packets = self.inner.flush();
        Ok(packets
            .into_iter()
            .map(|pkt| EncodedPacket { data: pkt.data, pts: pkt.pts, dts: pkt.dts, is_keyframe: pkt.keyframe, duration_ticks: 1000 })
            .collect())
    }
}
