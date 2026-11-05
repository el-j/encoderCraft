//! Multi-threaded video frame scaling and aspect ratio letterbox/pillarbox placement.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, ScalingFilter, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode, calculate_scale_placement};

/// Bilinear video frame scaler with automatic aspect ratio preservation.
#[derive(Debug, Clone, Copy)]
pub struct FrameScaler {
    scale_mode: ScaleMode,
    filter: ScalingFilter,
}

impl Default for FrameScaler {
    fn default() -> Self {
        Self { scale_mode: ScaleMode::Fit, filter: ScalingFilter::Bilinear }
    }
}

impl FrameScaler {
    /// Creates a new scaler with the specified scaling behavior.
    pub fn new(scale_mode: ScaleMode) -> Self {
        Self { scale_mode, filter: ScalingFilter::Bilinear }
    }

    /// Creates a scaler with explicit geometry and reconstruction filter.
    pub fn with_filter(scale_mode: ScaleMode, filter: ScalingFilter) -> Self {
        Self { scale_mode, filter }
    }

    /// Resizes a `VideoFrame` to the destination canvas dimensions.
    pub fn scale(&self, src: &VideoFrame, target_dims: Dimensions) -> Result<VideoFrame, EncoderError> {
        if src.dimensions == target_dims {
            return Ok(src.clone());
        }
        let mut dest = VideoFrame::new_allocated(target_dims, src.format, src.color_space, src.pts, src.duration, src.timebase)?;
        self.scale_into(src, &mut dest)?;
        Ok(dest)
    }

