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
    let decoded = craft_video::prores::decode_frame(data).map_err(|e| EncoderError::Internal(format!("ProRes decode error: {e}")))?;
    let dims = Dimensions::new(decoded.width, decoded.height)?;
    let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::REC709, pts, 1, fps)?;

    let w = decoded.width as usize;
    let h = decoded.height as usize;
    let cw = decoded.chroma_width() as usize;
    let shift = decoded.bit_depth.saturating_sub(8);
    let is_444 = decoded.chroma == craft_video::prores::ChromaFormat::Yuv444;

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
    let decoded = craft_video::dnx::decode_frame(data).map_err(|e| EncoderError::Internal(format!("DNxHR/VC-3 decode error: {e}")))?;
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
        let cy = if decoded.chroma == craft_video::dnx::ChromaFormat::Yuv420 { (py / 2).min(ch.saturating_sub(1)) } else { py };
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
                let cx = if decoded.chroma == craft_video::dnx::ChromaFormat::Yuv444 { px } else { (px / 2).min(cw.saturating_sub(1)) };
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

/// Stateful VP8 video decoder wrapper backed by `craft-video`.
#[derive(Default)]
pub struct Vp8DecoderWrapper {
    inner: craft_video::vp8::Decoder,
}

impl Vp8DecoderWrapper {
    /// Creates a new VP8 stream decoder.
    pub fn new() -> Self {
        Self { inner: craft_video::vp8::Decoder::new() }
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

/// Decodes a single compressed RFC 9924 APV frame (`b"aPv1"...`) into an `Rgb8` `VideoFrame`.
pub fn decode_apv_packet(data: &[u8], pts: i64, fps: Rational64) -> Result<VideoFrame, EncoderError> {
    let decoded = craft_video::apv::decode_frame(data).map_err(|e| EncoderError::Internal(format!("APV decode error: {e}")))?;
    let dims = Dimensions::new(decoded.width, decoded.height)?;
    let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, pts, 1, fps)?;

    let w = decoded.width as usize;
    let h = decoded.height as usize;
    let cw = decoded.chroma_width() as usize;
    let ch = decoded.chroma_height() as usize;
    let shift = decoded.bit_depth.saturating_sub(8);
    let to_u8 = |val: u16| -> i32 { i32::from((val >> shift).min(255)) };

    let (_, stride) = out.plane(0)?;
    let (rgb_plane, _) = out.plane_mut(0)?;
    for py in 0..h {
        let y_row = py.saturating_mul(w);
        let cy = py.min(ch.saturating_sub(1));
        let c_row = cy.saturating_mul(cw);
        let out_row = py.saturating_mul(stride);
        for px in 0..w {
            let y_val = to_u8(decoded.y.get(y_row + px).copied().unwrap_or(16));
            let (cb_val, cr_val) = if decoded.chroma == craft_video::apv::ChromaFormat::Monochrome {
                (128, 128)
            } else {
                let cx = if decoded.chroma == craft_video::apv::ChromaFormat::Yuv422 {
                    (px / 2).min(cw.saturating_sub(1))
                } else {
                    px.min(cw.saturating_sub(1))
                };
                (to_u8(decoded.cb.get(c_row + cx).copied().unwrap_or(128)), to_u8(decoded.cr.get(c_row + cx).copied().unwrap_or(128)))
            };
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

fn mpeg2_picture_to_video_frame(pic: &craft_video::mpeg2::Picture, fps: Rational64) -> Result<VideoFrame, EncoderError> {
    let dims = Dimensions::new(pic.width, pic.height)?;
    let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, pic.pts, 1, fps)?;
    let w = pic.width as usize;
    let h = pic.height as usize;
    let cw = pic.chroma_width() as usize;
    let ch = pic.chroma_height() as usize;

    let (_, stride) = out.plane(0)?;
    let (rgb_plane, _) = out.plane_mut(0)?;
    for py in 0..h {
        let y_row = py.saturating_mul(w);
        let cy =
            if pic.chroma == craft_video::mpeg2::ChromaFormat::Yuv420 { (py / 2).min(ch.saturating_sub(1)) } else { py.min(ch.saturating_sub(1)) };
        let c_row = cy.saturating_mul(cw);
        let out_row = py.saturating_mul(stride);
        for px in 0..w {
            let cx = if pic.chroma == craft_video::mpeg2::ChromaFormat::Yuv444 {
                px.min(cw.saturating_sub(1))
            } else {
                (px / 2).min(cw.saturating_sub(1))
            };
            let y_val = i32::from(pic.y.get(y_row + px).copied().unwrap_or(16));
            let cb_val = i32::from(pic.cb.get(c_row + cx).copied().unwrap_or(128));
            let cr_val = i32::from(pic.cr.get(c_row + cx).copied().unwrap_or(128));
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

/// Stateful MPEG-1 / MPEG-2 video decoder wrapper backed by `craft-video`.
pub struct Mpeg2DecoderWrapper {
    inner: craft_video::mpeg2::Decoder,
}

impl Default for Mpeg2DecoderWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl Mpeg2DecoderWrapper {
    pub fn new() -> Self {
        Self { inner: craft_video::mpeg2::Decoder::new() }
    }

    pub fn decode_packet(&mut self, data: &[u8], pts: i64, fps: Rational64) -> Result<Vec<VideoFrame>, EncoderError> {
        let pics = self.inner.decode(data, pts).map_err(|e| EncoderError::Internal(format!("MPEG-2 decode error: {e}")))?;
        pics.iter().map(|p| mpeg2_picture_to_video_frame(p, fps)).collect()
    }

    pub fn flush(&mut self, fps: Rational64) -> Result<Vec<VideoFrame>, EncoderError> {
        let pics = self.inner.flush();
        pics.iter().map(|p| mpeg2_picture_to_video_frame(p, fps)).collect()
    }
}

/// FFV1 v3 lossless video decoder wrapper backed by `craft-video`.
pub struct Ffv1DecoderWrapper {
    inner: craft_video::ffv1::Decoder,
}

impl Ffv1DecoderWrapper {
    pub fn new(config_record: &[u8], width: u32, height: u32) -> Result<Self, EncoderError> {
        let inner = craft_video::ffv1::Decoder::new(config_record, width, height)
            .map_err(|e| EncoderError::Internal(format!("FFV1 decoder init error: {e}")))?;
        Ok(Self { inner })
    }

    pub fn decode_packet(&self, data: &[u8], pts: i64, fps: Rational64) -> Result<VideoFrame, EncoderError> {
        let f = self.inner.decode(data).map_err(|e| EncoderError::Internal(format!("FFV1 decode error: {e}")))?;
        let dims = Dimensions::new(f.width, f.height)?;
        let mut out = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, pts, 1, fps)?;
        let w = f.width as usize;
        let h = f.height as usize;
        let shift = f.bit_depth.saturating_sub(8);
        let to_u8 = |val: u16| -> u8 { ((val >> shift).min(255)) as u8 };

        let (_, stride) = out.plane(0)?;
        let (rgb_plane, _) = out.plane_mut(0)?;
        match f.colorspace {
            craft_video::ffv1::Colorspace::Rgb => {
                let r_pl = f.planes.first().map(Vec::as_slice).unwrap_or(&[]);
                let g_pl = f.planes.get(1).map(Vec::as_slice).unwrap_or(r_pl);
                let b_pl = f.planes.get(2).map(Vec::as_slice).unwrap_or(r_pl);
                for py in 0..h {
                    let row = py * w;
                    let out_row = py * stride;
                    for px in 0..w {
                        let at = row + px;
                        let idx = out_row + px * 3;
                        if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                            dst[0] = to_u8(r_pl.get(at).copied().unwrap_or(0));
                            dst[1] = to_u8(g_pl.get(at).copied().unwrap_or(0));
                            dst[2] = to_u8(b_pl.get(at).copied().unwrap_or(0));
                        }
                    }
                }
            }
            craft_video::ffv1::Colorspace::YCbCr => {
                let y_pl = f.planes.first().map(Vec::as_slice).unwrap_or(&[]);
                let cb_pl = f.planes.get(1).map(Vec::as_slice).unwrap_or(&[]);
                let cr_pl = f.planes.get(2).map(Vec::as_slice).unwrap_or(&[]);
                let cw = w.div_ceil(1usize << f.chroma_shift.0);
                let ch = h.div_ceil(1usize << f.chroma_shift.1);
                for py in 0..h {
                    let y_row = py * w;
                    let cy = (py >> f.chroma_shift.1).min(ch.saturating_sub(1));
                    let c_row = cy * cw;
                    let out_row = py * stride;
                    for px in 0..w {
                        let y_val = i32::from(to_u8(y_pl.get(y_row + px).copied().unwrap_or(16)));
                        let (cb_val, cr_val) = if cb_pl.is_empty() || cr_pl.is_empty() {
                            (128, 128)
                        } else {
                            let cx = (px >> f.chroma_shift.0).min(cw.saturating_sub(1));
                            (
                                i32::from(to_u8(cb_pl.get(c_row + cx).copied().unwrap_or(128))),
                                i32::from(to_u8(cr_pl.get(c_row + cx).copied().unwrap_or(128))),
                            )
                        };
                        let (r, g, b) = bt709_yuv8_to_rgb8(y_val, cb_val, cr_val);
                        let idx = out_row + px * 3;
                        if let Some(dst) = rgb_plane.get_mut(idx..idx + 3) {
                            dst[0] = r;
                            dst[1] = g;
                            dst[2] = b;
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Inspects a compressed video packet's bitstream signature and decodes it if supported
/// (Apple ProRes `icpf`, Avid DNxHD/DNxHR VC-3, APV `aPv1`, or VP8).
pub fn decode_video_packet_auto(
    data: &[u8],
    pts: i64,
    fps: Rational64,
    vp8_decoder: &mut Option<Vp8DecoderWrapper>,
) -> Result<Option<VideoFrame>, EncoderError> {
    if data.len() >= 8 && &data[4..8] == b"icpf" {
        return decode_prores_packet(data, pts, fps).map(Some);
    }
    if data.len() >= 4 && &data[..4] == b"aPv1" {
        return decode_apv_packet(data, pts, fps).map(Some);
    }
    if data.len() >= 5 && data[..4] == [0x00, 0x00, 0x02, 0x80] {
        return decode_dnx_packet(data, pts, fps).map(Some);
    }
    if vp8_decoder.is_some() || craft_video::vp8::is_keyframe(data) {
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

    #[test]
    fn test_apv_encode_decode_roundtrip() {
        let dims = Dimensions::new(128, 72).unwrap();
        let fps = Rational64::FPS_24;
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, fps).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, b) in plane.iter_mut().enumerate() {
            *b = ((i * 5) % 200 + 20) as u8;
        }

        let mut enc = crate::apv::ApvEncoderWrapper::new(128, 72, fps).unwrap();
        assert!(!enc.config_record().is_empty());
        let pkts = enc.encode_frame(&frame).unwrap();
        assert_eq!(pkts.len(), 1);
        assert_eq!(&pkts[0].data[..4], b"aPv1");

        let decoded = decode_apv_packet(&pkts[0].data, 0, fps).unwrap();
        assert_eq!(decoded.dimensions, dims);
        assert_eq!(decoded.format, PixelFormat::Rgb8);
    }

    #[test]
    fn test_mpeg2_encode_decode_roundtrip() {
        let dims = Dimensions::new(128, 72).unwrap();
        let fps = Rational64::FPS_24;
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, fps).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, b) in plane.iter_mut().enumerate() {
            *b = ((i * 9) % 200 + 20) as u8;
        }

        let mut enc = crate::mpeg2::Mpeg2EncoderWrapper::new(128, 72, fps).unwrap();
        assert!(!enc.config_record().is_empty());
        let mut pkts = enc.encode_frame(&frame).unwrap();
        pkts.extend(enc.flush().unwrap());
        assert!(!pkts.is_empty());

        let mut dec = Mpeg2DecoderWrapper::new();
        let mut decoded = Vec::new();
        for pkt in &pkts {
            decoded.extend(dec.decode_packet(&pkt.data, pkt.pts, fps).unwrap());
        }
        decoded.extend(dec.flush(fps).unwrap());
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].dimensions, dims);
    }

    #[test]
    fn test_ffv1_encode_decode_roundtrip() {
        let dims = Dimensions::new(128, 72).unwrap();
        let fps = Rational64::FPS_24;
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, fps).unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        for (i, b) in plane.iter_mut().enumerate() {
            *b = ((i * 13) % 256) as u8;
        }

        let mut enc = crate::ffv1::Ffv1EncoderWrapper::new(128, 72, fps).unwrap();
        let cfg = enc.config_record();
        assert!(!cfg.is_empty());
        let pkts = enc.encode_frame(&frame).unwrap();
        assert_eq!(pkts.len(), 1);

        let dec = Ffv1DecoderWrapper::new(&cfg, 128, 72).unwrap();
        let decoded = dec.decode_packet(&pkts[0].data, 0, fps).unwrap();
        assert_eq!(decoded.dimensions, dims);
        assert_eq!(decoded.plane(0).unwrap().0, frame.plane(0).unwrap().0);
    }
}
