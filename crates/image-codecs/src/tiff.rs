//! Baseline TIFF 6.0 image encoder supporting 8-bit RGB and RGBA frames.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

/// Encodes a `VideoFrame` into an uncompressed Little-Endian TIFF byte vector.
pub fn encode_tiff(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    let width = frame.dimensions.width;
    let height = frame.dimensions.height;

    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode TIFF with zero dimensions".into()));
    }

    let converted;
    let target_format = if frame.format.has_alpha() { PixelFormat::Rgba8 } else { PixelFormat::Rgb8 };
    let (actual_frame, samples_per_pixel, bytes_per_pixel) = match frame.format {
        PixelFormat::Rgba8 => (frame, 4u16, 4usize),
        PixelFormat::Rgb8 => (frame, 3u16, 3usize),
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
            let spp = if target_format == PixelFormat::Rgba8 { 4u16 } else { 3u16 };
            let bpp = if target_format == PixelFormat::Rgba8 { 4usize } else { 3usize };
            (&converted, spp, bpp)
        }
    };

    let (plane_data, stride) = actual_frame.plane(0)?;

    let scanline_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating TIFF scanline size".into()))?;

    let image_bytes = scanline_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating TIFF image size".into()))?;

    // Layout plan:
    // Offset 0: 8-byte TIFF Header ("II", 42, IFD Offset = 8)
    // Offset 8: IFD Entry Count (10 entries = 2 bytes)
    // Offset 10: 10 entries * 12 bytes = 120 bytes
    // Offset 130: Next IFD offset = 0 (4 bytes)
    // Offset 134: Extra data:
    //   - BitsPerSample: 3 or 4 SHORTs (8 bytes)
    //   - XResolution Rational: 72, 1 (8 bytes)
    //   - YResolution Rational: 72, 1 (8 bytes)
    // Offset ~160: Image pixel data

    let ifd_offset: u32 = 8;
    let entry_count: u16 = 10;
    let entries_size = (entry_count as u32) * 12;
    let extra_data_offset = ifd_offset + 2 + entries_size + 4;

    let bits_per_sample_offset = extra_data_offset;
    let x_res_offset = bits_per_sample_offset + 8;
    let y_res_offset = x_res_offset + 8;
    let image_data_offset = y_res_offset + 8;

    let total_file_size = (image_data_offset as usize)
        .checked_add(image_bytes)
        .ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating TIFF total size".into()))?;

    let mut buf = Vec::with_capacity(total_file_size);

    // 1. TIFF Header: Little Endian ("II"), Version 42, IFD offset
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&ifd_offset.to_le_bytes());

    // 2. IFD: Entry count
    buf.extend_from_slice(&entry_count.to_le_bytes());

    // 3. IFD Entries (sorted by tag ID)
    // Tag 256: ImageWidth (LONG)
    write_ifd_entry(&mut buf, 256, 4, 1, width);
    // Tag 257: ImageLength (LONG)
    write_ifd_entry(&mut buf, 257, 4, 1, height);
    // Tag 258: BitsPerSample (SHORT, count = samples_per_pixel, offset)
    write_ifd_entry(&mut buf, 258, 3, samples_per_pixel as u32, bits_per_sample_offset);
    // Tag 259: Compression: 1 = uncompressed
    write_ifd_entry(&mut buf, 259, 3, 1, 1);
    // Tag 262: PhotometricInterpretation: 2 = RGB
    write_ifd_entry(&mut buf, 262, 3, 1, 2);
    // Tag 273: StripOffsets: image_data_offset
    write_ifd_entry(&mut buf, 273, 4, 1, image_data_offset);
    // Tag 277: SamplesPerPixel
    write_ifd_entry(&mut buf, 277, 3, 1, samples_per_pixel as u32);
    // Tag 278: RowsPerStrip = height
    write_ifd_entry(&mut buf, 278, 4, 1, height);
    // Tag 279: StripByteCounts = image_bytes
    write_ifd_entry(&mut buf, 279, 4, 1, image_bytes as u32);
    // Tag 282: XResolution (RATIONAL)
    write_ifd_entry(&mut buf, 282, 5, 1, x_res_offset);

    // 4. Next IFD offset: 0
    buf.extend_from_slice(&0u32.to_le_bytes());

    // 5. Extra data: BitsPerSample (8, 8, 8, 8)
    for _ in 0..samples_per_pixel {
        buf.extend_from_slice(&8u16.to_le_bytes());
    }
    if samples_per_pixel == 3 {
        buf.extend_from_slice(&0u16.to_le_bytes()); // padding to 8 bytes
    }

    // XResolution: 72 / 1
    buf.extend_from_slice(&72u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());

    // YResolution: 72 / 1
    buf.extend_from_slice(&72u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());

    // Pad until image_data_offset if necessary
    while (buf.len() as u32) < image_data_offset {
        buf.push(0);
    }

    // 6. Image pixel data
    for y in 0..(height as usize) {
        let row_start = y * stride;
        let row_end = row_start + scanline_bytes;
        let row_slice =
            plane_data.get(row_start..row_end).ok_or_else(|| EncoderError::Internal(format!("scanline {y} out of bounds in TIFF plane data")))?;
        buf.extend_from_slice(row_slice);
    }

    Ok(buf)
}

fn write_ifd_entry(buf: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value_or_offset: u32) {
    buf.extend_from_slice(&tag.to_le_bytes());
    buf.extend_from_slice(&field_type.to_le_bytes());
    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&value_or_offset.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_encode_tiff_rgb() {
        use encodercraft_core::Rational64;
        let dims = Dimensions::new(2, 2).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let tiff_bytes = encode_tiff(&frame).unwrap();

        assert_eq!(&tiff_bytes[0..4], b"II\x2A\x00");
        assert!(tiff_bytes.len() > 160);
    }
}
