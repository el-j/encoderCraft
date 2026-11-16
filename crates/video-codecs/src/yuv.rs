//! Safe planar YUV buffer extraction and color space conversion for video encoders.
//!
//! Converts RGB/RGBA frames into planar YUV420p (8-bit and 16-bit word) and YUV422p.
//! Strictly zero-panic with checked indexing and saturating arithmetic.

use encodercraft_color::{MatrixCoefficients, PixelFormat};
use encodercraft_core::{EncoderError, VideoFrame};

fn rgba_f32_to_rgb(pixel: &[u8]) -> Result<[f64; 3], EncoderError> {
    let alpha_bytes = pixel.get(12..16).ok_or_else(|| EncoderError::Internal("RGBAF32 pixel has no alpha channel".into()))?;
    let alpha = f32::from_ne_bytes(alpha_bytes.try_into().map_err(|_| EncoderError::Internal("RGBAF32 alpha channel is invalid".into()))?);
    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err(EncoderError::InvalidContainer("RGBAF32 frame contains an invalid alpha sample".into()));
    }
    let mut rgb = [0.0; 3];
    for (channel, sample) in rgb.iter_mut().enumerate() {
        let start = channel * 4;
        let bytes = pixel.get(start..start + 4).ok_or_else(|| EncoderError::Internal("RGBAF32 pixel is incomplete".into()))?;
        let value = f32::from_ne_bytes(bytes.try_into().map_err(|_| EncoderError::Internal("RGBAF32 color channel is invalid".into()))?);
        if !value.is_finite() {
            return Err(EncoderError::InvalidContainer("RGBAF32 frame contains a non-finite color sample".into()));
        }
        *sample = f64::from(value.clamp(0.0, 1.0));
    }
    Ok(rgb)
}

/// 8-bit planar 4:2:0 representation.
#[derive(Debug, Clone)]
pub struct Yuv420Planar8 {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub y_stride: usize,
    pub uv_stride: usize,
    pub width: u32,
    pub height: u32,
}

/// 16-bit word planar 4:2:0 representation (8-bit or 10-bit samples in `u16`).
#[derive(Debug, Clone)]
pub struct Yuv420Planar16 {
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    pub y_stride: usize,
    pub uv_stride: usize,
    pub width: u32,
    pub height: u32,
}

/// 16-bit word planar YUV 4:2:2 representation (8-bit or 10-bit samples in `u16`).
#[derive(Debug, Clone)]
pub struct Yuv422Planar16 {
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    pub y_stride: usize,
    pub uv_stride: usize,
    pub width: u32,
    pub height: u32,
}

