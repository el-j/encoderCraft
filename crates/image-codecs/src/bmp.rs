//! Pure-Rust Windows Bitmap (BMP) image encoder and decoder.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};
use encodercraft_geom::Dimensions;

/// Encodes a `VideoFrame` into a standard uncompressed Windows BMP file.
pub fn encode_bmp(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    let width = frame.dimensions.width as usize;
    let height = frame.dimensions.height as usize;

    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode BMP with zero dimensions".into()));
    }

    let (bpp, has_alpha) = match frame.format {
        PixelFormat::Rgb8 => (24u16, false),
        PixelFormat::Rgba8 => (32u16, true),
        _ => return Err(EncoderError::UnsupportedCodec(format!("BMP encoder requires RGB8 or RGBA8, got {:?}", frame.format))),
    };

    let bytes_per_pixel = (bpp / 8) as usize;
    let row_size = ((bytes_per_pixel * width) + 3) & !3; // 4-byte aligned
    let pixel_data_size = row_size * height;
    let file_size = 54 + pixel_data_size;

    let mut out = Vec::with_capacity(file_size);

    // 1. BMP File Header (14 bytes)
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // Offset to pixel array

    // 2. DIB Header (BITMAPINFOHEADER - 40 bytes)
    out.extend_from_slice(&40u32.to_le_bytes()); // Header size
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // Bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // Color planes
    out.extend_from_slice(&bpp.to_le_bytes()); // Bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // Compression: BI_RGB (uncompressed)
    out.extend_from_slice(&(pixel_data_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes()); // ~72 DPI horizontal (pixels/meter)
    out.extend_from_slice(&2835u32.to_le_bytes()); // ~72 DPI vertical
    out.extend_from_slice(&0u32.to_le_bytes()); // Palette colors
    out.extend_from_slice(&0u32.to_le_bytes()); // Important colors

    // 3. Pixel Array (Bottom-up BGR / BGRA)
    let (plane_data, stride) = frame.plane(0)?;

    for y in (0..height).rev() {
        let row_start = y * stride;
        let mut written_bytes = 0;

        for x in 0..width {
            let px_offset = row_start + x * bytes_per_pixel;
            if has_alpha {
                if let (Some(&r), Some(&g), Some(&b), Some(&a)) =
                    (plane_data.get(px_offset), plane_data.get(px_offset + 1), plane_data.get(px_offset + 2), plane_data.get(px_offset + 3))
                {
                    out.push(b);
                    out.push(g);
                    out.push(r);
                    out.push(a);
                    written_bytes += 4;
                }
            } else if let (Some(&r), Some(&g), Some(&b)) = (plane_data.get(px_offset), plane_data.get(px_offset + 1), plane_data.get(px_offset + 2)) {
                out.push(b);
                out.push(g);
                out.push(r);
                written_bytes += 3;
            }
        }

        // Row padding to 4 bytes
        while written_bytes < row_size {
            out.push(0x00);
            written_bytes += 1;
        }
    }

    Ok(out)
}

/// Decodes an uncompressed 24-bit or 32-bit Windows BMP into a `VideoFrame`.
pub fn decode_bmp(bytes: &[u8]) -> Result<VideoFrame, EncoderError> {
    if bytes.len() < 54 || !bytes.starts_with(b"BM") {
        return Err(EncoderError::InvalidContainer("invalid BMP signature".into()));
    }

    let offset = u32::from_le_bytes([
        bytes.get(10).copied().unwrap_or(0),
        bytes.get(11).copied().unwrap_or(0),
        bytes.get(12).copied().unwrap_or(0),
        bytes.get(13).copied().unwrap_or(0),
    ]) as usize;

    let width = i32::from_le_bytes([
        bytes.get(18).copied().unwrap_or(0),
        bytes.get(19).copied().unwrap_or(0),
        bytes.get(20).copied().unwrap_or(0),
        bytes.get(21).copied().unwrap_or(0),
    ])
    .unsigned_abs();

    let height_raw = i32::from_le_bytes([
        bytes.get(22).copied().unwrap_or(0),
        bytes.get(23).copied().unwrap_or(0),
        bytes.get(24).copied().unwrap_or(0),
        bytes.get(25).copied().unwrap_or(0),
    ]);
    let height = height_raw.unsigned_abs();
    let is_top_down = height_raw < 0;

    let bpp = u16::from_le_bytes([bytes.get(28).copied().unwrap_or(0), bytes.get(29).copied().unwrap_or(0)]);

    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidJobSpec("zero BMP dimensions".into()));
    }

    let dims = Dimensions::new(width, height)?;
    let (format, bytes_per_pixel) = match bpp {
        24 => (PixelFormat::Rgb8, 3usize),
        32 => (PixelFormat::Rgba8, 4usize),
        _ => return Err(EncoderError::UnsupportedCodec(format!("unsupported BMP bit depth: {bpp}"))),
    };

    let mut frame = VideoFrame::new_allocated(dims, format, encodercraft_color::ColorSpaceTag::SRGB, 0, 1, encodercraft_core::Rational64::FPS_24)?;
    let stride = dims.width as usize * bytes_per_pixel;
    let row_size = ((bytes_per_pixel * width as usize) + 3) & !3;

    let (plane_data, _) = frame.plane_mut(0)?;

    for y in 0..(height as usize) {
        let src_row_idx = if is_top_down { y } else { (height as usize) - 1 - y };
        let src_row_start = offset + src_row_idx * row_size;
        let dst_row_start = y * stride;

        for x in 0..(width as usize) {
            let src_px = src_row_start + x * bytes_per_pixel;
            let dst_px = dst_row_start + x * bytes_per_pixel;

            if let Some(dst_slice) = plane_data.get_mut(dst_px..dst_px + bytes_per_pixel) {
                if bpp == 32 {
                    if let (Some(&b), Some(&g), Some(&r), Some(&a)) =
                        (bytes.get(src_px), bytes.get(src_px + 1), bytes.get(src_px + 2), bytes.get(src_px + 3))
                        && let Some(p) = dst_slice.get_mut(0..4)
                    {
                        p[0] = r;
                        p[1] = g;
                        p[2] = b;
                        p[3] = a;
                    }
                } else if let (Some(&b), Some(&g), Some(&r)) = (bytes.get(src_px), bytes.get(src_px + 1), bytes.get(src_px + 2))
                    && let Some(p) = dst_slice.get_mut(0..3)
                {
                    p[0] = r;
                    p[1] = g;
                    p[2] = b;
                }
            }
        }
    }

    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bmp_roundtrip() {
        let dims = Dimensions::new(16, 8).unwrap();
        let mut frame =
            VideoFrame::new_allocated(dims, PixelFormat::Rgba8, encodercraft_color::ColorSpaceTag::SRGB, 0, 1, encodercraft_core::Rational64::FPS_24)
                .unwrap();
        let (plane, _) = frame.plane_mut(0).unwrap();
        plane.fill(128);

        let bmp_bytes = encode_bmp(&frame).unwrap();
        assert!(bmp_bytes.starts_with(b"BM"));

        let decoded = decode_bmp(&bmp_bytes).unwrap();
        assert_eq!(decoded.dimensions.width, 16);
        assert_eq!(decoded.dimensions.height, 8);
        assert_eq!(decoded.format, PixelFormat::Rgba8);
    }
}
