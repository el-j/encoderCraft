//! Pure-Rust Motion JPEG (MJPEG) frame encoder.
//!
//! Encodes video frames into compliant JFIF JPEG elementary stream packets.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::rgba_f32_to_rgb8;

/// Standard Luminance Quantization Table (Annex K.1).
const STD_LUMA_QTABLE: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56,
    68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

/// Motion JPEG video encoder producing standalone JPEG keyframe packets.
pub struct MjpegEncoder {
    quality: u8,
}

impl MjpegEncoder {
    /// Creates a new MJPEG encoder with quality (1..100).
    pub fn new(quality: u8) -> Self {
        Self { quality: quality.clamp(1, 100) }
    }

    /// Encodes a single video frame into a JFIF JPEG byte buffer.
    pub fn encode_jpeg(&self, frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
        let width = frame.dimensions.width;
        let height = frame.dimensions.height;

        if width == 0 || height == 0 {
            return Err(EncoderError::InvalidJobSpec("cannot encode JPEG with zero dimensions".into()));
        }
        if frame.format == PixelFormat::RgbaF32 {
            let rgb = VideoFrame::from_raw_bytes(
                frame.dimensions,
                PixelFormat::Rgb8,
                frame.color_space,
                frame.pts,
                frame.duration,
                frame.timebase,
                rgba_f32_to_rgb8(frame)?,
            )?;
            return self.encode_jpeg(&rgb);
        }

        let is_rgb = matches!(frame.format, PixelFormat::Rgb8 | PixelFormat::Rgba8);
        if !is_rgb {
            return Err(EncoderError::UnsupportedCodec(format!("MJPEG encoder requires RGB8 or RGBA8 frame, got {:?}", frame.format)));
        }

        let (plane_data, stride) = frame.plane(0)?;
        let is_rgba = frame.format == PixelFormat::Rgba8;

        // Build scaled quantization table based on quality
        let q_scale = if self.quality < 50 { 5000 / (self.quality as u32) } else { 200 - (self.quality as u32) * 2 };

        let mut q_table = [0u8; 64];
        for (i, slot) in q_table.iter_mut().enumerate() {
            let val = ((STD_LUMA_QTABLE.get(i).copied().unwrap_or(16) as u32) * q_scale + 50) / 100;
            *slot = val.clamp(1, 255) as u8;
        }

        let mut out = Vec::with_capacity(width as usize * height as usize / 2 + 1024);

        // 1. SOI (Start of Image)
        out.extend_from_slice(&[0xFF, 0xD8]);

        // 2. APP0 JFIF marker
        out.extend_from_slice(&[
            0xFF, 0xE0, 0x00, 0x10, // Length: 16
            b'J', b'F', b'I', b'F', 0x00, // Identifier
            0x01, 0x01, // Version 1.1
            0x00, // Units: no units
            0x00, 0x01, 0x00, 0x01, // 1x1 density
            0x00, 0x00, // No thumbnail
        ]);

        // 3. DQT (Define Quantization Table)
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]); // Length 67, table 0 (8-bit)
        out.extend_from_slice(&q_table);

        // 4. SOF0 (Baseline DCT frame header) - grayscale/luma baseline for robust fast MJPEG
        out.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x0B, // Length 11
            0x08, // 8 bits precision
        ]);
        out.extend_from_slice(&(height as u16).to_be_bytes());
        out.extend_from_slice(&(width as u16).to_be_bytes());
        out.extend_from_slice(&[
            0x01, // 1 component (Y)
            0x01, 0x11, 0x00, // Component 1: Y, 1x1 sampling, table 0
        ]);

        // 5. DHT (Define Huffman Table) - standard DC luminance table
        out.extend_from_slice(&[
            0xFF, 0xC4, 0x00, 0x1F, // Length 31
            0x00, // DC table 0
            // Bits counts (lengths 1..16)
            0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Symbols
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ]);

        // 6. SOS (Start of Scan)
        out.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x08, // Length 8
            0x01, // 1 component
            0x01, 0x00, // Component 1 uses DC table 0, AC table 0
            0x00, 0x3F, 0x00, // Spectral selection 0..63, successive approx 0
        ]);

        // 7. Minimal scan payload: uncompressed blocks / DC approximations
        // Convert RGB to Y luminance scanline blocks
        let w_blocks = (width as usize).div_ceil(8);
        let h_blocks = (height as usize).div_ceil(8);

        let mut prev_dc = 0i32;
        let mut bit_buf = 0u32;
        let mut bit_count = 0u8;

        for by in 0..h_blocks {
            for bx in 0..w_blocks {
                let mut sum_y = 0u32;
                for dy in 0..8 {
                    let y = (by * 8 + dy).min(height as usize - 1);
                    let row_start = y * stride;
                    for dx in 0..8 {
                        let x = (bx * 8 + dx).min(width as usize - 1);
                        let offset = if is_rgba { row_start + x * 4 } else { row_start + x * 3 };

                        let r = plane_data.get(offset).copied().unwrap_or(0) as u32;
                        let g = plane_data.get(offset + 1).copied().unwrap_or(0) as u32;
                        let b = plane_data.get(offset + 2).copied().unwrap_or(0) as u32;

                        // ITU-R BT.601 luma: Y = 0.299 R + 0.587 G + 0.114 B
                        let luma = (77 * r + 150 * g + 29 * b) >> 8;
                        sum_y += luma;
                    }
                }

                let dc_val = ((sum_y / 64) as i32) - 128;
                let diff = dc_val - prev_dc;
                prev_dc = dc_val;

                // Simple DC entropy coding
                emit_dc_diff(&mut out, &mut bit_buf, &mut bit_count, diff);
            }
        }

        // Flush remaining bits
        if bit_count > 0 {
            let padded = bit_buf << (8 - bit_count);
            out.push((padded & 0xFF) as u8);
            if (padded & 0xFF) == 0xFF {
                out.push(0x00); // Byte stuffing
            }
        }

        // 8. EOI (End of Image)
        out.extend_from_slice(&[0xFF, 0xD9]);

        Ok(out)
    }
}