/// Converts a `VideoFrame` to an 8-bit planar YUV 4:2:0 buffer.
pub fn frame_to_yuv420_8(frame: &VideoFrame) -> Result<Yuv420Planar8, EncoderError> {
    let w = frame.dimensions.width as usize;
    let h = frame.dimensions.height as usize;
    let uv_w = frame.dimensions.width.div_ceil(2) as usize;
    let uv_h = frame.dimensions.height.div_ceil(2) as usize;

    if frame.format == PixelFormat::Yuv420p {
        let (y_plane, y_stride) = frame.plane(0)?;
        let (u_plane, u_stride) = frame.plane(1)?;
        let (v_plane, v_stride) = frame.plane(2)?;

        // Pack into tight rows if strides differ
        let y_capacity = w.checked_mul(h).ok_or_else(|| EncoderError::Internal("YUV luma dimensions overflow".into()))?;
        let uv_capacity = uv_w.checked_mul(uv_h).ok_or_else(|| EncoderError::Internal("YUV chroma dimensions overflow".into()))?;
        let mut y = Vec::with_capacity(y_capacity);
        for row in 0..h {
            let start = row.checked_mul(y_stride).ok_or_else(|| EncoderError::Internal("YUV luma row offset overflow".into()))?;
            let end = start.checked_add(w).ok_or_else(|| EncoderError::Internal("YUV luma row end overflow".into()))?;
            let slice = y_plane.get(start..end).ok_or_else(|| EncoderError::Internal("YUV luma row is out of range".into()))?;
            y.extend_from_slice(slice);
        }

        let mut u = Vec::with_capacity(uv_capacity);
        let mut v = Vec::with_capacity(uv_capacity);
        for row in 0..uv_h {
            let u_start = row.checked_mul(u_stride).ok_or_else(|| EncoderError::Internal("YUV U row offset overflow".into()))?;
            let u_end = u_start.checked_add(uv_w).ok_or_else(|| EncoderError::Internal("YUV U row end overflow".into()))?;
            let slice = u_plane.get(u_start..u_end).ok_or_else(|| EncoderError::Internal("YUV U row is out of range".into()))?;
            u.extend_from_slice(slice);

            let v_start = row.checked_mul(v_stride).ok_or_else(|| EncoderError::Internal("YUV V row offset overflow".into()))?;
            let v_end = v_start.checked_add(uv_w).ok_or_else(|| EncoderError::Internal("YUV V row end overflow".into()))?;
            let slice = v_plane.get(v_start..v_end).ok_or_else(|| EncoderError::Internal("YUV V row is out of range".into()))?;
            v.extend_from_slice(slice);
        }

        return Ok(Yuv420Planar8 { y, u, v, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height });
    }
    if frame.format == PixelFormat::RgbaF32 {
        let yuv = frame_to_yuv420_16_float(frame)?;
        return Ok(Yuv420Planar8 {
            y: yuv.y.into_iter().map(|value| (value >> 2) as u8).collect(),
            u: yuv.u.into_iter().map(|value| (value >> 2) as u8).collect(),
            v: yuv.v.into_iter().map(|value| (value >> 2) as u8).collect(),
            y_stride: yuv.y_stride,
            uv_stride: yuv.uv_stride,
            width: yuv.width,
            height: yuv.height,
        });
    }

    // Encoders in this crate signal limited-range YUV; convert according to the frame's matrix tag.
    let bpp = match frame.format {
        PixelFormat::Rgb8 => 3,
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
        format => return Err(EncoderError::UnsupportedCodec(format!("video encoders require RGB8/RGBA8 or YUV420p input, got {format:?}"))),
    };
    let (raw, stride) = frame.plane(0)?;
    let (kr, kb) = match frame.color_space.matrix {
        MatrixCoefficients::Smpte170M => (0.299, 0.114),
        MatrixCoefficients::Bt2020NonConstant => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;

    let y_size = w.checked_mul(h).ok_or_else(|| EncoderError::Internal("YUV luma dimensions overflow".into()))?;
    let uv_size = uv_w.checked_mul(uv_h).ok_or_else(|| EncoderError::Internal("YUV chroma dimensions overflow".into()))?;

    let mut y_buf = vec![0u8; y_size];
    let mut u_buf = vec![128u8; uv_size];
    let mut v_buf = vec![128u8; uv_size];

    let get_rgb = |px: usize, py: usize| -> Result<(u8, u8, u8), EncoderError> {
        let idx = py
            .checked_mul(stride)
            .and_then(|row| px.checked_mul(bpp).and_then(|col| row.checked_add(col)))
            .ok_or_else(|| EncoderError::Internal("RGB to YUV pixel offset overflow".into()))?;
        let end = idx.checked_add(bpp).ok_or_else(|| EncoderError::Internal("RGB to YUV pixel offset overflow".into()))?;
        let pixel = raw.get(idx..end).ok_or_else(|| EncoderError::Internal("RGB to YUV pixel is out of range".into()))?;
        let b0 = pixel.first().copied().ok_or_else(|| EncoderError::Internal("RGB pixel has no red/blue channel".into()))? as i32;
        let b1 = pixel.get(1).copied().ok_or_else(|| EncoderError::Internal("RGB pixel has no green channel".into()))? as i32;
        let b2 = pixel.get(2).copied().ok_or_else(|| EncoderError::Internal("RGB pixel has no blue/red channel".into()))? as i32;
        if frame.format == PixelFormat::Bgra8 { Ok((b2 as u8, b1 as u8, b0 as u8)) } else { Ok((b0 as u8, b1 as u8, b2 as u8)) }
    };

    for py in 0..h {
        for px in 0..w {
            let (r, g, b) = get_rgb(px, py)?;
            let luma = kr * f64::from(r) + kg * f64::from(g) + kb * f64::from(b);
            let y_val = (16.0 + 219.0 * luma / 255.0).round().clamp(16.0, 235.0) as u8;
            if let Some(dest) = y_buf.get_mut(py * w + px) {
                *dest = y_val;
            }
        }
    }

    // Compute subsampled U and V planes (2x2 averaging)
    for uy in 0..uv_h {
        let y0 = uy * 2;
        let y1 = (y0 + 1).min(h.saturating_sub(1));
        for ux in 0..uv_w {
            let x0 = ux * 2;
            let x1 = (x0 + 1).min(w.saturating_sub(1));

            let (r0, g0, b0) = get_rgb(x0, y0)?;
            let (r1, g1, b1) = get_rgb(x1, y0)?;
            let (r2, g2, b2) = get_rgb(x0, y1)?;
            let (r3, g3, b3) = get_rgb(x1, y1)?;

            let r = f64::from(r0) + f64::from(r1) + f64::from(r2) + f64::from(r3);
            let g = f64::from(g0) + f64::from(g1) + f64::from(g2) + f64::from(g3);
            let b = f64::from(b0) + f64::from(b1) + f64::from(b2) + f64::from(b3);
            let y = (kr * r + kg * g + kb * b) / 4.0;
            let u_val = (128.0 + 224.0 * (b / 4.0 - y) / (2.0 * (1.0 - kb) * 255.0)).round().clamp(16.0, 240.0) as u8;
            let v_val = (128.0 + 224.0 * (r / 4.0 - y) / (2.0 * (1.0 - kr) * 255.0)).round().clamp(16.0, 240.0) as u8;

            let uv_idx = uy * uv_w + ux;
            if let Some(dest) = u_buf.get_mut(uv_idx) {
                *dest = u_val;
            }
            if let Some(dest) = v_buf.get_mut(uv_idx) {
                *dest = v_val;
            }
        }
    }

    Ok(Yuv420Planar8 { y: y_buf, u: u_buf, v: v_buf, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height })
}

/// Converts 8-bit limited-range YUV to 10-bit limited-range code values stored in `u16`.
pub fn frame_to_yuv420_16(frame: &VideoFrame) -> Result<Yuv420Planar16, EncoderError> {
    if frame.format == PixelFormat::RgbaF32 {
        return frame_to_yuv420_16_float(frame);
    }
    let p8 = frame_to_yuv420_8(frame)?;
    let y = p8.y.into_iter().map(|v| u16::from(v) * 4).collect();
    let u = p8.u.into_iter().map(|v| u16::from(v) * 4).collect();
    let v = p8.v.into_iter().map(|v| u16::from(v) * 4).collect();

    Ok(Yuv420Planar16 { y, u, v, y_stride: p8.y_stride, uv_stride: p8.uv_stride, width: p8.width, height: p8.height })
}

/// Converts a high-precision RGB frame directly to 10-bit limited-range YUV 4:2:0.
fn frame_to_yuv420_16_float(frame: &VideoFrame) -> Result<Yuv420Planar16, EncoderError> {
    if frame.format != PixelFormat::RgbaF32 {
        return Err(EncoderError::UnsupportedCodec(format!("10-bit YUV conversion requires RGBAF32 input, got {:?}", frame.format)));
    }
    let w = frame.dimensions.width as usize;
    let h = frame.dimensions.height as usize;
    let uv_w = frame.dimensions.width.div_ceil(2) as usize;
    let uv_h = frame.dimensions.height.div_ceil(2) as usize;
    let (raw, stride) = frame.plane(0)?;
    let (kr, kb) = match frame.color_space.matrix {
        MatrixCoefficients::Smpte170M => (0.299, 0.114),
        MatrixCoefficients::Bt2020NonConstant => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let read_rgb = |px: usize, py: usize| -> Result<[f64; 3], EncoderError> {
        let start = py
            .checked_mul(stride)
            .and_then(|row| px.checked_mul(16).and_then(|col| row.checked_add(col)))
            .ok_or_else(|| EncoderError::Internal("RGBAF32 to YUV pixel offset overflow".into()))?;
        let pixel = raw.get(start..start + 16).ok_or_else(|| EncoderError::Internal("RGBAF32 to YUV pixel is out of range".into()))?;
        rgba_f32_to_rgb(pixel)
    };
    let luma_code = |[r, g, b]: [f64; 3]| -> u16 { (64.0 + 876.0 * (kr * r + kg * g + kb * b)).round().clamp(64.0, 940.0) as u16 };
    let chroma_codes = |r: f64, g: f64, b: f64| -> (u16, u16) {
        let y = kr * r + kg * g + kb * b;
        let u = (512.0 + 896.0 * (b - y) / (2.0 * (1.0 - kb))).round().clamp(64.0, 960.0) as u16;
        let v = (512.0 + 896.0 * (r - y) / (2.0 * (1.0 - kr))).round().clamp(64.0, 960.0) as u16;
        (u, v)
    };
    let y_size = w.checked_mul(h).ok_or_else(|| EncoderError::Internal("10-bit YUV luma dimensions overflow".into()))?;
    let uv_size = uv_w.checked_mul(uv_h).ok_or_else(|| EncoderError::Internal("10-bit YUV chroma dimensions overflow".into()))?;
    let mut y = vec![0u16; y_size];
    let mut u = vec![512u16; uv_size];
    let mut v = vec![512u16; uv_size];

    for py in 0..h {
        for px in 0..w {
            let index = py * w + px;
            *y.get_mut(index).ok_or_else(|| EncoderError::Internal("10-bit YUV luma destination is out of range".into()))? =
                luma_code(read_rgb(px, py)?);
        }
    }
    for uy in 0..uv_h {
        let y0 = uy * 2;
        let y1 = (y0 + 1).min(h.saturating_sub(1));
        for ux in 0..uv_w {
            let x0 = ux * 2;
            let x1 = (x0 + 1).min(w.saturating_sub(1));
            let samples = [read_rgb(x0, y0)?, read_rgb(x1, y0)?, read_rgb(x0, y1)?, read_rgb(x1, y1)?];
            let (r, g, b) = samples.iter().fold((0.0, 0.0, 0.0), |(r, g, b), rgb| (r + rgb[0], g + rgb[1], b + rgb[2]));
            let (u_code, v_code) = chroma_codes(r / 4.0, g / 4.0, b / 4.0);
            let index = uy * uv_w + ux;
            *u.get_mut(index).ok_or_else(|| EncoderError::Internal("10-bit YUV U destination is out of range".into()))? = u_code;
            *v.get_mut(index).ok_or_else(|| EncoderError::Internal("10-bit YUV V destination is out of range".into()))? = v_code;
        }
    }
    Ok(Yuv420Planar16 { y, u, v, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height })
}

/// Converts an RGB frame to planar 10-bit YUV 4:2:2, preserving high-depth samples.
pub fn frame_to_yuv422_16(frame: &VideoFrame) -> Result<Yuv422Planar16, EncoderError> {
    let w = frame.dimensions.width as usize;
    let h = frame.dimensions.height as usize;
    let uv_w = frame.dimensions.width.div_ceil(2) as usize;
    let (raw, stride) = frame.plane(0)?;
    let bytes_per_pixel = match frame.format {
        PixelFormat::Rgb8 => 3,
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
        PixelFormat::RgbaF32 => 16,
        format => return Err(EncoderError::UnsupportedCodec(format!("10-bit YUV422 conversion does not support {format:?} frames"))),
    };
    let (kr, kb) = match frame.color_space.matrix {
        MatrixCoefficients::Smpte170M => (0.299, 0.114),
        MatrixCoefficients::Bt2020NonConstant => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let read_rgb = |px: usize, py: usize| -> Result<[f64; 3], EncoderError> {
        let start = py
            .checked_mul(stride)
            .and_then(|row| px.checked_mul(bytes_per_pixel).and_then(|column| row.checked_add(column)))
            .ok_or_else(|| EncoderError::Internal("RGB to YUV422 pixel offset overflow".into()))?;
        let end = start.checked_add(bytes_per_pixel).ok_or_else(|| EncoderError::Internal("YUV422 pixel end overflow".into()))?;
        let pixel = raw.get(start..end).ok_or_else(|| EncoderError::Internal("YUV422 pixel is out of range".into()))?;
        let mut rgb = [0.0_f64; 3];
        if frame.format == PixelFormat::RgbaF32 {
            rgb = rgba_f32_to_rgb(pixel)?;
        } else {
            let (r, g, b) = if frame.format == PixelFormat::Bgra8 {
                (pixel.get(2), pixel.get(1), pixel.first())
            } else {
                (pixel.first(), pixel.get(1), pixel.get(2))
            };
            for (sample, byte) in rgb.iter_mut().zip([r, g, b]) {
                *sample = f64::from(byte.copied().ok_or_else(|| EncoderError::Internal("RGB to YUV422 channel is out of range".into()))?) / 255.0;
            }
        }
        Ok(rgb)
    };
    let y_size = w.checked_mul(h).ok_or_else(|| EncoderError::Internal("10-bit YUV422 luma dimensions overflow".into()))?;
    let uv_size = uv_w.checked_mul(h).ok_or_else(|| EncoderError::Internal("10-bit YUV422 chroma dimensions overflow".into()))?;
    let mut y = vec![0u16; y_size];
    let mut u = vec![512u16; uv_size];
    let mut v = vec![512u16; uv_size];
    for py in 0..h {
        for px in 0..w {
            let rgb = read_rgb(px, py)?;
            let y_code = (64.0 + 876.0 * (kr * rgb[0] + kg * rgb[1] + kb * rgb[2])).round().clamp(64.0, 940.0) as u16;
            *y.get_mut(py * w + px).ok_or_else(|| EncoderError::Internal("10-bit YUV422 luma destination is out of range".into()))? = y_code;
        }
        for ux in 0..uv_w {
            let x0 = ux * 2;
            let x1 = (x0 + 1).min(w.saturating_sub(1));
            let a = read_rgb(x0, py)?;
            let b = read_rgb(x1, py)?;
            let (r, g, b) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0);
            let luma = kr * r + kg * g + kb * b;
            let u_code = (512.0 + 896.0 * (b - luma) / (2.0 * (1.0 - kb))).round().clamp(64.0, 960.0) as u16;
            let v_code = (512.0 + 896.0 * (r - luma) / (2.0 * (1.0 - kr))).round().clamp(64.0, 960.0) as u16;
            let index = py * uv_w + ux;
            *u.get_mut(index).ok_or_else(|| EncoderError::Internal("10-bit YUV422 U destination is out of range".into()))? = u_code;
            *v.get_mut(index).ok_or_else(|| EncoderError::Internal("10-bit YUV422 V destination is out of range".into()))? = v_code;
        }
    }
    Ok(Yuv422Planar16 { y, u, v, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height })
}

/// Converts an RGBAF32 frame to tightly packed RGB8 at the 8-bit encoder boundary.
pub fn rgba_f32_to_rgb8(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    if frame.format != PixelFormat::RgbaF32 {
        return Err(EncoderError::UnsupportedCodec(format!("RGB8 quantization requires RGBAF32 input, got {:?}", frame.format)));
    }
    let width = frame.dimensions.width as usize;
    let height = frame.dimensions.height as usize;
    let capacity = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| EncoderError::InvalidJobSpec("RGB8 output dimensions overflow".into()))?;
    let (source, stride) = frame.plane(0)?;
    let mut output = Vec::with_capacity(capacity);
    for y in 0..height {
        for x in 0..width {
            let start = y
                .checked_mul(stride)
                .and_then(|row| x.checked_mul(16).and_then(|column| row.checked_add(column)))
                .ok_or_else(|| EncoderError::Internal("RGBAF32 to RGB8 pixel offset overflow".into()))?;
            let end = start.checked_add(16).ok_or_else(|| EncoderError::Internal("RGBAF32 to RGB8 pixel end overflow".into()))?;
            let pixel = source.get(start..end).ok_or_else(|| EncoderError::Internal("RGBAF32 to RGB8 pixel is out of range".into()))?;
            output.extend(rgba_f32_to_rgb(pixel)?.map(|value| (value * 255.0).round() as u8));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, MatrixCoefficients};
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    fn solid_frame(rgb: [u8; 3], matrix: MatrixCoefficients) -> VideoFrame {
        let dims = Dimensions::new(2, 2).unwrap();
        let mut frame =
            VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag { matrix, ..ColorSpaceTag::REC709 }, 0, 1, Rational64::FPS_24).unwrap();
        let (data, stride) = frame.plane_mut(0).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                let offset = y * stride + x * 4;
                data[offset..offset + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        frame
    }

    fn float_gradient_frame() -> VideoFrame {
        let dims = Dimensions::new(2, 2).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::RgbaF32, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_24).unwrap();
        let (data, stride) = frame.plane_mut(0).unwrap();
        for y in 0..2 {
            for x in 0..2 {
                let value = if x == 0 { 0.1_f32 } else { 0.102_f32 };
                let mut offset = y * stride + x * 16;
                for channel in [value, value, value, 1.0] {
                    data[offset..offset + 4].copy_from_slice(&channel.to_ne_bytes());
                    offset += 4;
                }
            }
        }
        frame
    }

    #[test]
    fn rgb_to_limited_range_yuv_uses_studio_black_and_white() {
        let black = frame_to_yuv420_8(&solid_frame([0, 0, 0], MatrixCoefficients::Bt709)).unwrap();
        let white = frame_to_yuv420_8(&solid_frame([255, 255, 255], MatrixCoefficients::Bt709)).unwrap();
        assert_eq!(black.y, vec![16; 4]);
        assert_eq!(white.y, vec![235; 4]);
        assert_eq!(black.u, vec![128]);
        assert_eq!(black.v, vec![128]);
    }

    #[test]
    fn rgb_to_yuv_uses_the_tagged_luma_matrix() {
        let red_709 = frame_to_yuv420_8(&solid_frame([255, 0, 0], MatrixCoefficients::Bt709)).unwrap();
        let red_601 = frame_to_yuv420_8(&solid_frame([255, 0, 0], MatrixCoefficients::Smpte170M)).unwrap();
        assert!(red_601.y[0] > red_709.y[0]);
    }

    #[test]
    fn ten_bit_yuv_scales_limited_range_code_values() {
        let black = frame_to_yuv420_16(&solid_frame([0, 0, 0], MatrixCoefficients::Bt709)).unwrap();
        let white = frame_to_yuv420_16(&solid_frame([255, 255, 255], MatrixCoefficients::Bt709)).unwrap();
        assert_eq!(black.y, vec![64; 4]);
        assert_eq!(white.y, vec![940; 4]);
        assert_eq!(black.u, vec![512]);
        assert_eq!(black.v, vec![512]);
    }

    #[test]
    fn float_working_frames_keep_precision_until_the_ten_bit_output_boundary() {
        let frame = float_gradient_frame();
        let rgb8 = rgba_f32_to_rgb8(&frame).unwrap();
        assert_eq!(rgb8[0..3], rgb8[3..6], "the source samples intentionally quantize to the same RGB8 value");
        let yuv8 = frame_to_yuv420_8(&frame).unwrap();
        assert_eq!(yuv8.y[0], yuv8.y[1], "the 8-bit output should quantize the close source samples identically");
        let yuv10 = frame_to_yuv420_16(&frame).unwrap();
        assert_ne!(yuv10.y[0], yuv10.y[1], "the 10-bit conversion must retain distinctions lost by RGB8 quantization");
        let yuv422_10 = frame_to_yuv422_16(&frame).unwrap();
        assert_ne!(yuv422_10.y[0], yuv422_10.y[1], "the 10-bit 4:2:2 output must retain the same source precision");
    }

    #[test]
    fn float_to_rgb8_rejects_non_finite_samples() {
        let mut frame = float_gradient_frame();
        let (data, _) = frame.plane_mut(0).unwrap();
        data[..4].copy_from_slice(&f32::NAN.to_ne_bytes());
        assert!(matches!(rgba_f32_to_rgb8(&frame), Err(EncoderError::InvalidContainer(_))));
    }

    #[test]
    fn float_encoder_boundaries_discard_alpha_without_changing_rgb() {
        let dims = Dimensions::new(2, 2).unwrap();
        let mut opaque = VideoFrame::new_allocated(dims, PixelFormat::RgbaF32, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_24).unwrap();
        let mut translucent = opaque.clone();
        for pixel in opaque.data_mut().as_chunks_mut::<16>().0 {
            for (channel, value) in [1.0_f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
                pixel[channel * 4..channel * 4 + 4].copy_from_slice(&value.to_ne_bytes());
            }
        }
        for pixel in translucent.data_mut().as_chunks_mut::<16>().0 {
            for (channel, value) in [1.0_f32, 0.0, 0.0, 0.5].into_iter().enumerate() {
                pixel[channel * 4..channel * 4 + 4].copy_from_slice(&value.to_ne_bytes());
            }
        }

        assert_eq!(rgba_f32_to_rgb8(&translucent).unwrap(), rgba_f32_to_rgb8(&opaque).unwrap());
        assert_eq!(frame_to_yuv420_16(&translucent).unwrap().y, frame_to_yuv420_16(&opaque).unwrap().y);
        assert_eq!(frame_to_yuv422_16(&translucent).unwrap().y, frame_to_yuv422_16(&opaque).unwrap().y);
    }
}
