//! Avid DNxHR video encoder wrapper backed by `craft-dnx`.

use craft_dnx::{ChromaFormat, ColorVolume, Encoder, EncoderConfig, Frame, Interlace, Profile};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::params::EncodeParams;
use crate::yuv::frame_to_yuv420_8;

/// Avid DNxHR video encoder wrapper.
pub struct DnxEncoderWrapper {
    inner: Encoder,
    width: u32,
    height: u32,
    profile: Profile,
    bit_depth: u8,
}

impl DnxEncoderWrapper {
    /// Creates a new Avid DNxHR encoder instance (defaults to DNxHR HQ 8-bit).
    pub fn new(width: u32, height: u32, _fps: Rational64) -> Result<Self, EncoderError> {
        Self::with_profile(width, height, _fps, Profile::Hq)
    }

    /// Creates a DNxHR encoder from parameters; the profile selects LB, SQ, HQ or HQX (10-bit).
    pub fn with_params(width: u32, height: u32, fps: Rational64, params: &EncodeParams) -> Result<Self, EncoderError> {
        params.reject_unsupported("DNxHR", false, false, true)?;
        let profile = match params.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("hq") => Profile::Hq,
            Some("lb") => Profile::Lb,
            Some("sq") => Profile::Sq,
            Some("hqx") => Profile::Hqx,
            Some(other) => return Err(EncoderError::UnsupportedCodec(format!("DNxHR profile `{other}` is not available (LB, SQ, HQ, HQX)"))),
        };
        Self::with_profile(width, height, fps, profile)
    }

    /// Creates a new Avid DNxHR encoder instance with a specific profile (e.g. HQ or HQX).
    pub fn with_profile(width: u32, height: u32, _fps: Rational64, profile: Profile) -> Result<Self, EncoderError> {
        let cfg = EncoderConfig::new(profile, width, height);
        let bit_depth = cfg.bit_depth;
        let inner = Encoder::with_config(cfg).map_err(|e| EncoderError::Internal(format!("Avid DNxHR encoder init error: {e}")))?;

        Ok(Self { inner, width, height, profile, bit_depth })
    }
}

impl VideoEncoder for DnxEncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let w = self.width as usize;
        let h = self.height as usize;
        let uv_w = (self.width.div_ceil(2)) as usize;

        let mut y_plane = Vec::with_capacity(w.checked_mul(h).unwrap_or(0));
        let mut cb_plane = Vec::with_capacity(uv_w.checked_mul(h).unwrap_or(0));
        let mut cr_plane = Vec::with_capacity(uv_w.checked_mul(h).unwrap_or(0));

        let shift = if self.bit_depth > 8 { (self.bit_depth - 8) as u32 } else { 0 };

        // Convert 8-bit 4:2:0 YUV to 4:2:2 planar YUV
        for py in 0..h {
            let row_start = py.checked_mul(yuv.y_stride).unwrap_or(0);
            for px in 0..w {
                let sample = (yuv.y.get(row_start + px).copied().unwrap_or(16) as u16) << shift;
                y_plane.push(sample);
            }

            let chroma_y = py / 2;
            let uv_row_start = chroma_y.checked_mul(yuv.uv_stride).unwrap_or(0);
            for cx in 0..uv_w {
                let u_sample = (yuv.u.get(uv_row_start + cx).copied().unwrap_or(128) as u16) << shift;
                let v_sample = (yuv.v.get(uv_row_start + cx).copied().unwrap_or(128) as u16) << shift;
                cb_plane.push(u_sample);
                cr_plane.push(v_sample);
            }
        }

        let dnx_frame = Frame {
            width: self.width,
            height: self.height,
            chroma: ChromaFormat::Yuv422,
            rgb: false,
            bit_depth: self.bit_depth,
            y: y_plane,
            cb: cb_plane,
            cr: cr_plane,
            alpha: None,
            interlace: Interlace::Progressive,
            color_volume: ColorVolume::Bt709,
            cid: self.profile.cid(),
            par: (0, 0),
        };

        let encoded_bytes = self.inner.encode(&dnx_frame).map_err(|e| EncoderError::Internal(format!("DNxHR encode error: {e}")))?;

        Ok(vec![EncodedPacket { data: encoded_bytes, pts: frame.pts, dts: frame.pts, is_keyframe: true, duration_ticks: 1000 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }
}
