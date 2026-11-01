//! Apple ProRes video encoder wrapper backed by `filmcraft-prores`.

use encodercraft_core::{EncoderError, Rational64, VideoFrame};
use filmcraft_prores::{ChromaFormat, ColorInfo, Encoder, EncoderConfig, Frame, Interlace, Profile};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::frame_to_yuv420_8;

/// Apple ProRes encoder wrapper.
pub struct ProResEncoderWrapper {
    inner: Encoder,
    width: u32,
    height: u32,
}

impl ProResEncoderWrapper {
    /// Creates a new ProRes encoder instance (Standard 422 default).
    pub fn new(width: u32, height: u32, _fps: Rational64) -> Result<Self, EncoderError> {
        let cfg = EncoderConfig::new(Profile::Standard, width, height);
        let inner = Encoder::with_config(cfg);
        Ok(Self { inner, width, height })
    }
}

impl VideoEncoder for ProResEncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let yuv = frame_to_yuv420_8(frame)?;
        let w = self.width as usize;
        let h = self.height as usize;
        let uv_w = (self.width.div_ceil(2)) as usize;

        let mut y_plane = Vec::with_capacity(w.checked_mul(h).unwrap_or(0));
        let mut cb_plane = Vec::with_capacity(uv_w.checked_mul(h).unwrap_or(0));
        let mut cr_plane = Vec::with_capacity(uv_w.checked_mul(h).unwrap_or(0));

        // Expand 8-bit YUV420 to 10-bit YUV422
        for py in 0..h {
            let row_start = py.checked_mul(yuv.y_stride).unwrap_or(0);
            for px in 0..w {
                let sample = yuv.y.get(row_start + px).copied().unwrap_or(16) as u16;
                y_plane.push(sample << 2);
            }

            let chroma_y = py / 2;
            let uv_row_start = chroma_y.checked_mul(yuv.uv_stride).unwrap_or(0);
            for cx in 0..uv_w {
                let u_sample = yuv.u.get(uv_row_start + cx).copied().unwrap_or(128) as u16;
                let v_sample = yuv.v.get(uv_row_start + cx).copied().unwrap_or(128) as u16;
                cb_plane.push(u_sample << 2);
                cr_plane.push(v_sample << 2);
            }
        }

        let prores_frame = Frame {
            width: self.width,
            height: self.height,
            chroma: ChromaFormat::Yuv422,
            bit_depth: 10,
            y: y_plane,
            cb: cb_plane,
            cr: cr_plane,
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
}
