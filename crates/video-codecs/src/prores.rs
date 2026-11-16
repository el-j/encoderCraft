//! Apple ProRes video encoder wrapper backed by `craft-prores`.

use craft_prores::{ChromaFormat, ColorInfo, Encoder, EncoderConfig, Frame, Interlace, Profile};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv422_16;

/// Apple ProRes encoder wrapper.
pub struct ProResEncoderWrapper {
    inner: Encoder,
    profile: Profile,
}

impl ProResEncoderWrapper {
    /// Creates a new ProRes encoder instance (Standard 422 default).
    pub fn new(width: u32, height: u32, _fps: Rational64) -> Result<Self, EncoderError> {
        Self::with_profile(width, height, Profile::Standard)
    }

    /// Creates a ProRes encoder for a 4:2:2 profile.
    pub fn with_profile(width: u32, height: u32, profile: Profile) -> Result<Self, EncoderError> {
        let cfg = EncoderConfig::new(profile, width, height);
        let inner = Encoder::with_config(cfg);
        Ok(Self { inner, profile })
    }

    /// Creates a ProRes encoder from parameters; the profile selects proxy, LT, 422 or HQ.
    pub fn with_params(width: u32, height: u32, params: &EncodeParams) -> Result<Self, EncoderError> {
        params.reject_unsupported("ProRes", false, false, true)?;
        let profile = match params.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("422") | Some("standard") => Profile::Standard,
            Some("proxy") => Profile::Proxy,
            Some("lt") => Profile::Lt,
            Some("hq") | Some("422hq") => Profile::Hq,
            Some(other) => return Err(EncoderError::UnsupportedCodec(format!("ProRes profile `{other}` is not available (4:2:2 profiles only)"))),
        };
        Self::with_profile(width, height, profile)
    }
}

impl VideoEncoder for ProResEncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv422_16(frame)?;

        let prores_frame = Frame {
            width: yuv.width,
            height: yuv.height,
            chroma: ChromaFormat::Yuv422,
            bit_depth: 10,
            y: yuv.y,
            cb: yuv.u,
            cr: yuv.v,
            alpha: None,
            interlace: Interlace::Progressive,
            color: ColorInfo::default(),
            aspect_ratio: 0,
            frame_rate_code: 0,
        };

        let encoded_bytes = self.inner.encode(&prores_frame).map_err(|e| EncoderError::Internal(format!("ProRes encode error: {e}")))?;

        Ok(vec![EncodedPacket { data: encoded_bytes, pts: frame.pts, dts: frame.pts, is_keyframe: true, duration_ticks: 1000 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        Some(self.profile.fourcc())
    }
}
