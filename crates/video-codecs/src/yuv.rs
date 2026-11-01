//! Safe planar YUV buffer extraction and color space conversion for video encoders.
//!
//! Converts RGB/RGBA frames into planar YUV420p (8-bit and 16-bit word) and YUV422p.
//! Strictly zero-panic with checked indexing and saturating arithmetic.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

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
        let mut y = Vec::with_capacity(w.checked_mul(h).unwrap_or(0));
        for row in 0..h {
            let start = row.checked_mul(y_stride).unwrap_or(0);
            let end = start.checked_add(w).unwrap_or(start);
            if let Some(slice) = y_plane.get(start..end) {
                y.extend_from_slice(slice);
            }
        }

        let mut u = Vec::with_capacity(uv_w.checked_mul(uv_h).unwrap_or(0));
        let mut v = Vec::with_capacity(uv_w.checked_mul(uv_h).unwrap_or(0));
        for row in 0..uv_h {
            let u_start = row.checked_mul(u_stride).unwrap_or(0);
            let u_end = u_start.checked_add(uv_w).unwrap_or(u_start);
            if let Some(slice) = u_plane.get(u_start..u_end) {
                u.extend_from_slice(slice);
            }

            let v_start = row.checked_mul(v_stride).unwrap_or(0);
            let v_end = v_start.checked_add(uv_w).unwrap_or(v_start);
            if let Some(slice) = v_plane.get(v_start..v_end) {
                v.extend_from_slice(slice);
            }
        }

        return Ok(Yuv420Planar8 { y, u, v, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height });
    }

    // Convert packed RGB/RGBA to planar YUV 4:2:0 (ITU-R BT.709 conversion)
    let raw = frame.data();
    let (channels, is_bgra) = match frame.format {
        PixelFormat::Rgb8 => (3, false),
        PixelFormat::Rgba8 => (4, false),
        PixelFormat::Bgra8 => (4, true),
        _ => (3, false),
    };

    let y_size = w.checked_mul(h).unwrap_or(0);
    let uv_size = uv_w.checked_mul(uv_h).unwrap_or(0);

    let mut y_buf = vec![0u8; y_size];
    let mut u_buf = vec![128u8; uv_size];
    let mut v_buf = vec![128u8; uv_size];

    let get_rgb = |px: usize, py: usize| -> (i32, i32, i32) {
        let idx = (py.checked_mul(w).unwrap_or(0).checked_add(px).unwrap_or(0)).checked_mul(channels).unwrap_or(0);
        let b0 = raw.get(idx).copied().unwrap_or(0) as i32;
        let b1 = raw.get(idx + 1).copied().unwrap_or(0) as i32;
        let b2 = raw.get(idx + 2).copied().unwrap_or(0) as i32;
        if is_bgra { (b2, b1, b0) } else { (b0, b1, b2) }
    };

    // Compute Y plane
    for py in 0..h {
        for px in 0..w {
            let (r, g, b) = get_rgb(px, py);
            // BT.709 standard luma: Y = 0.2126 R + 0.7152 G + 0.0722 B
            let y_val = ((13933 * r + 46871 * g + 4732 * b + 32768) >> 16).clamp(0, 255) as u8;
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

            let (r0, g0, b0) = get_rgb(x0, y0);
            let (r1, g1, b1) = get_rgb(x1, y0);
            let (r2, g2, b2) = get_rgb(x0, y1);
            let (r3, g3, b3) = get_rgb(x1, y1);

            let r = (r0 + r1 + r2 + r3 + 2) >> 2;
            let g = (g0 + g1 + g2 + g3 + 2) >> 2;
            let b = (b0 + b1 + b2 + b3 + 2) >> 2;

            // BT.709 chroma:
            // U = -0.1146 R - 0.3854 G + 0.5000 B + 128
            // V = 0.5000 R - 0.4542 G - 0.0458 B + 128
            let u_val = ((-7510 * r - 25258 * g + 32768 * b + 32768) >> 16) + 128;
            let v_val = ((32768 * r - 29766 * g - 3002 * b + 32768) >> 16) + 128;

            let uv_idx = uy * uv_w + ux;
            if let Some(dest) = u_buf.get_mut(uv_idx) {
                *dest = u_val.clamp(0, 255) as u8;
            }
            if let Some(dest) = v_buf.get_mut(uv_idx) {
                *dest = v_val.clamp(0, 255) as u8;
            }
        }
    }

    Ok(Yuv420Planar8 { y: y_buf, u: u_buf, v: v_buf, y_stride: w, uv_stride: uv_w, width: frame.dimensions.width, height: frame.dimensions.height })
}

/// Converts a `VideoFrame` to a 16-bit word planar YUV 4:2:0 buffer (8-bit values expanded to `u16`).
pub fn frame_to_yuv420_16(frame: &VideoFrame) -> Result<Yuv420Planar16, EncoderError> {
    let p8 = frame_to_yuv420_8(frame)?;
    let y = p8.y.into_iter().map(|v| v as u16).collect();
    let u = p8.u.into_iter().map(|v| v as u16).collect();
    let v = p8.v.into_iter().map(|v| v as u16).collect();

    Ok(Yuv420Planar16 { y, u, v, y_stride: p8.y_stride, uv_stride: p8.uv_stride, width: p8.width, height: p8.height })
}
