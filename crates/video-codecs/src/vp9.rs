//! VP9 video encoder wrapper backed by `craft-vp9enc`.

use craft_vp9enc::{EncoderConfig, Vp9Encoder};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::frame_to_yuv420_8;

/// VP9 video encoder wrapper.
pub struct Vp9EncoderWrapper {
    inner: Vp9Encoder,
}

impl Vp9EncoderWrapper {
    /// Creates a new VP9 encoder instance.
    pub fn new(width: u32, height: u32, fps: Rational64, bitrate_kbps: Option<u32>) -> Result<Self, EncoderError> {
        let fps_f64 = if fps.num > 0 && fps.den > 0 { fps.num as f64 / fps.den as f64 } else { 30.0 };
        let cfg = EncoderConfig {
            width,
            height,
            quality: 80,
            full_range: false,
            keyframe_interval: 60,
            target_kbps: bitrate_kbps,
            frame_rate: fps_f64,
            loop_filter: true,
        };
        let inner = Vp9Encoder::new(cfg);
        Ok(Self { inner })
    }
}

impl VideoEncoder for Vp9EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let encoded = self.inner.encode_frame(&yuv.y, &yuv.u, &yuv.v, false);

        Ok(vec![EncodedPacket { data: encoded.data, pts: frame.pts, dts: frame.pts, is_keyframe: encoded.key, duration_ticks: 1000 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }
}
