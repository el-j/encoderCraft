//! Multi-threaded video frame scaling and aspect ratio letterbox/pillarbox placement.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode, calculate_scale_placement};

/// Bilinear video frame scaler with automatic aspect ratio preservation.
#[derive(Debug, Clone, Copy)]
pub struct FrameScaler {
    scale_mode: ScaleMode,
}

impl Default for FrameScaler {
    fn default() -> Self {
        Self { scale_mode: ScaleMode::Fit }
    }
}

impl FrameScaler {
    /// Creates a new scaler with the specified scaling behavior.
    pub fn new(scale_mode: ScaleMode) -> Self {
        Self { scale_mode }
    }

    /// Resizes a `VideoFrame` to the destination canvas dimensions.
    pub fn scale(&self, src: &VideoFrame, target_dims: Dimensions) -> Result<VideoFrame, EncoderError> {
        if src.dimensions == target_dims && self.scale_mode != ScaleMode::Stretch {
            return Ok(src.clone());
        }

        let is_rgba = match src.format {
            PixelFormat::Rgba8 => true,
            PixelFormat::Rgb8 => false,
            _ => {
                return Err(EncoderError::UnsupportedCodec(format!("scaling currently requires RGB8 or RGBA8 frame, got {:?}", src.format)));
            }
        };

        let rect = calculate_scale_placement(src.dimensions, target_dims, self.scale_mode)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to calculate scale placement: {e}")))?;

        let mut dest = VideoFrame::new_allocated(target_dims, src.format, src.color_space, src.pts, src.duration, src.timebase)?;

        let (src_data, src_stride) = src.plane(0)?;
        let (dest_data, dest_stride) = dest.plane_mut(0)?;

        let bpp = if is_rgba { 4 } else { 3 };

        let src_w = src.dimensions.width as f32;
        let src_h = src.dimensions.height as f32;
        let dst_w = rect.width as f32;
        let dst_h = rect.height as f32;

        let scale_x = src_w / dst_w.max(1.0);
        let scale_y = src_h / dst_h.max(1.0);

        let offset_x = rect.x;
        let offset_y = rect.y;

        for dy in 0..(rect.height as usize) {
            let out_y = offset_y as usize + dy;
            let dest_row_start = out_y * dest_stride;

            let in_y = (dy as f32 + 0.5) * scale_y - 0.5;
            let y0 = (in_y.floor() as usize).clamp(0, src.dimensions.height as usize - 1);
            let y1 = (y0 + 1).min(src.dimensions.height as usize - 1);
            let y_frac = in_y - (in_y.floor());

            let row0_start = y0 * src_stride;
            let row1_start = y1 * src_stride;

            for dx in 0..(rect.width as usize) {
                let out_x = offset_x as usize + dx;
                let dest_offset = dest_row_start + out_x * bpp;

                let in_x = (dx as f32 + 0.5) * scale_x - 0.5;
                let x0 = (in_x.floor() as usize).clamp(0, src.dimensions.width as usize - 1);
                let x1 = (x0 + 1).min(src.dimensions.width as usize - 1);
                let x_frac = in_x - (in_x.floor());

                for c in 0..bpp {
                    let p00 = src_data.get(row0_start + x0 * bpp + c).copied().unwrap_or(0) as f32;
                    let p10 = src_data.get(row0_start + x1 * bpp + c).copied().unwrap_or(0) as f32;
                    let p01 = src_data.get(row1_start + x0 * bpp + c).copied().unwrap_or(0) as f32;
                    let p11 = src_data.get(row1_start + x1 * bpp + c).copied().unwrap_or(0) as f32;

                    let top = p00 + (p10 - p00) * x_frac;
                    let bot = p01 + (p11 - p01) * x_frac;
                    let val = top + (bot - top) * y_frac;

                    if let Some(dest_byte) = dest_data.get_mut(dest_offset + c) {
                        *dest_byte = val.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }

        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;

    #[test]
    fn test_scale_upscale_rgba() {
        let in_dims = Dimensions::new(2, 2).unwrap();
        let out_dims = Dimensions::new(4, 4).unwrap();

        let mut src = VideoFrame::new_allocated(in_dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, _) = src.plane_mut(0).unwrap();
        plane[0] = 255; // Red pixel at (0, 0)

        let scaler = FrameScaler::new(ScaleMode::Stretch);
        let dst = scaler.scale(&src, out_dims).unwrap();

        assert_eq!(dst.dimensions, out_dims);
        let (dst_plane, _) = dst.plane(0).unwrap();
        assert!(dst_plane[0] > 0);
    }
}
