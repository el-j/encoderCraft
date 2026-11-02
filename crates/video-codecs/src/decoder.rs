//! Unified pure-Rust video packet decoders (Apple ProRes, Avid DNxHD/DNxHR, and VP8).
//!
//! Converts decoded planar YUV frames (8/10/12-bit 4:2:0, 4:2:2, and 4:4:4) into
//! standard `encodercraft_core::VideoFrame` (`PixelFormat::Rgb8`) ready for scaling,
//! LUT grading, overlay burn-in, and re-encoding.

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{EncoderError, Rational64, VideoFrame};
use encodercraft_geom::Dimensions;

/// Decodes a single Apple ProRes packet (`icpf` frame) into an `Rgb8` [`VideoFrame`].
pub fn decode_prores_packet(data: &[u8], pts: i64, fps: Rational64) -> Result<VideoFrame, EncoderError> {
    let decoded = craft_prores::decode_frame(data).map_err(|e| EncoderError::Internal(format!("ProRes decode error: {e}")))?;
    let dims = Dimensions::new(decoded.width, decoded.height)?;
    let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::REC709, pts, 1, fps)?;

    let w = decoded.width as usize;
    let h = decoded.height as usize;
    let cw = decoded.chroma_width() as usize;
    let shift = decoded.bit_depth.saturating_sub(8);
    let is_444 = decoded.chroma == craft_prores::ChromaFormat::Yuv444;

    let (rgb_plane, stride) = out.plane_mut(0)?;
    for py in 0..h {
        let y_row = py.saturating_mul(w);
        let c_row = py.saturating_mul(cw);
        let out_row = py.saturating_mul(stride);
        for px in 0..w {
            let cx = if is_444 { px } else { px / 2 };
            let y_val = (decoded.y.get(y_row + px).copied().unwrap_or(64) >> shift) as i32;
            let cb_val = (decoded.cb.get(c_row + cx).copied().unwrap_or(512) >> shift) as i32;
            let cr_val = (decoded.cr.get(c_row + cx).copied().unwrap_or(512) >> shift) as i32;

            let (r, g, b) = bt709_yuv8_to_rgb8(y_val, cb_val, cr_val);
            let idx = out_row.saturating_add(px.saturating_mul(3));
            if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                dst[0] = r;
                dst[1] = g;
                dst[2] = b;
            }
        }
    }

    Ok(out)
}

/// Decodes a single Avid DNxHD / DNxHR (SMPTE VC-3) packet into an `Rgb8` [`VideoFrame`].
pub fn decode_dnx_packet(data: &[u8], pts: i64, fps: Rational64) -> Result<VideoFrame, EncoderError> {
    let decoded = craft_dnx::decode_frame(data).map_err(|e| EncoderError::Internal(format!("DNxHR/VC-3 decode error: {e}")))?;
    let dims = Dimensions::new(decoded.width, decoded.height)?;
    let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::REC709, pts, 1, fps)?;

    let w = decoded.width as usize;
    let h = decoded.height as usize;
    let cw = decoded.chroma_width() as usize;
    let ch = decoded.chroma_height() as usize;
    let shift = decoded.bit_depth.saturating_sub(8);

    let (rgb_plane, stride) = out.plane_mut(0)?;
    for py in 0..h {
        let y_row = py.saturating_mul(w);
        let cy = if decoded.chroma == craft_dnx::ChromaFormat::Yuv420 { (py / 2).min(ch.saturating_sub(1)) } else { py };
        let c_row = cy.saturating_mul(cw);
        let out_row = py.saturating_mul(stride);
        for px in 0..w {
            if decoded.rgb {
                let g = (decoded.y.get(y_row + px).copied().unwrap_or(0) >> shift).min(255) as u8;
                let b = (decoded.cb.get(y_row + px).copied().unwrap_or(0) >> shift).min(255) as u8;
                let r = (decoded.cr.get(y_row + px).copied().unwrap_or(0) >> shift).min(255) as u8;
                let idx = out_row.saturating_add(px.saturating_mul(3));
                if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                    dst[0] = r;
                    dst[1] = g;
                    dst[2] = b;
                }
            } else {
                let cx = if decoded.chroma == craft_dnx::ChromaFormat::Yuv444 { px } else { (px / 2).min(cw.saturating_sub(1)) };
                let y_val = (decoded.y.get(y_row + px).copied().unwrap_or(16) >> shift) as i32;
                let cb_val = (decoded.cb.get(c_row + cx).copied().unwrap_or(128) >> shift) as i32;
                let cr_val = (decoded.cr.get(c_row + cx).copied().unwrap_or(128) >> shift) as i32;

                let (r, g, b) = bt709_yuv8_to_rgb8(y_val, cb_val, cr_val);
                let idx = out_row.saturating_add(px.saturating_mul(3));
                if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                    dst[0] = r;
                    dst[1] = g;
                    dst[2] = b;
                }
            }
        }
    }

    Ok(out)
}

/// Stateful VP8 video decoder wrapper backed by `craft-vp8`.
#[derive(Default)]
pub struct Vp8DecoderWrapper {
    inner: craft_vp8::Decoder,
}

