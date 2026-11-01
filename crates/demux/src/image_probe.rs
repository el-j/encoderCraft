//! Lightweight, non-decoding header probers for still image formats.
//!
//! Inspects only the first few kilobytes to extract dimensions and bit depth without pixel decoding.

use encodercraft_color::PixelFormat;
use encodercraft_core::EncoderError;
use encodercraft_geom::Dimensions;

/// Basic dimension and format information extracted from an image header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageProbeInfo {
    pub dimensions: Dimensions,
    pub format: PixelFormat,
    pub bit_depth: u8,
}

/// Probes a PNG header for dimensions and color layout.
pub fn probe_png(bytes: &[u8]) -> Result<ImageProbeInfo, EncoderError> {
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(EncoderError::InvalidContainer("not a valid PNG stream".into()));
    }

    // IHDR chunk starts at byte 12 (4B length, 4B "IHDR", 4B width, 4B height, 1B bit depth, 1B color type)
    if bytes.get(12..16) != Some(b"IHDR") {
        return Err(EncoderError::InvalidContainer("PNG missing IHDR chunk".into()));
    }

    let width = u32::from_be_bytes([
        bytes.get(16).copied().unwrap_or(0),
        bytes.get(17).copied().unwrap_or(0),
        bytes.get(18).copied().unwrap_or(0),
        bytes.get(19).copied().unwrap_or(0),
    ]);
    let height = u32::from_be_bytes([
        bytes.get(20).copied().unwrap_or(0),
        bytes.get(21).copied().unwrap_or(0),
        bytes.get(22).copied().unwrap_or(0),
        bytes.get(23).copied().unwrap_or(0),
    ]);

    let bit_depth = bytes.get(24).copied().unwrap_or(8);
    let color_type = bytes.get(25).copied().unwrap_or(2);

    let format = match (color_type, bit_depth) {
        (0, 8) => PixelFormat::Gray8,
        (0, 16) => PixelFormat::Gray16,
        (2, 8) => PixelFormat::Rgb8,
        (6, 8) => PixelFormat::Rgba8,
        (6, 16) => PixelFormat::Rgba16,
        _ => PixelFormat::Rgba8,
    };

    let dimensions = Dimensions::new(width, height)?;
    Ok(ImageProbeInfo { dimensions, format, bit_depth })
}

/// Probes a JPEG stream for SOF0/SOF2 markers to determine image dimensions.
pub fn probe_jpeg(bytes: &[u8]) -> Result<ImageProbeInfo, EncoderError> {
    if bytes.len() < 4 || !bytes.starts_with(b"\xFF\xD8\xFF") {
        return Err(EncoderError::InvalidContainer("not a valid JPEG stream".into()));
    }

    let mut cursor: usize = 2;
    while cursor.checked_add(4).is_some_and(|end| end <= bytes.len()) {
        if bytes.get(cursor).copied() != Some(0xFF) {
            cursor += 1;
            continue;
        }

        let marker = bytes.get(cursor + 1).copied().unwrap_or(0);
        // SOF0 (Baseline DCT), SOF1 (Extended Sequential), SOF2 (Progressive DCT)
        if marker == 0xC0 || marker == 0xC1 || marker == 0xC2 {
            let length = u16::from_be_bytes([bytes.get(cursor + 2).copied().unwrap_or(0), bytes.get(cursor + 3).copied().unwrap_or(0)]) as usize;

            if length >= 6 && cursor + 4 + length <= bytes.len() {
                let precision = bytes.get(cursor + 4).copied().unwrap_or(8);
                let height = u16::from_be_bytes([bytes.get(cursor + 5).copied().unwrap_or(0), bytes.get(cursor + 6).copied().unwrap_or(0)]) as u32;
                let width = u16::from_be_bytes([bytes.get(cursor + 7).copied().unwrap_or(0), bytes.get(cursor + 8).copied().unwrap_or(0)]) as u32;
                let components = bytes.get(cursor + 9).copied().unwrap_or(3);

                let format = if components == 1 { PixelFormat::Gray8 } else { PixelFormat::Rgb8 };

                let dimensions = Dimensions::new(width, height)?;
                return Ok(ImageProbeInfo { dimensions, format, bit_depth: precision });
            }
        }

        // Advance to next marker
        let chunk_len = u16::from_be_bytes([bytes.get(cursor + 2).copied().unwrap_or(0), bytes.get(cursor + 3).copied().unwrap_or(0)]) as usize;

        let next = cursor.checked_add(2).and_then(|c| c.checked_add(chunk_len));
        let Some(n) = next else { break };
        cursor = n;
    }

    Err(EncoderError::InvalidContainer("no SOF marker found in JPEG stream".into()))
}

/// Probes a WebP header for dimensions.
pub fn probe_webp(bytes: &[u8]) -> Result<ImageProbeInfo, EncoderError> {
    if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return Err(EncoderError::InvalidContainer("not a valid WebP container".into()));
    }

    let chunk_tag = bytes.get(12..16).ok_or_else(|| EncoderError::InvalidContainer("error reading WebP chunk tag".into()))?;

    if chunk_tag == b"VP8 " && bytes.len() >= 30 {
        // Lossy VP8
        // Keyframe bit is at byte 23; start code 0x9D 0x01 0x2A at bytes 26..29
        let width = u16::from_le_bytes([bytes.get(26).copied().unwrap_or(0), bytes.get(27).copied().unwrap_or(0) & 0x3F]) as u32;
        let height = u16::from_le_bytes([bytes.get(28).copied().unwrap_or(0), bytes.get(29).copied().unwrap_or(0) & 0x3F]) as u32;

        let dimensions = Dimensions::new(width, height)?;
        return Ok(ImageProbeInfo { dimensions, format: PixelFormat::Rgb8, bit_depth: 8 });
    } else if chunk_tag == b"VP8L" && bytes.len() >= 25 {
        // Lossless VP8L
        // Signature 0x2F at byte 20; 14-bit width and 14-bit height
        let b0 = bytes.get(21).copied().unwrap_or(0) as u32;
        let b1 = bytes.get(22).copied().unwrap_or(0) as u32;
        let b2 = bytes.get(23).copied().unwrap_or(0) as u32;
        let b3 = bytes.get(24).copied().unwrap_or(0) as u32;

        let width = 1 + (b0 | ((b1 & 0x3F) << 8));
        let height = 1 + ((b1 >> 6) | (b2 << 2) | ((b3 & 0xF) << 10));

        let dimensions = Dimensions::new(width, height)?;
        return Ok(ImageProbeInfo { dimensions, format: PixelFormat::Rgba8, bit_depth: 8 });
    }

    Err(EncoderError::InvalidContainer("unsupported WebP chunk type".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_png() {
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&13u32.to_be_bytes()); // IHDR length
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.push(8); // 8-bit
        png.push(6); // RGBA
        png.push(0); // compression
        png.push(0); // filter
        png.push(0); // interlace

        let info = probe_png(&png).unwrap();
        assert_eq!(info.dimensions.width, 1920);
        assert_eq!(info.dimensions.height, 1080);
        assert_eq!(info.format, PixelFormat::Rgba8);
    }
}
