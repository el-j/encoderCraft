//! MPEG-4 Part 2 (ISO/IEC 14496-2 Simple Profile) encoder wrapper backed by `craft-video`.

use craft_video::mpeg4::{ColorConfig, Encoder, EncoderConfig, RateControl, YuvFrame};
use encodercraft_core::{ColorTag, EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv420_8;

fn gcd_u32(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd_u32(b, a % b) }
}

fn reduced_mpeg4_fps(fps: Rational64) -> (u32, u32) {
    let mut num = u32::try_from(fps.num.max(1)).unwrap_or(30000);
    let mut den = u32::try_from(fps.den.max(1)).unwrap_or(1001);
    let g = gcd_u32(num, den);
    num /= g.max(1);
    den /= g.max(1);
    while num > 0xFFFF {
        num = (num / 2).max(1);
        den = (den / 2).max(1);
    }
    (num, den)
}

/// MPEG-4 Part 2 (Xvid / DivX compatible Simple Profile) video encoder.
pub struct Mpeg4EncoderWrapper {
    encoder: Encoder,
    fps_den: i64,
}

impl Mpeg4EncoderWrapper {
    pub fn new(width: u32, height: u32, fps: Rational64) -> Result<Self, EncoderError> {
        Self::with_params(width, height, fps, &EncodeParams::default())
    }

    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        let (fps_num, fps_den) = reduced_mpeg4_fps(fps);
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        cfg.keyint = params.keyframe_interval.unwrap_or(250).max(1);
        cfg.headers_in_band = true;
        cfg.rate = match params.bitrate_kbps {
            Some(kbps) => RateControl::Bitrate { kbps: kbps.max(100) },
            None => RateControl::Quant(4),
        };
        cfg.color = match params.color {
            Some(ColorTag::Pq) => Some(ColorConfig { primaries: 9, transfer: 16, matrix: 9, full_range: false }),
            Some(ColorTag::Hlg) => Some(ColorConfig { primaries: 9, transfer: 18, matrix: 9, full_range: false }),
            Some(ColorTag::Bt709) => Some(ColorConfig { primaries: 1, transfer: 1, matrix: 1, full_range: false }),
            None => None,
        };

        let encoder = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("MPEG-4 Part 2 encoder init failed: {e}")))?;
        Ok(Self { encoder, fps_den: i64::from(fps_den) })
    }
}

impl VideoEncoder for Mpeg4EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let yuv_frame = YuvFrame { y: &yuv.y, u: &yuv.u, v: &yuv.v, y_stride: yuv.y_stride, uv_stride: yuv.uv_stride };
        let pkt = self.encoder.encode(&yuv_frame).map_err(|e| EncoderError::Internal(format!("MPEG-4 Part 2 encode failed: {e}")))?;
        let pts = pkt.pts.saturating_mul(self.fps_den.max(1));
        Ok(vec![EncodedPacket { data: pkt.data, pts, dts: pts, is_keyframe: pkt.keyframe, duration_ticks: self.fps_den.max(1) as u32 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }

    fn config_record(&self) -> Vec<u8> {
        self.encoder.extradata().to_vec()
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        Some(*b"mp4v")
    }
}
