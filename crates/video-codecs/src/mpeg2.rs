//! MPEG-2 video (ITU-T H.262) encoder wrapper backed by `craft-video`.

use craft_video::mpeg2::ChromaFormat;
use craft_video::mpeg2::encode::{AspectRatio, Encoder, EncoderConfig, RateControl, YuvFrame};
use encodercraft_core::{EncoderError, RateMode, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv420_8;

const H262_RATES: [(u32, u32); 8] = [(24000, 1001), (24, 1), (25, 1), (30000, 1001), (30, 1), (50, 1), (60000, 1001), (60, 1)];

fn nearest_mpeg2_fps(fps: Rational64) -> (u32, u32) {
    let target = fps.as_f64();
    H262_RATES
        .iter()
        .copied()
        .min_by(|&(n1, d1), &(n2, d2)| {
            let e1 = ((f64::from(n1) / f64::from(d1)) - target).abs();
            let e2 = ((f64::from(n2) / f64::from(d2)) - target).abs();
            e1.total_cmp(&e2)
        })
        .unwrap_or((30000, 1001))
}

/// MPEG-2 video encoder (Main Profile @ Low/Main/High Level).
pub struct Mpeg2EncoderWrapper {
    encoder: Encoder,
    fps_den: i64,
    next_pts: i64,
}

impl Mpeg2EncoderWrapper {
    pub fn new(width: u32, height: u32, fps: Rational64) -> Result<Self, EncoderError> {
        Self::with_params(width, height, fps, &EncodeParams::default())
    }

    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(EncoderError::InvalidJobSpec(format!("MPEG-2 requires even non-zero dimensions, got {width}x{height}")));
        }
        let (fps_num, fps_den) = nearest_mpeg2_fps(fps);
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        cfg.chroma = ChromaFormat::Yuv420;
        cfg.aspect = if width * 9 == height * 16 { AspectRatio::Dar16x9 } else { AspectRatio::Square };
        cfg.gop_size = params.keyframe_interval.unwrap_or(12).clamp(1, 1023);
        cfg.b_pictures = u32::from(params.bframes.unwrap_or(0).min(4));
        let kbps = params.bitrate_kbps.unwrap_or(6000).clamp(400, 80_000);
        cfg.rate = match params.rate_mode {
            RateMode::Cbr => RateControl::Cbr { kbps },
            RateMode::Vbr => RateControl::Vbr { kbps, max_kbps: kbps.saturating_mul(3).saturating_div(2).min(80_000) },
            RateMode::Auto => RateControl::Abr { kbps },
            RateMode::ConstantQuality => RateControl::Quantiser(4),
        };

        let encoder = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("MPEG-2 encoder init failed: {e}")))?;
        Ok(Self { encoder, fps_den: i64::from(fps_den), next_pts: 0 })
    }
}

impl VideoEncoder for Mpeg2EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let yuv_frame = YuvFrame { y: &yuv.y, cb: &yuv.u, cr: &yuv.v, y_stride: yuv.y_stride, c_stride: yuv.uv_stride };
        let pts = if self.next_pts == 0 { frame.pts } else { self.next_pts.max(frame.pts) };
        self.next_pts = pts.saturating_add(self.fps_den.max(1));

        let packets = self.encoder.encode(&yuv_frame, pts).map_err(|e| EncoderError::Internal(format!("MPEG-2 encode failed: {e}")))?;
        Ok(packets
            .into_iter()
            .map(|p| EncodedPacket { data: p.data, pts: p.pts, dts: p.dts, is_keyframe: p.keyframe, duration_ticks: self.fps_den.max(1) as u32 })
            .collect())
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(self
            .encoder
            .flush()
            .into_iter()
            .map(|p| EncodedPacket { data: p.data, pts: p.pts, dts: p.dts, is_keyframe: p.keyframe, duration_ticks: self.fps_den.max(1) as u32 })
            .collect())
    }

    fn config_record(&self) -> Vec<u8> {
        self.encoder.sequence_header().to_vec()
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        Some(*b"mp2v")
    }
}
