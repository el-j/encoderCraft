//! Minimal OpenEXR scanline image encoder.
//!
//! Complies with OpenEXR 2.x/3.x file format specification for single-part scanline files.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

const EXR_MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];

/// Encodes an RGBA8 or RGB8 `VideoFrame` into an uncompressed OpenEXR file with half-float channels.
pub fn encode_exr_half(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    let width = frame.dimensions.width;
    let height = frame.dimensions.height;

    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode EXR with zero dimensions".into()));
    }

    let has_alpha = match frame.format {
        PixelFormat::Rgba8 => true,
        PixelFormat::Rgb8 => false,
        _ => {
            return Err(EncoderError::UnsupportedCodec(format!("EXR encoder requires RGB8 or RGBA8 pixel format, got {:?}", frame.format)));
        }
    };

    let (plane_data, stride) = frame.plane(0)?;

    let mut header = Vec::with_capacity(512);

    // 1. Magic number & version (Single part scanline = 2)
    header.extend_from_slice(&EXR_MAGIC);
    header.extend_from_slice(&2u32.to_le_bytes());

    // 2. Attributes
    // channels attribute: type = "chlist"
    // OpenEXR requires channels sorted alphabetically: A, B, G, R
    let mut ch_data = Vec::new();
    if has_alpha {
        write_channel_spec(&mut ch_data, b"A", 1); // 1 = HALF (16-bit float)
    }
    write_channel_spec(&mut ch_data, b"B", 1);
    write_channel_spec(&mut ch_data, b"G", 1);
    write_channel_spec(&mut ch_data, b"R", 1);
    ch_data.push(0); // list terminator

    write_attribute(&mut header, "channels", "chlist", &ch_data);

    // compression attribute: 0 = NO_COMPRESSION
    write_attribute(&mut header, "compression", "compression", &[0]);

    // dataWindow: box2i (xMin, yMin, xMax, yMax)
    let mut data_window = Vec::with_capacity(16);
    data_window.extend_from_slice(&0i32.to_le_bytes());
    data_window.extend_from_slice(&0i32.to_le_bytes());
    data_window.extend_from_slice(&((width as i32) - 1).to_le_bytes());
    data_window.extend_from_slice(&((height as i32) - 1).to_le_bytes());
    write_attribute(&mut header, "dataWindow", "box2i", &data_window);

    // displayWindow: box2i
    write_attribute(&mut header, "displayWindow", "box2i", &data_window);

    // lineOrder: 0 = INCREASING_Y
    write_attribute(&mut header, "lineOrder", "lineOrder", &[0]);

    // pixelAspectRatio: float 1.0
    write_attribute(&mut header, "pixelAspectRatio", "float", &1.0f32.to_le_bytes());

    // screenWindowCenter: v2f (0.0, 0.0)
    let mut swc = Vec::with_capacity(8);
    swc.extend_from_slice(&0.0f32.to_le_bytes());
    swc.extend_from_slice(&0.0f32.to_le_bytes());
    write_attribute(&mut header, "screenWindowCenter", "v2f", &swc);

    // screenWindowWidth: float 1.0
    write_attribute(&mut header, "screenWindowWidth", "float", &1.0f32.to_le_bytes());

    // Header terminator
    header.push(0);

    // 3. Scanline table (height * 8 bytes offset pointers)
    let scanline_table_pos = header.len();
    let scanline_table_len = (height as usize) * 8;
    let channel_bytes_per_row = (width as usize) * 2; // 2 bytes per half float
    let num_channels = if has_alpha { 4 } else { 3 };
    let row_payload_size = channel_bytes_per_row * num_channels;
    let scanline_block_size = 4 + 4 + row_payload_size; // y-coord (4), data size (4), channel data

    let first_scanline_pos = scanline_table_pos + scanline_table_len;

    let mut scanline_offsets = Vec::with_capacity(scanline_table_len);
    for y in 0..(height as usize) {
        let offset = (first_scanline_pos + y * scanline_block_size) as u64;
        scanline_offsets.extend_from_slice(&offset.to_le_bytes());
    }

    header.extend_from_slice(&scanline_offsets);

    // 4. Scanline data blocks
    let mut full_file = header;

    for y in 0..(height as usize) {
        // y-coordinate (i32)
        full_file.extend_from_slice(&(y as i32).to_le_bytes());
        // pixel data size (u32)
        full_file.extend_from_slice(&(row_payload_size as u32).to_le_bytes());

        let row_start = y * stride;

        // OpenEXR stores channel planar scanlines in alphabetical order: A (if present), B, G, R
        if has_alpha {
            for x in 0..(width as usize) {
                let a = plane_data.get(row_start + x * 4 + 3).copied().unwrap_or(255);
                let half_a = f32_to_half((a as f32) / 255.0);
                full_file.extend_from_slice(&half_a.to_le_bytes());
            }
        }

        // B channel
        for x in 0..(width as usize) {
            let offset = if has_alpha { row_start + x * 4 + 2 } else { row_start + x * 3 + 2 };
            let b = plane_data.get(offset).copied().unwrap_or(0);
            let half_b = f32_to_half((b as f32) / 255.0);
            full_file.extend_from_slice(&half_b.to_le_bytes());
        }

        // G channel
        for x in 0..(width as usize) {
            let offset = if has_alpha { row_start + x * 4 + 1 } else { row_start + x * 3 + 1 };
            let g = plane_data.get(offset).copied().unwrap_or(0);
            let half_g = f32_to_half((g as f32) / 255.0);
            full_file.extend_from_slice(&half_g.to_le_bytes());
        }

        // R channel
        for x in 0..(width as usize) {
            let offset = if has_alpha { row_start + x * 4 } else { row_start + x * 3 };
            let r = plane_data.get(offset).copied().unwrap_or(0);
            let half_r = f32_to_half((r as f32) / 255.0);
            full_file.extend_from_slice(&half_r.to_le_bytes());
        }
    }

    Ok(full_file)
}