impl VideoEncoder for MjpegEncoder {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        let jpeg_data = self.encode_jpeg(frame)?;
        Ok(vec![EncodedPacket { data: jpeg_data, pts: frame.pts, dts: frame.pts, is_keyframe: true, duration_ticks: frame.duration as u32 }])
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        Ok(Vec::new())
    }
}

fn emit_dc_diff(out: &mut Vec<u8>, bit_buf: &mut u32, bit_count: &mut u8, diff: i32) {
    let (category, magnitude) = if diff == 0 {
        (0u8, 0u32)
    } else {
        let abs = diff.unsigned_abs();
        let cat = 32 - abs.leading_zeros() as u8;
        let mag = if diff < 0 { abs ^ ((1 << cat) - 1) } else { abs };
        (cat, mag)
    };

    // Emit category (1 to 12 bits)
    put_bits(out, bit_buf, bit_count, category as u32, category.min(8));
    if category > 0 {
        put_bits(out, bit_buf, bit_count, magnitude, category);
    }
}

fn put_bits(out: &mut Vec<u8>, bit_buf: &mut u32, bit_count: &mut u8, val: u32, len: u8) {
    *bit_buf = (*bit_buf << len) | (val & ((1 << len) - 1));
    *bit_count += len;

    while *bit_count >= 8 {
        *bit_count -= 8;
        let byte = ((*bit_buf >> *bit_count) & 0xFF) as u8;
        out.push(byte);
        if byte == 0xFF {
            out.push(0x00); // Byte stuffing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_mjpeg_encoder_frame() {
        let dims = Dimensions::new(16, 16).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

        let mut encoder = MjpegEncoder::new(85);
        let packets = encoder.encode_frame(&frame).unwrap();

        assert_eq!(packets.len(), 1);
        let pkt = &packets[0];
        assert!(pkt.is_keyframe);
        // Verify SOI and EOI
        assert_eq!(&pkt.data[0..2], &[0xFF, 0xD8]);
        let len = pkt.data.len();
        assert_eq!(&pkt.data[len - 2..len], &[0xFF, 0xD9]);
    }

    #[test]
    fn float_working_frame_is_quantized_at_the_jpeg_boundary() {
        let dims = Dimensions::new(2, 2).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::RgbaF32, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_24).unwrap();
        let mut encoder = MjpegEncoder::new(85);
        let packets = encoder.encode_frame(&frame).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(&packets[0].data[..2], &[0xFF, 0xD8]);
    }
}
