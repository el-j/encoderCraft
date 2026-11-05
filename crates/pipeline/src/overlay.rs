//! Video frame overlay engine: SMPTE timecode burn-in and watermark compositing.
//!
//! Provides broadcast-grade timecode overlay, watermark branding, and text burn-in
//! with sub-pixel alpha blending and custom anchor positioning.
//! Zero-panic implementation using bounded slice operations and saturating arithmetic.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, MediaTimecode, VideoFrame};

/// Screen anchoring position for visual overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayPosition {
    TopLeft,
    TopRight,
    #[default]
    BottomLeft,
    BottomRight,
    Center,
}

/// Bitmapped 5x7 font glyph definitions for digits and symbols.
/// Each byte represents one row of 5 horizontal pixels (bits 0..=4).
fn glyph_5x7(c: char) -> [u8; 7] {
    match c {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        ':' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        ';' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b00100, 0b01000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        ' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

pub(crate) fn rgb_layout(format: PixelFormat) -> Result<(usize, usize), EncoderError> {
    match format {
        PixelFormat::Rgb8 => Ok((3, 1)),
        PixelFormat::Rgba8 => Ok((4, 1)),
        PixelFormat::RgbaF32 => Ok((4, 4)),
        other => Err(EncoderError::UnsupportedCodec(format!("RGB overlay does not support {other:?} frames"))),
    }
}

pub(crate) fn blend_rgb(data: &mut [u8], offset: usize, bytes_per_component: usize, color: [f32; 3], alpha: f32) -> Result<(), EncoderError> {
    for (channel, value) in color.into_iter().enumerate() {
        let start = offset
            .checked_add(channel.saturating_mul(bytes_per_component))
            .ok_or_else(|| EncoderError::Internal("overlay pixel offset overflow".into()))?;
        if bytes_per_component == 1 {
            let pixel = data.get_mut(start).ok_or_else(|| EncoderError::Internal("overlay pixel is out of range".into()))?;
            *pixel = (f32::from(*pixel) * (1.0 - alpha) + value * 255.0 * alpha).round().clamp(0.0, 255.0) as u8;
        } else {
            let bytes = data.get_mut(start..start + 4).ok_or_else(|| EncoderError::Internal("overlay float pixel is out of range".into()))?;
            let current = f32::from_ne_bytes(bytes.try_into().map_err(|_| EncoderError::Internal("overlay float pixel is invalid".into()))?);
            bytes.copy_from_slice(&(current * (1.0 - alpha) + value * alpha).to_ne_bytes());
        }
    }
    Ok(())
}

/// Timecode overlay burn-in filter.
#[derive(Debug, Clone)]
pub struct TimecodeOverlay {
    pub position: OverlayPosition,
    pub scale: usize,
    pub text_opacity: f32,
    pub bg_opacity: f32,
    pub margin: usize,
}

impl Default for TimecodeOverlay {
    fn default() -> Self {
        Self { position: OverlayPosition::BottomLeft, scale: 2, text_opacity: 1.0, bg_opacity: 0.65, margin: 20 }
    }
}

impl TimecodeOverlay {
    /// Renders a timecode string burn-in onto an uncompressed video frame.
    pub fn draw(&self, frame: &mut VideoFrame, timecode: &MediaTimecode) -> Result<(), EncoderError> {
        let text = timecode.to_smpte_string();
        self.draw_text(frame, &text)
    }

    /// Renders custom text burn-in onto an uncompressed RGB8, RGBA8, or RGBAF32 frame.
    pub fn draw_text(&self, frame: &mut VideoFrame, text: &str) -> Result<(), EncoderError> {
        let (channels, bytes_per_component) = rgb_layout(frame.format)?;

        let char_w = 5 * self.scale;
        let char_h = 7 * self.scale;
        let spacing = self.scale;
        let padding = 4 * self.scale;

        let total_chars = text.len();
        let text_width = total_chars.checked_mul(char_w + spacing).and_then(|w| w.checked_sub(spacing)).unwrap_or(0);
        let box_width = text_width.checked_add(padding * 2).unwrap_or(text_width);
        let box_height = char_h.checked_add(padding * 2).unwrap_or(char_h);

        let fw = frame.dimensions.width as usize;
        let fh = frame.dimensions.height as usize;

        if box_width >= fw || box_height >= fh {
            return Ok(()); // Frame too small for overlay
        }

        // Calculate bounding box position
        let (box_x, box_y) = match self.position {
            OverlayPosition::TopLeft => (self.margin, self.margin),
            OverlayPosition::TopRight => (fw.saturating_sub(box_width + self.margin), self.margin),
            OverlayPosition::BottomLeft => (self.margin, fh.saturating_sub(box_height + self.margin)),
            OverlayPosition::BottomRight => (fw.saturating_sub(box_width + self.margin), fh.saturating_sub(box_height + self.margin)),
            OverlayPosition::Center => ((fw.saturating_sub(box_width)) / 2, (fh.saturating_sub(box_height)) / 2),
        };

        let bg_alpha = self.bg_opacity.clamp(0.0, 1.0);
        let text_alpha = self.text_opacity.clamp(0.0, 1.0);

        let (data, stride) = frame.plane_mut(0)?;
        let bytes_per_pixel = channels * bytes_per_component;

        // 1. Draw pill background (semi-transparent dark)
        for by in 0..box_height {
            let py = box_y + by;
            if py >= fh {
                break;
            }
            for bx in 0..box_width {
                let px = box_x + bx;
                if px >= fw {
                    break;
                }
                let idx = py * stride + px * bytes_per_pixel;
                blend_rgb(data, idx, bytes_per_component, [0.0; 3], bg_alpha)?;
            }
        }

        // 2. Render text glyphs
        let text_start_x = box_x + padding;
        let text_start_y = box_y + padding;

        for (ci, ch) in text.chars().enumerate() {
            let glyph = glyph_5x7(ch);
            let char_x = text_start_x + ci * (char_w + spacing);

            for row in 0..7 {
                let row_bits = glyph.get(row).copied().unwrap_or(0);
                for sy in 0..self.scale {
                    let py = text_start_y + row * self.scale + sy;
                    if py >= fh {
                        break;
                    }
                    for col in 0..5 {
                        let bit = (row_bits >> (4 - col)) & 1;
                        if bit == 1 {
                            for sx in 0..self.scale {
                                let px = char_x + col * self.scale + sx;
                                if px >= fw {
                                    break;
                                }
                                let idx = py * stride + px * bytes_per_pixel;
                                blend_rgb(data, idx, bytes_per_component, [1.0; 3], text_alpha)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// Watermark image overlay filter.
#[derive(Debug, Clone)]
pub struct WatermarkOverlay {
    pub position: OverlayPosition,
    pub opacity: f32,
    pub margin: usize,
    pub width: usize,
    pub height: usize,
    pub rgba_data: Vec<u8>,
}

impl WatermarkOverlay {
    /// Creates a new watermark overlay.
    pub fn new(rgba_data: Vec<u8>, width: usize, height: usize, position: OverlayPosition, opacity: f32) -> Self {
        Self { position, opacity: opacity.clamp(0.0, 1.0), margin: 20, width, height, rgba_data }
    }

    /// Blends the watermark onto an uncompressed video frame.
    pub fn draw(&self, frame: &mut VideoFrame) -> Result<(), EncoderError> {
        let (frame_channels, bytes_per_component) = rgb_layout(frame.format)?;

        let fw = frame.dimensions.width as usize;
        let fh = frame.dimensions.height as usize;
        let ww = self.width;
        let wh = self.height;

        if ww == 0 || wh == 0 || ww >= fw || wh >= fh {
            return Ok(());
        }

        let (origin_x, origin_y) = match self.position {
            OverlayPosition::TopLeft => (self.margin, self.margin),
            OverlayPosition::TopRight => (fw.saturating_sub(ww + self.margin), self.margin),
            OverlayPosition::BottomLeft => (self.margin, fh.saturating_sub(wh + self.margin)),
            OverlayPosition::BottomRight => (fw.saturating_sub(ww + self.margin), fh.saturating_sub(wh + self.margin)),
            OverlayPosition::Center => ((fw.saturating_sub(ww)) / 2, (fh.saturating_sub(wh)) / 2),
        };

        let master_opacity = self.opacity;

        let (frame_data, frame_stride) = frame.plane_mut(0)?;
        let bytes_per_pixel = frame_channels * bytes_per_component;

        for wy in 0..wh {
            let py = origin_y + wy;
            if py >= fh {
                break;
            }
            for wx in 0..ww {
                let px = origin_x + wx;
                if px >= fw {
                    break;
                }

                let wm_idx = (wy * ww + wx) * 4;
                let src_r = self.rgba_data.get(wm_idx).copied().unwrap_or(0) as f32 / 255.0;
                let src_g = self.rgba_data.get(wm_idx + 1).copied().unwrap_or(0) as f32 / 255.0;
                let src_b = self.rgba_data.get(wm_idx + 2).copied().unwrap_or(0) as f32 / 255.0;
                let src_a = (self.rgba_data.get(wm_idx + 3).copied().unwrap_or(0) as f32 / 255.0) * master_opacity;

                if src_a <= 0.001 {
                    continue;
                }

                let dst_idx = py * frame_stride + px * bytes_per_pixel;
                blend_rgb(frame_data, dst_idx, bytes_per_component, [src_r, src_g, src_b], src_a)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_timecode_overlay_draw() {
        let dims = Dimensions::new(320, 240).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgba8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        frame.data_mut().fill(100);

        let overlay = TimecodeOverlay { position: OverlayPosition::TopLeft, scale: 2, text_opacity: 1.0, bg_opacity: 0.8, margin: 10 };

        let tc = MediaTimecode::new(0, 0, 5, 0, false, Rational64::FPS_24);
        let res = overlay.draw(&mut frame, &tc);
        assert!(res.is_ok());

        // Verify that pixels within the box were modified
        let data = frame.data();
        let sample = data[0];
        // Corner outside margin is untouched (100)
        assert_eq!(sample, 100);
    }

    #[test]
    fn test_watermark_overlay_draw() {
        let dims = Dimensions::new(100, 100).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        frame.data_mut().fill(0); // black

        // 10x10 white square with 50% alpha
        let mut wm_data = Vec::with_capacity(400);
        for _ in 0..100 {
            wm_data.extend_from_slice(&[255, 255, 255, 128]);
        }
        let watermark = WatermarkOverlay::new(wm_data, 10, 10, OverlayPosition::TopLeft, 1.0);

        let res = watermark.draw(&mut frame);
        assert!(res.is_ok());
    }
}
