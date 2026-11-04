//! H.264 / AVC video encoder wrapper backed by `craft-h264enc`.

use craft_h264enc::{ColorConfig, Encoder, EncoderConfig, PacketFormat, Profile, RateControl, YuvFrame};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv420_8;
use encodercraft_core::RateMode;

/// Clean-room H.264/AVC encoder wrapper.
pub struct H264EncoderWrapper {
    inner: Encoder,
}

impl H264EncoderWrapper {
    /// Creates a new H.264 encoder instance.
    pub fn new(width: u32, height: u32, fps: Rational64, bitrate_kbps: Option<u32>) -> Result<Self, EncoderError> {
        Self::with_params(width, height, fps, &EncodeParams { bitrate_kbps, ..EncodeParams::default() })
    }

    /// Creates an H.264 encoder honouring rate mode, GOP, B-frames, profile and colour tags.
    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        let fps_num = if fps.num > 0 { fps.num as u32 } else { 30 };
        let fps_den = if fps.den > 0 { fps.den as u32 } else { 1 };
        let mut cfg = EncoderConfig::new(width, height, fps_num, fps_den);
        cfg.format = if params.annex_b { PacketFormat::AnnexB } else { PacketFormat::LengthPrefixed };
        cfg.rate = match (params.rate_mode, params.bitrate_kbps) {
            (RateMode::ConstantQuality, _) | (_, None) => RateControl::Crf(23.0),
            (RateMode::Vbr, Some(kbps)) => RateControl::Vbr { target_kbps: kbps, max_kbps: kbps.saturating_mul(3) / 2 },
            (_, Some(kbps)) => RateControl::Cbr { kbps },
        };
        if let Some(interval) = params.keyframe_interval {
            cfg.keyint = interval.max(1);
        }
        cfg.profile = match params.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("high") => Profile::High,
            Some("main") => Profile::Main,
            Some("baseline") => Profile::Baseline,
            Some(other) => return Err(EncoderError::UnsupportedCodec(format!("unknown H.264 profile `{other}`"))),
        };
        if let Some(b) = params.bframes {
            if b > 3 {
                return Err(EncoderError::InvalidJobSpec(format!("H.264 supports at most 3 B-frames, requested {b}")));
            }
            cfg.bframes = if cfg.profile == Profile::Baseline { 0 } else { b };
        }
        if let Some(tag) = params.color {
            let (primaries, transfer, matrix) = tag.code_points();
            cfg.color = ColorConfig { primaries, transfer, matrix, full_range: false };
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

    fn config_record(&self) -> Vec<u8> {
        self.inner.avcc()
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        let packets = self.inner.flush();
        Ok(packets
            .into_iter()
            .map(|pkt| EncodedPacket { data: pkt.data, pts: pkt.pts, dts: pkt.dts, is_keyframe: pkt.keyframe, duration_ticks: 1000 })
            .collect())
    }
}
