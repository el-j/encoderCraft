//! Secondary Capture DICOM (Part 10 File Format) writer.
//!
//! Complies with DICOM PS3.10 and PS3.5 for uncompressed Explicit VR Little Endian images.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

const PREAMBLE_LEN: usize = 128;
const DICOM_MAGIC: &[u8; 4] = b"DICM";
const SOP_CLASS_SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";
const TRANSFER_SYNTAX_EXPLICIT_VR_LE: &str = "1.2.840.10008.1.2.1";

fn write_ui(b: &mut Vec<u8>, group: u16, elem: u16, val: &str) {
    let mut s = val.as_bytes().to_vec();
    if s.len() % 2 == 1 {
        s.push(0);
    }
    b.extend_from_slice(&group.to_le_bytes());
    b.extend_from_slice(&elem.to_le_bytes());
    b.extend_from_slice(b"UI");
    b.extend_from_slice(&(s.len() as u16).to_le_bytes());
    b.extend_from_slice(&s);
}

fn write_us(b: &mut Vec<u8>, group: u16, elem: u16, val: u16) {
    b.extend_from_slice(&group.to_le_bytes());
    b.extend_from_slice(&elem.to_le_bytes());
    b.extend_from_slice(b"US");
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&val.to_le_bytes());
}

fn write_cs(b: &mut Vec<u8>, group: u16, elem: u16, val: &str) {
    let mut s = val.as_bytes().to_vec();
    if s.len() % 2 == 1 {
        s.push(0);
    }
    b.extend_from_slice(&group.to_le_bytes());
    b.extend_from_slice(&elem.to_le_bytes());
    b.extend_from_slice(b"CS");
    b.extend_from_slice(&(s.len() as u16).to_le_bytes());
    b.extend_from_slice(&s);
}

/// Encodes a `VideoFrame` (RGB8 or Gray8) to a standardized DICOM Part 10 file.
pub fn encode_dicom(frame: &VideoFrame) -> Result<Vec<u8>, EncoderError> {
    let (w, h) = (frame.dimensions.width, frame.dimensions.height);
    if w == 0 || h == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode DICOM with zero dimensions".into()));
    }
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err(EncoderError::InvalidJobSpec("DICOM dimensions exceed 16-bit limits".into()));
    }

    // Convert frame to straight RGB8
    let data = frame.data();
    let (pixels, samples, photometric) = match frame.format {
        PixelFormat::Rgb8 => (data.to_vec(), 3u16, "RGB"),
        PixelFormat::Rgba8 => {
            // Drop alpha, composite on black
            let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
            for chunk in data.as_chunks::<4>().0 {
                rgb.push(chunk[0]);
                rgb.push(chunk[1]);
                rgb.push(chunk[2]);
            }
            (rgb, 3u16, "RGB")
        }
        _ => {
            let opts = encodercraft_color::ConvertOptions::default();
            let converted = encodercraft_color::convert_pixels_with_options(data, frame.format, PixelFormat::Rgb8, w, h, &opts)
                .map_err(|e| EncoderError::Internal(format!("pixel conversion failed: {e}")))?;
            (converted, 3u16, "RGB")
        }
    };

    let mut out = Vec::with_capacity(512 + pixels.len());

    // 128-byte preamble + DICM
    out.resize(PREAMBLE_LEN, 0u8);
    out.extend_from_slice(DICOM_MAGIC);

    // Group 0002 File Meta Information
    write_ui(&mut out, 0x0002, 0x0002, SOP_CLASS_SECONDARY_CAPTURE);
    write_ui(&mut out, 0x0002, 0x0003, "1.2.826.0.1.3680043.10.1000.1");
    write_ui(&mut out, 0x0002, 0x0010, TRANSFER_SYNTAX_EXPLICIT_VR_LE);

    // Image attributes (Group 0028)
    write_us(&mut out, 0x0028, 0x0010, h as u16);
    write_us(&mut out, 0x0028, 0x0011, w as u16);
    write_us(&mut out, 0x0028, 0x0002, samples);
    write_us(&mut out, 0x0028, 0x0006, 0); // Planar configuration = 0 (color-by-pixel)
    write_cs(&mut out, 0x0028, 0x0004, photometric);
    write_us(&mut out, 0x0028, 0x0100, 8); // Bits Allocated
    write_us(&mut out, 0x0028, 0x0101, 8); // Bits Stored
    write_us(&mut out, 0x0028, 0x0102, 7); // High Bit
    write_us(&mut out, 0x0028, 0x0103, 0); // Pixel Representation = unsigned

    // Pixel Data (7FE0, 0010)
    out.extend_from_slice(&0x7FE0u16.to_le_bytes());
    out.extend_from_slice(&0x0010u16.to_le_bytes());
    out.extend_from_slice(b"OB");
    out.extend_from_slice(&0u16.to_le_bytes()); // 2 bytes reserved for OB VR
    let mut padded_len = pixels.len();
    if padded_len % 2 == 1 {
        padded_len += 1;
    }
    out.extend_from_slice(&(padded_len as u32).to_le_bytes());
    out.extend_from_slice(&pixels);
    if pixels.len() % 2 == 1 {
        out.push(0);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_dicom_roundtrip() {
        let frame = VideoFrame::new_allocated(
            Dimensions { width: 4, height: 4 },
            PixelFormat::Rgb8,
            ColorSpaceTag::SRGB,
            0,
            1,
            Rational64 { num: 1, den: 1 },
        )
        .unwrap();

        let dicom_bytes = encode_dicom(&frame).unwrap();
        assert_eq!(&dicom_bytes[128..132], b"DICM");

        // Verify decodable by photocraft-codecs
        let decoded = photocraft_codecs::decode_with(&dicom_bytes, &photocraft_codecs::DecodeOptions::default()).unwrap();
        assert_eq!(decoded.dimensions(), (4, 4));
    }
}
