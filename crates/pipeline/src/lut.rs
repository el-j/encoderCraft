//! 3D Lookup Table (LUT) parser and trilinear interpolation engine.
//!
//! Supports industry-standard `.cube` files (Adobe / DaVinci Resolve specification),
//! providing high-speed color grading, film emulation, and camera log-to-Rec.709 transforms.
//! Strictly zero-panic with safe bounded indexing and saturating float arithmetic.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};

/// A 3D color lookup table for color grading and gamut transformations.
#[derive(Debug, Clone)]
pub struct Lut3D {
    pub title: String,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    data: Vec<[f32; 3]>,
}

impl Lut3D {
    /// Creates a 3D LUT directly from a raw table of RGB triplets.
    pub fn from_raw_table(size: usize, data: Vec<[f32; 3]>) -> Result<Self, EncoderError> {
        if !(2..=256).contains(&size) {
            return Err(EncoderError::InvalidJobSpec(format!("unsupported LUT size {size} (must be 2..=256)")));
        }
        let expected =
            size.checked_mul(size).and_then(|s| s.checked_mul(size)).ok_or_else(|| EncoderError::InvalidJobSpec("LUT size overflow".into()))?;
        if data.len() != expected {
            return Err(EncoderError::InvalidJobSpec(format!("data length mismatch: expected {expected}, got {}", data.len())));
        }
        Ok(Self { title: "Raw Table LUT".into(), size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Parses a 3D LUT from standard Adobe `.cube` file format string.
    pub fn parse_cube(cube_str: &str) -> Result<Self, EncoderError> {
        let mut title = "Untitled LUT".to_string();
        let mut size = 0usize;
        let mut domain_min = [0.0f32, 0.0, 0.0];
        let mut domain_max = [1.0f32, 1.0, 1.0];
        let mut data = Vec::new();

        for line in cube_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "TITLE" => {
                    if parts.len() > 1 {
                        title = parts[1..].join(" ").trim_matches('"').to_string();
                    }
                }
                "LUT_3D_SIZE" => {
                    if parts.len() > 1 {
                        size = parts[1].parse::<usize>().map_err(|e| EncoderError::InvalidJobSpec(format!("invalid LUT_3D_SIZE: {e}")))?;
                        if !(2..=256).contains(&size) {
                            return Err(EncoderError::InvalidJobSpec(format!("unsupported LUT size {size} (must be 2..=256)")));
                        }
                        data.reserve(size.checked_mul(size).and_then(|s| s.checked_mul(size)).unwrap_or(0));
                    }
                }
                "DOMAIN_MIN" => {
                    if parts.len() >= 4 {
                        domain_min[0] = parts[1].parse().unwrap_or(0.0);
                        domain_min[1] = parts[2].parse().unwrap_or(0.0);
                        domain_min[2] = parts[3].parse().unwrap_or(0.0);
                    }
                }
                "DOMAIN_MAX" => {
                    if parts.len() >= 4 {
                        domain_max[0] = parts[1].parse().unwrap_or(1.0);
                        domain_max[1] = parts[2].parse().unwrap_or(1.0);
                        domain_max[2] = parts[3].parse().unwrap_or(1.0);
                    }
                }
                _ => {
                    if parts.len() >= 3
                        && let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>())
                    {
                        data.push([r, g, b]);
                    }
                }
            }
        }

        let expected_entries = size.checked_mul(size).and_then(|s| s.checked_mul(size)).unwrap_or(0);
        if size == 0 || data.len() < expected_entries {
            return Err(EncoderError::InvalidJobSpec(format!("incomplete 3D LUT data: expected {expected_entries} entries, found {}", data.len())));
        }

        Ok(Self { title, size, domain_min, domain_max, data })
    }

