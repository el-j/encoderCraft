//! Pure-Rust PNG (Portable Network Graphics) image encoder.
//!
//! Complies with W3C PNG Specification (Second Edition) / ISO/IEC 15948:2004.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Encodes a `VideoFrame` into a standard PNG byte vector.
pub fn encode_png(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    let width = frame.dimensions.width;
    let height = frame.dimensions.height;

    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode PNG with zero dimensions".into()));
    }

    let converted;
    let target_format = if frame.format.has_alpha() { PixelFormat::Rgba8 } else { PixelFormat::Rgb8 };
    let (actual_frame, color_type, bytes_per_pixel) = match frame.format {
        PixelFormat::Rgba8 => (frame, 6u8, 4usize),
        PixelFormat::Rgb8 => (frame, 2u8, 3usize),
        _ => {
            let converted_bytes = encodercraft_color::convert_pixels(frame.data(), frame.format, target_format, width, height)
                .map_err(|e| EncoderError::Internal(format!("color conversion failed: {e}")))?;
            converted = VideoFrame::from_raw_bytes(
                frame.dimensions,
                target_format,
                frame.color_space,
                frame.pts,
                frame.duration,
                frame.timebase,
                converted_bytes,
            )?;
            let ct = if target_format == PixelFormat::Rgba8 { 6u8 } else { 2u8 };
            let bpp = if target_format == PixelFormat::Rgba8 { 4usize } else { 3usize };
            (&converted, ct, bpp)
        }
    };

    let (plane_data, stride) = actual_frame.plane(0)?;

    let scanline_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating PNG scanline size".into()))?;

    // Filter 0 (None) prepended to each scanline: height * (1 + scanline_bytes)
    let raw_payload_len = (height as usize)
        .checked_mul(scanline_bytes + 1)
        .ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating PNG raw data size".into()))?;

    let mut raw_scanlines = Vec::with_capacity(raw_payload_len);

    for y in 0..(height as usize) {
        raw_scanlines.push(0u8); // Filter type: None

        let row_start = y * stride;
        let row_end = row_start + scanline_bytes;

        let row_slice =
            plane_data.get(row_start..row_end).ok_or_else(|| EncoderError::Internal(format!("scanline {y} out of bounds in plane data")))?;

        raw_scanlines.extend_from_slice(row_slice);
    }

    // Compress raw scanlines with zlib DEFLATE
    let compressed_idat = miniz_oxide::deflate::compress_to_vec_zlib(&raw_scanlines, 6);

    let mut output = Vec::with_capacity(PNG_SIGNATURE.len() + 100 + compressed_idat.len());

    // 1. Signature
    output.extend_from_slice(&PNG_SIGNATURE);

    // 2. IHDR Chunk: 13 bytes
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // Bit depth: 8
    ihdr.push(color_type); // Color type
    ihdr.push(0); // Compression method: 0 (DEFLATE)
    ihdr.push(0); // Filter method: 0 (adaptive)
    ihdr.push(0); // Interlace method: 0 (no interlace)
    write_chunk(&mut output, b"IHDR", &ihdr);

    // 3. IDAT Chunk
    write_chunk(&mut output, b"IDAT", &compressed_idat);

    // 4. IEND Chunk
    write_chunk(&mut output, b"IEND", &[]);

    Ok(output)
}

fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let length = data.len() as u32;
    output.extend_from_slice(&length.to_be_bytes());

    let crc_start = output.len();
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);

    let crc = calculate_crc32(&output[crc_start..]);
    output.extend_from_slice(&crc.to_be_bytes());
}

/// Standard IEEE 802.3 CRC32 calculation.
fn calculate_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = ((crc & 1) as i32).wrapping_neg() as u32;
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_encode_png_rgba() {
        use encodercraft_core::Rational64;
        let dims = Dimensions::new(4, 4).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

        // Paint first pixel red
        let (plane_data, _) = frame.plane_mut(0).unwrap();
        plane_data[0] = 255;
        plane_data[1] = 0;
        plane_data[2] = 0;
        plane_data[3] = 255;

        let png_bytes = encode_png(&frame).unwrap();
        assert_eq!(&png_bytes[0..8], &PNG_SIGNATURE);
        // Verify IHDR is present
        assert_eq!(&png_bytes[12..16], b"IHDR");
        // Verify IEND is at the end
        let len = png_bytes.len();
        assert_eq!(&png_bytes[len - 8..len - 4], b"IEND");
    }
}