    /// Resizes `src` into the existing frame `dest`, which defines the canvas size. `dest` must be
    /// cleared by the caller when it is reused, since letterbox bars are not redrawn.
    pub fn scale_into(&self, src: &VideoFrame, dest: &mut VideoFrame) -> Result<(), EncoderError> {
        let target_dims = dest.dimensions;
        if src.format != dest.format {
            return Err(EncoderError::Internal("scale_into: source and destination formats differ".into()));
        }
        dest.pts = src.pts;
        dest.duration = src.duration;
        dest.timebase = src.timebase;
        dest.color_space = src.color_space;
        dest.keyframe = src.keyframe;
        if src.dimensions == target_dims {
            if src.data().len() != dest.data().len() {
                return Err(EncoderError::Internal("scale_into: buffer sizes differ".into()));
            }
            dest.data_mut().copy_from_slice(src.data());
            return Ok(());
        }

        let (channels, bytes_per_component) = match src.format {
            PixelFormat::Rgb8 => (3, 1),
            PixelFormat::Rgba8 => (4, 1),
            PixelFormat::RgbaF32 => (4, 4),
            _ => {
                return Err(EncoderError::UnsupportedCodec(format!("scaling requires RGB8, RGBA8, or RGBAF32 frames, got {:?}", src.format)));
            }
        };

        let rect = calculate_scale_placement(src.dimensions, target_dims, self.scale_mode)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to calculate scale placement: {e}")))?;

        let (src_data, src_stride) = src.plane(0)?;
        let (dest_data, dest_stride) = dest.plane_mut(0)?;

        let bpp = channels * bytes_per_component;

        let src_w = src.dimensions.width as f32;
        let src_h = src.dimensions.height as f32;
        let fill = self.scale_mode == ScaleMode::Fill;
        let center = self.scale_mode == ScaleMode::Center;
        let draw_width = if fill { target_dims.width } else { rect.width };
        let draw_height = if fill { target_dims.height } else { rect.height };
        let scale_x = if center { 1.0 } else { src_w / rect.width.max(1) as f32 };
        let scale_y = if center { 1.0 } else { src_h / rect.height.max(1) as f32 };
        let source_x = if center {
            (src_w - draw_width as f32).max(0.0) / 2.0
        } else if fill {
            rect.width.saturating_sub(target_dims.width) as f32 * scale_x / 2.0
        } else {
            0.0
        };
        let source_y = if center {
            (src_h - draw_height as f32).max(0.0) / 2.0
        } else if fill {
            rect.height.saturating_sub(target_dims.height) as f32 * scale_y / 2.0
        } else {
            0.0
        };

        let offset_x = rect.x;
        let offset_y = rect.y;

        let x_weights = (0..draw_width as usize)
            .map(|x| axis_weights(source_x + (x as f32 + 0.5) * scale_x - 0.5, scale_x, src.dimensions.width as usize, self.filter))
            .collect::<Vec<_>>();
        let y_weights = (0..draw_height as usize)
            .map(|y| axis_weights(source_y + (y as f32 + 0.5) * scale_y - 0.5, scale_y, src.dimensions.height as usize, self.filter))
            .collect::<Vec<_>>();

        for (dy, ys) in y_weights.iter().enumerate() {
            for (dx, xs) in x_weights.iter().enumerate() {
                for channel in 0..channels {
                    let mut value = 0.0;
                    for &(sy, wy) in ys {
                        for &(sx, wx) in xs {
                            let offset = sy
                                .checked_mul(src_stride)
                                .and_then(|row| sx.checked_mul(bpp).and_then(|column| row.checked_add(column)))
                                .and_then(|pixel| pixel.checked_add(channel * bytes_per_component))
                                .ok_or_else(|| EncoderError::Internal("scale_into: source pixel offset overflow".into()))?;
                            let sample = if bytes_per_component == 4 {
                                let bytes = src_data
                                    .get(offset..offset + 4)
                                    .ok_or_else(|| EncoderError::Internal("scale_into: source float component out of range".into()))?;
                                f32::from_ne_bytes(
                                    bytes.try_into().map_err(|_| EncoderError::Internal("scale_into: source float component is invalid".into()))?,
                                )
                            } else {
                                f32::from(
                                    *src_data.get(offset).ok_or_else(|| EncoderError::Internal("scale_into: source pixel out of range".into()))?,
                                )
                            };
                            value += sample * wx * wy;
                        }
                    }
                    let offset = (offset_y as usize + dy)
                        .checked_mul(dest_stride)
                        .and_then(|row| (offset_x as usize + dx).checked_mul(bpp).and_then(|column| row.checked_add(column)))
                        .and_then(|pixel| pixel.checked_add(channel * bytes_per_component))
                        .ok_or_else(|| EncoderError::Internal("scale_into: destination pixel offset overflow".into()))?;
                    if bytes_per_component == 4 {
                        let dest = dest_data
                            .get_mut(offset..offset + 4)
                            .ok_or_else(|| EncoderError::Internal("scale_into: destination float component out of range".into()))?;
                        dest.copy_from_slice(&value.to_ne_bytes());
                    } else {
                        *dest_data.get_mut(offset).ok_or_else(|| EncoderError::Internal("scale_into: destination pixel out of range".into()))? =
                            value.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }

        Ok(())
    }
}

fn axis_weights(center: f32, scale: f32, source_len: usize, filter: ScalingFilter) -> Vec<(usize, f32)> {
    if source_len == 0 {
        return Vec::new();
    }
    if filter == ScalingFilter::Bilinear {
        let left = center.floor() as i64;
        let fraction = center - left as f32;
        return [(left, 1.0 - fraction), (left + 1, fraction)]
            .into_iter()
            .map(|(index, weight)| (index.clamp(0, source_len.saturating_sub(1) as i64) as usize, weight))
            .collect();
    }
    let reduction = scale.max(1.0);
    let radius = match filter {
        ScalingFilter::Bicubic => 2.0,
        ScalingFilter::Lanczos3 => 3.0,
        _ => 1.0,
    } * reduction;
    let first = (center - radius).ceil() as i64;
    let last = (center + radius).floor() as i64;
    let mut weights = Vec::new();
    let mut sum = 0.0;
    for source_index in first..=last {
        if source_index < 0 || source_index >= source_len as i64 {
            continue;
        }
        let weight = kernel((source_index as f32 - center) / reduction, filter) / reduction;
        if weight != 0.0 {
            weights.push((source_index as usize, weight));
            sum += weight;
        }
    }
    if !sum.is_finite() || sum.abs() < f32::EPSILON {
        return vec![(center.round().clamp(0.0, source_len.saturating_sub(1) as f32) as usize, 1.0)];
    }
    for (_, weight) in &mut weights {
        *weight /= sum;
    }
    weights
}

fn kernel(distance: f32, filter: ScalingFilter) -> f32 {
    let x = distance.abs();
    match filter {
        ScalingFilter::Bilinear => (1.0 - x).max(0.0),
        ScalingFilter::Bicubic => {
            if x < 1.0 {
                (1.5 * x - 2.5) * x * x + 1.0
            } else if x < 2.0 {
                ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
            } else {
                0.0
            }
        }
        ScalingFilter::Lanczos3 => {
            if x == 0.0 {
                1.0
            } else if x < 3.0 {
                let pix = std::f32::consts::PI * x;
                (pix.sin() / pix) * (pix / 3.0).sin() / (pix / 3.0)
            } else {
                0.0
            }
        }
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

    #[test]
    fn fill_crops_the_center_of_the_scaled_source() {
        let src_dims = Dimensions::new(4, 2).unwrap();
        let dst_dims = Dimensions::new(2, 2).unwrap();
        let mut src = VideoFrame::new_allocated(src_dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, stride) = src.plane_mut(0).unwrap();
        for (x, value) in [10, 50, 100, 200].into_iter().enumerate() {
            for y in 0..2 {
                plane[y * stride + x * 3..y * stride + x * 3 + 3].fill(value);
            }
        }

        let output = FrameScaler::new(ScaleMode::Fill).scale(&src, dst_dims).unwrap();
        let (pixels, _) = output.plane(0).unwrap();
        assert_eq!(&pixels[..3], &[50, 50, 50]);
        assert_eq!(&pixels[3..6], &[100, 100, 100]);
    }

    #[test]
    fn center_preserves_one_to_one_pixels_and_crops_symmetrically() {
        let src_dims = Dimensions::new(4, 2).unwrap();
        let dst_dims = Dimensions::new(2, 2).unwrap();
        let mut src = VideoFrame::new_allocated(src_dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (plane, stride) = src.plane_mut(0).unwrap();
        for (x, value) in [10, 50, 100, 200].into_iter().enumerate() {
            for y in 0..2 {
                plane[y * stride + x * 3..y * stride + x * 3 + 3].fill(value);
            }
        }

        let output = FrameScaler::new(ScaleMode::Center).scale(&src, dst_dims).unwrap();
        let (pixels, _) = output.plane(0).unwrap();
        assert_eq!(&pixels[..3], &[50, 50, 50]);
        assert_eq!(&pixels[3..6], &[100, 100, 100]);
    }

    #[test]
    fn higher_quality_filters_preserve_constant_frames() {
        let src_dims = Dimensions::new(4, 3).unwrap();
        let dst_dims = Dimensions::new(7, 5).unwrap();
        let mut src = VideoFrame::new_allocated(src_dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        src.data_mut().as_chunks_mut::<4>().0.iter_mut().for_each(|pixel| pixel.copy_from_slice(&[80, 120, 160, 255]));

        for filter in [ScalingFilter::Bicubic, ScalingFilter::Lanczos3] {
            let output = FrameScaler::with_filter(ScaleMode::Stretch, filter).scale(&src, dst_dims).unwrap();
            let (pixels, remainder) = output.data().as_chunks::<4>();
            assert!(remainder.is_empty() && pixels.iter().all(|pixel| *pixel == [80, 120, 160, 255]));
        }
    }

    #[test]
    fn bicubic_and_lanczos_filters_reconstruct_a_sharp_edge() {
        let dims = Dimensions::new(2, 1).unwrap();
        let mut src = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (pixels, _) = src.plane_mut(0).unwrap();
        pixels[..3].fill(0);
        pixels[3..].fill(255);
        let output_dims = Dimensions::new(5, 1).unwrap();

        let bilinear = FrameScaler::new(ScaleMode::Stretch).scale(&src, output_dims).unwrap();
        let bicubic = FrameScaler::with_filter(ScaleMode::Stretch, ScalingFilter::Bicubic).scale(&src, output_dims).unwrap();
        let lanczos = FrameScaler::with_filter(ScaleMode::Stretch, ScalingFilter::Lanczos3).scale(&src, output_dims).unwrap();

        assert_ne!(bilinear.data(), bicubic.data());
        assert_ne!(bicubic.data(), lanczos.data());
        assert_eq!(&bicubic.data()[..3], &[0, 0, 0]);
        assert_eq!(&lanczos.data()[12..15], &[255, 255, 255]);
    }
}