impl Vp8DecoderWrapper {
    /// Creates a new VP8 stream decoder.
    pub fn new() -> Self {
        Self { inner: craft_vp8::Decoder::new() }
    }

    /// Decodes a VP8 packet, returning `Some(VideoFrame)` for visible frames or `None` for altref hidden frames.
    pub fn decode_packet(&mut self, data: &[u8], pts: i64, fps: Rational64) -> Result<Option<VideoFrame>, EncoderError> {
        let Some(pic) = self.inner.decode(data, pts).map_err(|e| EncoderError::Internal(format!("VP8 decode error: {e}")))? else {
            return Ok(None);
        };

        let dims = Dimensions::new(pic.width, pic.height)?;
        let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::REC709, pic.pts, 1, fps)?;

        let w = pic.width as usize;
        let h = pic.height as usize;
        let cw = pic.chroma_width as usize;
        let ch = pic.chroma_height as usize;

        let (rgb_plane, stride) = out.plane_mut(0)?;
        for py in 0..h {
            let y_row = py.saturating_mul(w);
            let cy = (py / 2).min(ch.saturating_sub(1));
            let c_row = cy.saturating_mul(cw);
            let out_row = py.saturating_mul(stride);
            for px in 0..w {
                let cx = (px / 2).min(cw.saturating_sub(1));
                let y_val = i32::from(pic.y.get(y_row + px).copied().unwrap_or(16));
                let cb_val = i32::from(pic.u.get(c_row + cx).copied().unwrap_or(128));
                let cr_val = i32::from(pic.v.get(c_row + cx).copied().unwrap_or(128));

                let (r, g, b) = bt709_yuv8_to_rgb8(y_val, cb_val, cr_val);
                let idx = out_row.saturating_add(px.saturating_mul(3));
                if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                    dst[0] = r;
                    dst[1] = g;
                    dst[2] = b;
                }
            }
        }

        Ok(Some(out))
    }
}

/// Inspects a compressed video packet's bitstream signature and decodes it if supported
/// (Apple ProRes `icpf`, Avid DNxHD/DNxHR VC-3, or VP8).
pub fn decode_video_packet_auto(
    data: &[u8],
    pts: i64,
    fps: Rational64,
    vp8_decoder: &mut Option<Vp8DecoderWrapper>,
) -> Result<Option<VideoFrame>, EncoderError> {
    if data.len() >= 8 && &data[4..8] == b"icpf" {
        return decode_prores_packet(data, pts, fps).map(Some);
    }
    if data.len() >= 5 && data[..4] == [0x00, 0x00, 0x02, 0x80] {
        return decode_dnx_packet(data, pts, fps).map(Some);
    }
    if vp8_decoder.is_some() || craft_vp8::is_keyframe(data) {
        let dec = vp8_decoder.get_or_insert_with(Vp8DecoderWrapper::new);
        return dec.decode_packet(data, pts, fps);
    }
    Ok(None)
}

#[inline]
fn bt709_yuv8_to_rgb8(y: i32, cb: i32, cr: i32) -> (u8, u8, u8) {
    let u = cb - 128;
    let v = cr - 128;
    let r = (y + ((103_206 * v + 32_768) >> 16)).clamp(0, 255) as u8;
    let g = (y - ((12_276 * u + 30_679 * v + 32_768) >> 16)).clamp(0, 255) as u8;
    let b = (y + ((121_609 * u + 32_768) >> 16)).clamp(0, 255) as u8;
    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnx::DnxEncoderWrapper;
    use crate::encoder::VideoEncoder;
    use crate::prores::ProResEncoderWrapper;

    #[test]
    fn test_prores_encode_decode_roundtrip() {
        let dims = Dimensions::new(320, 180).unwrap();
        let fps = Rational64::FPS_24;
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, fps).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, b) in plane.iter_mut().enumerate() {
            *b = ((i * 7) % 200 + 20) as u8;
        }

        let mut enc = ProResEncoderWrapper::new(320, 180, fps).unwrap();
        let pkts = enc.encode_frame(&frame).unwrap();
        assert_eq!(pkts.len(), 1);

        let decoded = decode_prores_packet(&pkts[0].data, 0, fps).unwrap();
        assert_eq!(decoded.dimensions, dims);
        assert_eq!(decoded.format, PixelFormat::Rgb8);
    }

    #[test]
    fn test_dnxhr_encode_decode_roundtrip() {
        let dims = Dimensions::new(320, 180).unwrap();
        let fps = Rational64::FPS_24;
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, fps).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, b) in plane.iter_mut().enumerate() {
            *b = ((i * 11) % 200 + 20) as u8;
        }

        let mut enc = DnxEncoderWrapper::new(320, 180, fps).unwrap();
        let pkts = enc.encode_frame(&frame).unwrap();
        assert_eq!(pkts.len(), 1);

        let decoded = decode_dnx_packet(&pkts[0].data, 0, fps).unwrap();
        assert_eq!(decoded.dimensions, dims);
        assert_eq!(decoded.format, PixelFormat::Rgb8);
    }
}