    /// Evaluates the 3D LUT for a normalized input RGB triplet using trilinear interpolation.
    #[inline]
    pub fn evaluate(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let n = self.size as f32;
        let d_min = self.domain_min;
        let d_max = self.domain_max;

        // Normalize to [0, size - 1]
        let nr = ((r - d_min[0]) / (d_max[0] - d_min[0]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let ng = ((g - d_min[1]) / (d_max[1] - d_min[1]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let nb = ((b - d_min[2]) / (d_max[2] - d_min[2]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);

        let r0 = (nr.floor() as usize).min(self.size.saturating_sub(1));
        let g0 = (ng.floor() as usize).min(self.size.saturating_sub(1));
        let b0 = (nb.floor() as usize).min(self.size.saturating_sub(1));

        let r1 = (r0 + 1).min(self.size.saturating_sub(1));
        let g1 = (g0 + 1).min(self.size.saturating_sub(1));
        let b1 = (b0 + 1).min(self.size.saturating_sub(1));

        let dr = nr - (r0 as f32);
        let dg = ng - (g0 as f32);
        let db = nb - (b0 as f32);

        // Fetch 8 lattice corners
        // Cube index ordering: r + size * (g + size * b)
        let s = self.size;
        let get = |rx: usize, gx: usize, bx: usize| -> [f32; 3] {
            let idx = rx + s * (gx + s * bx);
            self.data.get(idx).copied().unwrap_or([0.0, 0.0, 0.0])
        };

        let c000 = get(r0, g0, b0);
        let c100 = get(r1, g0, b0);
        let c010 = get(r0, g1, b0);
        let c110 = get(r1, g1, b0);
        let c001 = get(r0, g0, b1);
        let c101 = get(r1, g0, b1);
        let c011 = get(r0, g1, b1);
        let c111 = get(r1, g1, b1);

        // Trilinear interpolation
        let mut out = [0.0f32; 3];
        for i in 0..3 {
            let c00 = c000[i] * (1.0 - dr) + c100[i] * dr;
            let c10 = c010[i] * (1.0 - dr) + c110[i] * dr;
            let c01 = c001[i] * (1.0 - dr) + c101[i] * dr;
            let c11 = c011[i] * (1.0 - dr) + c111[i] * dr;

            let c0 = c00 * (1.0 - dg) + c10 * dg;
            let c1 = c01 * (1.0 - dg) + c11 * dg;

            out[i] = c0 * (1.0 - db) + c1 * db;
        }

        out
    }

    /// Evaluates 8-bit integer RGB samples through the LUT.
    #[inline]
    pub fn evaluate_u8(&self, r: u8, g: u8, b: u8) -> (u8, u8, u8) {
        let fr = r as f32 / 255.0;
        let fg = g as f32 / 255.0;
        let fb = b as f32 / 255.0;

        let [or, og, ob] = self.evaluate(fr, fg, fb);
        ((or * 255.0 + 0.5).clamp(0.0, 255.0) as u8, (og * 255.0 + 0.5).clamp(0.0, 255.0) as u8, (ob * 255.0 + 0.5).clamp(0.0, 255.0) as u8)
    }

    /// Applies the 3D LUT in-place to an uncompressed RGB8 or RGBA8 video frame.
    pub fn apply_to_frame(&self, frame: &mut VideoFrame) -> Result<(), EncoderError> {
        match frame.format {
            PixelFormat::Rgb8 => {
                let data = frame.data_mut();
                for chunk in data.as_chunks_mut::<3>().0 {
                    let (r, g, b) = self.evaluate_u8(chunk[0], chunk[1], chunk[2]);
                    chunk[0] = r;
                    chunk[1] = g;
                    chunk[2] = b;
                }
                Ok(())
            }
            PixelFormat::Rgba8 => {
                let data = frame.data_mut();
                for chunk in data.as_chunks_mut::<4>().0 {
                    let (r, g, b) = self.evaluate_u8(chunk[0], chunk[1], chunk[2]);
                    chunk[0] = r;
                    chunk[1] = g;
                    chunk[2] = b;
                    // preserve alpha channel chunk[3]
                }
                Ok(())
            }
            PixelFormat::RgbaF32 => {
                let data = frame.data_mut();
                for pixel in data.as_chunks_mut::<16>().0 {
                    let channels = pixel.as_chunks_mut::<4>().0;
                    if channels.len() != 4 {
                        return Err(EncoderError::Internal("RGBAF32 LUT received an incomplete pixel".into()));
                    }
                    let values = [f32::from_ne_bytes(channels[0]), f32::from_ne_bytes(channels[1]), f32::from_ne_bytes(channels[2])];
                    let [r, g, b] = self.evaluate(values[0], values[1], values[2]);
                    for (channel, value) in channels.iter_mut().zip([r, g, b]) {
                        *channel = value.to_ne_bytes();
                    }
                }
                Ok(())
            }
            _ => Err(EncoderError::UnsupportedCodec(format!("3D LUT application not supported for pixel format {:?}", frame.format))),
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
    fn test_parse_cube_and_evaluate() {
        let cube_content = "\
TITLE \"Identity 2x2x2\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
        let lut = Lut3D::parse_cube(cube_content).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.title, "Identity 2x2x2");

        let out = lut.evaluate(0.5, 0.5, 0.5);
        assert!((out[0] - 0.5).abs() < 1e-4);
        assert!((out[1] - 0.5).abs() < 1e-4);
        assert!((out[2] - 0.5).abs() < 1e-4);

        let (r, g, b) = lut.evaluate_u8(128, 128, 128);
        assert_eq!(r, 128);
        assert_eq!(g, 128);
        assert_eq!(b, 128);
    }

    #[test]
    fn test_lut_apply_to_frame() {
        let cube_content = "\
TITLE \"Invert\"
LUT_3D_SIZE 2
1.0 1.0 1.0
0.0 1.0 1.0
1.0 0.0 1.0
0.0 0.0 1.0
1.0 1.0 0.0
0.0 1.0 0.0
1.0 0.0 0.0
0.0 0.0 0.0
";
        let lut = Lut3D::parse_cube(cube_content).unwrap();
        let dims = Dimensions::new(4, 4).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

        // Fill with black
        frame.data_mut().fill(0);

        lut.apply_to_frame(&mut frame).unwrap();
        // Should invert to white (255)
        let data = frame.data();
        assert_eq!(data[0], 255);
        assert_eq!(data[1], 255);
        assert_eq!(data[2], 255);
    }
}
