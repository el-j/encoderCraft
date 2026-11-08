//! 3D Lookup Table (LUT) parser and tetrahedral / trilinear interpolation engine.
//!
//! Supports industry-standard `.cube`, `.3dl`, and `.look` files,
//! providing high-speed color grading, film emulation, and camera log-to-Rec.709 transforms.
//! Strictly zero-panic with safe bounded indexing and saturating float arithmetic.

use encodercraft_color::PixelFormat;
pub use encodercraft_color::lut::{InterpolationMode, Lut1D, Lut3D, LutError};
use encodercraft_core::{EncoderError, VideoFrame};

/// Extension trait for applying 3D LUT transformations directly to `VideoFrame` instances.
pub trait LutVideoFrameExt {
    /// Applies 3D LUT color transform using default tetrahedral interpolation.
    fn apply_to_frame(&self, frame: &mut VideoFrame) -> Result<(), EncoderError>;

    /// Applies 3D LUT color transform using specified interpolation mode.
    fn apply_to_frame_with_mode(&self, frame: &mut VideoFrame, mode: InterpolationMode) -> Result<(), EncoderError>;
}

impl LutVideoFrameExt for Lut3D {
    fn apply_to_frame(&self, frame: &mut VideoFrame) -> Result<(), EncoderError> {
        self.apply_to_frame_with_mode(frame, InterpolationMode::Tetrahedral)
    }

    fn apply_to_frame_with_mode(&self, frame: &mut VideoFrame, mode: InterpolationMode) -> Result<(), EncoderError> {
        match frame.format {
            PixelFormat::Rgb8 => {
                let data = frame.data_mut();
                for chunk in data.as_chunks_mut::<3>().0 {
                    let (r, g, b) = self.evaluate_u8(chunk[0], chunk[1], chunk[2], mode);
                    chunk[0] = r;
                    chunk[1] = g;
                    chunk[2] = b;
                }
                Ok(())
            }
            PixelFormat::Rgba8 => {
                let data = frame.data_mut();
                for chunk in data.as_chunks_mut::<4>().0 {
                    let (r, g, b) = self.evaluate_u8(chunk[0], chunk[1], chunk[2], mode);
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
                    let [r, g, b] = self.evaluate(values[0], values[1], values[2], mode);
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

        let out = lut.evaluate(0.5, 0.5, 0.5, InterpolationMode::Tetrahedral);
        assert!((out[0] - 0.5).abs() < 1e-4);
        assert!((out[1] - 0.5).abs() < 1e-4);
        assert!((out[2] - 0.5).abs() < 1e-4);

        let (r, g, b) = lut.evaluate_u8(128, 128, 128, InterpolationMode::Tetrahedral);
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

        frame.data_mut().fill(0);

        lut.apply_to_frame(&mut frame).unwrap();
        let data = frame.data();
        assert_eq!(data[0], 255);
        assert_eq!(data[1], 255);
        assert_eq!(data[2], 255);
    }
}