fn write_attribute(header: &mut Vec<u8>, name: &str, attr_type: &str, data: &[u8]) {
    header.extend_from_slice(name.as_bytes());
    header.push(0);
    header.extend_from_slice(attr_type.as_bytes());
    header.push(0);
    header.extend_from_slice(&(data.len() as u32).to_le_bytes());
    header.extend_from_slice(data);
}

fn write_channel_spec(ch_data: &mut Vec<u8>, name: &[u8], pixel_type: i32) {
    ch_data.extend_from_slice(name);
    ch_data.push(0);
    ch_data.extend_from_slice(&pixel_type.to_le_bytes());
    ch_data.push(0); // pLinear
    ch_data.extend_from_slice(&[0, 0, 0]); // reserved
    ch_data.extend_from_slice(&1i32.to_le_bytes()); // xSampling
    ch_data.extend_from_slice(&1i32.to_le_bytes()); // ySampling
}

/// Converts an f32 float to IEEE 754-2008 16-bit half precision float bits.
fn f32_to_half(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = (bits >> 31) & 0x1;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x7FFFFF;

    if exp == 255 {
        // Inf / NaN
        let h_mant = if mant != 0 { 0x200 } else { 0 };
        return ((sign << 15) | 0x7C00 | h_mant) as u16;
    }

    let h_exp = exp - 127 + 15;
    if h_exp >= 31 {
        // Overflow to Inf
        return ((sign << 15) | 0x7C00) as u16;
    } else if h_exp <= 0 {
        // Subnormal or zero
        return (sign << 15) as u16;
    }

    let h_mant = (mant >> 13) & 0x3FF;
    ((sign << 15) | ((h_exp as u32) << 10) | h_mant) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_encode_exr_half() {
        use encodercraft_core::Rational64;
        let dims = Dimensions::new(2, 2).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let exr_bytes = encode_exr_half(&frame).unwrap();

        assert_eq!(&exr_bytes[0..4], &EXR_MAGIC);
        assert!(exr_bytes.len() > 100);
    }
}
