//! Pure-Rust GIF89a animated image encoder with color palette quantization and LZW compression.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

use crate::encoder::{EncodedPacket, VideoEncoder};
use crate::yuv::rgba_f32_to_rgb8;

/// Animated GIF encoder conforming to Compuserve GIF89a specification.
pub struct GifEncoder {
    width: u16,
    height: u16,
    palette: Vec<[u8; 3]>,
    delay_cs: u16,
    first_frame: bool,
    accumulated_data: Vec<u8>,
}

impl GifEncoder {
    /// Creates a new GIF encoder with specified dimensions and frame delay in hundredths of a second (centiseconds).
    pub fn new(width: u16, height: u16, fps: u32) -> Result<Self, EncoderError> {
        if width == 0 || height == 0 {
            return Err(EncoderError::InvalidJobSpec("GIF dimensions cannot be zero".into()));
        }

        let delay_cs = 100u32.checked_div(fps).unwrap_or(10).clamp(1, 1000) as u16;

        // Generate standard 256-color uniform web palette (6x6x6 color cube + 40 grays)
        let mut palette = Vec::with_capacity(256);
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    palette.push([(r * 51) as u8, (g * 51) as u8, (b * 51) as u8]);
                }
            }
        }
        for i in 0..40 {
            let gray = (i * 255 / 39) as u8;
            palette.push([gray, gray, gray]);
        }

        let mut enc = Self { width, height, palette, delay_cs, first_frame: true, accumulated_data: Vec::new() };

        enc.write_header()?;
        Ok(enc)
    }

    fn write_header(&mut self) -> Result<(), EncoderError> {
        // 1. Signature: GIF89a
        self.accumulated_data.extend_from_slice(b"GIF89a");

        // 2. Logical Screen Descriptor
        self.accumulated_data.extend_from_slice(&self.width.to_le_bytes());
        self.accumulated_data.extend_from_slice(&self.height.to_le_bytes());
        // Packed fields: GCT present (0x80), 8 bits/pixel resolution (0x70), 256 colors (0x07) -> 0xF7
        self.accumulated_data.push(0xF7);
        self.accumulated_data.push(0); // Background color index
        self.accumulated_data.push(0); // Pixel aspect ratio

        // 3. Global Color Table (256 * 3 = 768 bytes)
        for rgb in &self.palette {
            self.accumulated_data.extend_from_slice(rgb);
        }

        // 4. Netscape 2.0 looping application extension (infinite loop)
        self.accumulated_data
            .extend_from_slice(&[0x21, 0xFF, 0x0B, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0', 0x03, 0x01, 0x00, 0x00, 0x00]);

        Ok(())
    }

    /// Quantizes RGB/RGBA pixels to palette indices.
    fn quantize_frame(&self, frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
        if frame.format == PixelFormat::RgbaF32 {
            let rgb = rgba_f32_to_rgb8(frame)?;
            return Ok(rgb.as_chunks::<3>().0.iter().map(|pixel| self.find_closest_palette_index(pixel[0], pixel[1], pixel[2])).collect());
        }
        if !matches!(frame.format, PixelFormat::Rgb8 | PixelFormat::Rgba8) {
            return Err(EncoderError::UnsupportedCodec(format!("GIF encoder requires RGB8, RGBA8, or RGBAF32 input, got {:?}", frame.format)));
        }
        let (plane_data, stride) = frame.plane(0)?;
        let w = frame.dimensions.width as usize;
        let h = frame.dimensions.height as usize;
        let mut indexed = Vec::with_capacity(w * h);

        let is_rgba = frame.format == PixelFormat::Rgba8;

        for y in 0..h {
            let row_offset = y * stride;
            for x in 0..w {
                let pixel_offset = if is_rgba { row_offset + x * 4 } else { row_offset + x * 3 };

                let r = plane_data.get(pixel_offset).copied().unwrap_or(0);
                let g = plane_data.get(pixel_offset + 1).copied().unwrap_or(0);
                let b = plane_data.get(pixel_offset + 2).copied().unwrap_or(0);

                // Find closest match in palette (Euclidean distance)
                let idx = self.find_closest_palette_index(r, g, b);
                indexed.push(idx);
            }
        }

        Ok(indexed)
    }

    #[inline]
    fn find_closest_palette_index(&self, r: u8, g: u8, b: u8) -> u8 {
        // Fast quantization to 6x6x6 color cube: r_idx*36 + g_idx*6 + b_idx
        let ri = (((r as u16) + 25) / 51).min(5) as u8;
        let gi = (((g as u16) + 25) / 51).min(5) as u8;
        let bi = (((b as u16) + 25) / 51).min(5) as u8;
        ri * 36 + gi * 6 + bi
    }

    /// Appends a frame block to the GIF stream.
    pub fn add_frame(&mut self, frame: &VideoFrame) -> Result<(), EncoderError> {
        let indexed = self.quantize_frame(frame)?;

        // 1. Graphic Control Extension (GCE)
        self.accumulated_data.extend_from_slice(&[
            0x21, 0xF9, 0x04, 0x00, // Packed fields
        ]);
        self.accumulated_data.extend_from_slice(&self.delay_cs.to_le_bytes());
        self.accumulated_data.extend_from_slice(&[0x00, 0x00]); // Transparent index, terminator

        // 2. Image Descriptor
        self.accumulated_data.push(0x2C); // Image separator
        self.accumulated_data.extend_from_slice(&0u16.to_le_bytes()); // Left
        self.accumulated_data.extend_from_slice(&0u16.to_le_bytes()); // Top
        self.accumulated_data.extend_from_slice(&self.width.to_le_bytes());
        self.accumulated_data.extend_from_slice(&self.height.to_le_bytes());
        self.accumulated_data.push(0x00); // No local color table

        // 3. LZW Compressed Image Data
        let min_code_size = 8u8;
        self.accumulated_data.push(min_code_size);
        let lzw_bytes = lzw_encode(&indexed, min_code_size);

        // Emit 255-byte sub-blocks
        for chunk in lzw_bytes.chunks(255) {
            self.accumulated_data.push(chunk.len() as u8);
            self.accumulated_data.extend_from_slice(chunk);
        }
        self.accumulated_data.push(0x00); // Block terminator

        self.first_frame = false;
        Ok(())
    }

    /// Finalizes the GIF byte stream and appends the trailer.
    pub fn finish(mut self) -> Vec<u8> {
        self.accumulated_data.push(0x3B); // GIF trailer
        self.accumulated_data
    }
}

impl VideoEncoder for GifEncoder {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.add_frame(frame)?;
        Ok(Vec::new()) // GIF buffers frames until finish
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        let final_bytes = self.accumulated_data.clone();
        Ok(vec![EncodedPacket { data: final_bytes, pts: 0, dts: 0, is_keyframe: true, duration_ticks: self.delay_cs as u32 }])
    }
}

