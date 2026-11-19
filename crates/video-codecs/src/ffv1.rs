//! FFV1 (RFC 9043 v3) lossless video encoder wrapper backed by `craft-video`.

use craft_video::ffv1::{Colorspace, Encoder, EncoderConfig, Frame};
use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, Rational64, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::rgba_f32_to_rgb8;

fn video_frame_to_rgb_planes16(frame: &VideoFrame) -> Result<(Vec<u16>, Vec<u16>, Vec<u16>), EncoderError> {
    let w = frame.dimensions.width as usize;
    let h = frame.dimensions.height as usize;
    let pixels = w.checked_mul(h).ok_or_else(|| EncoderError::Internal("FFV1 dimensions overflow".into()))?;
    let mut r_plane = Vec::with_capacity(pixels);
    let mut g_plane = Vec::with_capacity(pixels);
    let mut b_plane = Vec::with_capacity(pixels);

    match frame.format {
        PixelFormat::Rgb8 => {
            let (raw, stride) = frame.plane(0)?;
            for y in 0..h {
                let row = y * stride;
                for x in 0..w {
                    let i = row + x * 3;
                    r_plane.push(u16::from(raw.get(i).copied().unwrap_or(0)));
                    g_plane.push(u16::from(raw.get(i + 1).copied().unwrap_or(0)));
                    b_plane.push(u16::from(raw.get(i + 2).copied().unwrap_or(0)));
                }
            }
        }
        PixelFormat::Rgba8 => {
            let (raw, stride) = frame.plane(0)?;
            for y in 0..h {
                let row = y * stride;
                for x in 0..w {
                    let i = row + x * 4;
                    r_plane.push(u16::from(raw.get(i).copied().unwrap_or(0)));
                    g_plane.push(u16::from(raw.get(i + 1).copied().unwrap_or(0)));
                    b_plane.push(u16::from(raw.get(i + 2).copied().unwrap_or(0)));
                }
            }
        }
        PixelFormat::Bgra8 => {
            let (raw, stride) = frame.plane(0)?;
            for y in 0..h {
                let row = y * stride;
                for x in 0..w {
                    let i = row + x * 4;
                    r_plane.push(u16::from(raw.get(i + 2).copied().unwrap_or(0)));
                    g_plane.push(u16::from(raw.get(i + 1).copied().unwrap_or(0)));
                    b_plane.push(u16::from(raw.get(i).copied().unwrap_or(0)));
                }
            }
        }
        PixelFormat::RgbaF32 => {
            let rgb = rgba_f32_to_rgb8(frame)?;
            for px in rgb.as_chunks::<3>().0 {
                r_plane.push(u16::from(px[0]));
                g_plane.push(u16::from(px[1]));
                b_plane.push(u16::from(px[2]));
            }
        }
        other => return Err(EncoderError::UnsupportedCodec(format!("FFV1 encoder does not support {other:?} input"))),
    }

    Ok((r_plane, g_plane, b_plane))
}

/// FFV1 v3 lossless video encoder (RFC 9043).
pub struct Ffv1EncoderWrapper {
    encoder: Encoder,
    width: u32,
    height: u32,
    fps: Rational64,
    frame_index: i64,
}

impl Ffv1EncoderWrapper {
    pub fn new(width: u32, height: u32, fps: Rational64) -> Result<Self, EncoderError> {
        let len = (width as usize).checked_mul(height as usize).ok_or_else(|| EncoderError::Internal("FFV1 dimensions overflow".into()))?;
        let template = Frame { width, height, colorspace: Colorspace::Rgb, bit_depth: 8, chroma_shift: (0, 0), planes: vec![vec![0; len]; 3] };
        let encoder =
            Encoder::new(&template, EncoderConfig::default()).map_err(|e| EncoderError::Internal(format!("FFV1 encoder init failed: {e}")))?;
        Ok(Self { encoder, width, height, fps, frame_index: 0 })
    }
}

impl VideoEncoder for Ffv1EncoderWrapper {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let (r, g, b) = video_frame_to_rgb_planes16(frame)?;
        let ffv1_frame =
            Frame { width: self.width, height: self.height, colorspace: Colorspace::Rgb, bit_depth: 8, chroma_shift: (0, 0), planes: vec![r, g, b] };
        let data = self.encoder.encode(&ffv1_frame).map_err(|e| EncoderError::Internal(format!("FFV1 encode failed: {e}")))?;
        let pts = if frame.pts != 0 || self.frame_index == 0 { frame.pts } else { self.frame_index * self.fps.den.max(1) };
        self.frame_index += 1;

        Ok(vec![EncodedPacket { data, pts, dts: pts, is_keyframe: true, duration_ticks: self.fps.den.max(1) as u32 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }

    fn config_record(&self) -> Vec<u8> {
        self.encoder.config_record()
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        Some(*b"ffv1")
    }
}