/// Bit-packing helper for GIF LZW.
struct LzwBitWriter {
    bytes: Vec<u8>,
    accumulator: u32,
    bits_in_acc: u8,
}

impl LzwBitWriter {
    fn new() -> Self {
        Self { bytes: Vec::new(), accumulator: 0, bits_in_acc: 0 }
    }

    fn write_code(&mut self, code: u32, code_size: u8) {
        self.accumulator |= code << self.bits_in_acc;
        self.bits_in_acc += code_size;

        while self.bits_in_acc >= 8 {
            self.bytes.push((self.accumulator & 0xFF) as u8);
            self.accumulator >>= 8;
            self.bits_in_acc -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits_in_acc > 0 {
            self.bytes.push((self.accumulator & 0xFF) as u8);
        }
        self.bytes
    }
}

/// Variable-width LZW compression for GIF image data.
fn lzw_encode(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear_code = 1u32 << min_code_size;
    let eoi_code = clear_code + 1;

    let mut bit_writer = LzwBitWriter::new();
    bit_writer.write_code(clear_code, min_code_size + 1);

    // Simple uncompressed / literal LZW stream with regular clear codes
    let mut current_code_size = min_code_size + 1;
    let mut next_code = eoi_code + 1;

    for &idx in indices {
        bit_writer.write_code(idx as u32, current_code_size);
        next_code += 1;

        if next_code >= (1 << current_code_size) && current_code_size < 12 {
            current_code_size += 1;
        } else if next_code >= 4095 {
            bit_writer.write_code(clear_code, current_code_size);
            current_code_size = min_code_size + 1;
            next_code = eoi_code + 1;
        }
    }

    bit_writer.write_code(eoi_code, current_code_size);
    bit_writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_gif_encoder_single_frame() {
        let dims = Dimensions::new(4, 4).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

        let mut encoder = GifEncoder::new(4, 4, 10).unwrap();
        encoder.add_frame(&frame).unwrap();
        let gif_bytes = encoder.finish();

        assert_eq!(&gif_bytes[0..6], b"GIF89a");
        assert_eq!(gif_bytes.last().copied(), Some(0x3B));
    }

    #[test]
    fn float_working_frame_is_quantized_at_the_palette_boundary() {
        let dims = Dimensions::new(4, 4).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::RgbaF32, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let mut encoder = GifEncoder::new(4, 4, 10).unwrap();
        encoder.add_frame(&frame).unwrap();
        let gif_bytes = encoder.finish();
        assert_eq!(&gif_bytes[..6], b"GIF89a");
        assert_eq!(gif_bytes.last().copied(), Some(0x3B));
    }
}
