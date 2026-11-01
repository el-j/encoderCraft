//! Foundational geometry, dimensions, and aspect ratio math for media frames.
//!
//! Enforces safe arithmetic without division by zero or integer overflow.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Geometry and dimension calculation errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GeomError {
    #[error("dimensions must be non-zero (got {0}x{1})")]
    ZeroDimension(u32, u32),

    #[error("arithmetic overflow during scaling calculation")]
    Overflow,

    #[error("crop rectangle out of bounds (frame: {frame_w}x{frame_h}, crop: {crop_x}+{crop_w} x {crop_y}+{crop_h})")]
    CropOutOfBounds { frame_w: u32, frame_h: u32, crop_x: u32, crop_y: u32, crop_w: u32, crop_h: u32 },
}

/// 2D media dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

impl Dimensions {
    /// Creates a new `Dimensions` instance, verifying that both dimensions are non-zero.
    pub fn new(width: u32, height: u32) -> Result<Self, GeomError> {
        if width == 0 || height == 0 {
            return Err(GeomError::ZeroDimension(width, height));
        }
        Ok(Self { width, height })
    }

    /// Total number of pixels, using checked arithmetic.
    pub fn pixel_count(&self) -> Result<u64, GeomError> {
        (self.width as u64).checked_mul(self.height as u64).ok_or(GeomError::Overflow)
    }

    /// Display aspect ratio as a floating-point ratio (width / height).
    pub fn display_aspect_ratio(&self) -> f64 {
        if self.height == 0 {
            return 1.0;
        }
        self.width as f64 / self.height as f64
    }

    /// Whether the dimensions represent landscape orientation (width >= height).
    pub fn is_landscape(&self) -> bool {
        self.width >= self.height
    }

    /// Returns standard 16-pixel aligned dimensions for video codecs (macroblock alignment).
    pub fn macroblock_aligned(&self) -> Self {
        let align = |dim: u32| (dim.saturating_add(15) / 16).saturating_mul(16);
        Self { width: align(self.width), height: align(self.height) }
    }
}

/// Pixel aspect ratio (PAR) for non-square pixel formats.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelAspectRatio {
    pub num: u32,
    pub den: u32,
}

impl PixelAspectRatio {
    /// Standard square pixels (1:1).
    pub const SQUARE: Self = Self { num: 1, den: 1 };
    /// NTSC standard definition 4:3 (10:11).
    pub const NTSC_4_3: Self = Self { num: 10, den: 11 };
    /// PAL standard definition 4:3 (59:54).
    pub const PAL_4_3: Self = Self { num: 59, den: 54 };
    /// HDV / DVCPRO HD anamorphic 16:9 (4:3).
    pub const ANAMORPHIC_4_3: Self = Self { num: 4, den: 3 };

    pub fn new(num: u32, den: u32) -> Result<Self, GeomError> {
        if num == 0 || den == 0 {
            return Err(GeomError::ZeroDimension(num, den));
        }
        Ok(Self { num, den })
    }

    pub fn as_f64(&self) -> f64 {
        if self.den == 0 {
            return 1.0;
        }
        self.num as f64 / self.den as f64
    }
}

impl Default for PixelAspectRatio {
    fn default() -> Self {
        Self::SQUARE
    }
}

/// Mode used to scale a source image or video frame into destination dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScaleMode {
    /// Fit within destination bounds, maintaining aspect ratio (adds letterbox or pillarbox padding).
    #[default]
    Fit,
    /// Fill the destination bounds entirely, maintaining aspect ratio (crops excess source content).
    Fill,
    /// Stretch non-uniformly to exactly match target width and height.
    Stretch,
    /// Center 1:1 without scaling; pad with black if destination is larger, or crop if smaller.
    Center,
}

/// 2D Rectangle defining an offset and size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, GeomError> {
        if width == 0 || height == 0 {
            return Err(GeomError::ZeroDimension(width, height));
        }
        Ok(Self { x, y, width, height })
    }

    /// Verifies that this rectangle lies entirely inside frame bounds.
    pub fn validate_within(&self, frame: Dimensions) -> Result<(), GeomError> {
        let max_x = self.x.saturating_add(self.width);
        let max_y = self.y.saturating_add(self.height);
        if max_x > frame.width || max_y > frame.height {
            return Err(GeomError::CropOutOfBounds {
                frame_w: frame.width,
                frame_h: frame.height,
                crop_x: self.x,
                crop_y: self.y,
                crop_w: self.width,
                crop_h: self.height,
            });
        }
        Ok(())
    }
}

/// Calculates the target placement rectangle when scaling `src` into `dst` under `mode`.
pub fn calculate_scale_placement(src: Dimensions, dst: Dimensions, mode: ScaleMode) -> Result<Rect, GeomError> {
    match mode {
        ScaleMode::Stretch => Rect::new(0, 0, dst.width, dst.height),
        ScaleMode::Center => {
            let placed_w = src.width.min(dst.width);
            let placed_h = src.height.min(dst.height);
            let offset_x = (dst.width.saturating_sub(placed_w)) / 2;
            let offset_y = (dst.height.saturating_sub(placed_h)) / 2;
            Rect::new(offset_x, offset_y, placed_w, placed_h)
        }
        ScaleMode::Fit => {
            let src_dar = src.display_aspect_ratio();
            let dst_dar = dst.display_aspect_ratio();

            if src_dar > dst_dar {
                // Source is wider than destination: letterbox top and bottom
                let scaled_w = dst.width;
                let scaled_h = ((scaled_w as f64) / src_dar).round() as u32;
                let scaled_h = scaled_h.clamp(1, dst.height);
                let offset_y = (dst.height.saturating_sub(scaled_h)) / 2;
                Rect::new(0, offset_y, scaled_w, scaled_h)
            } else {
                // Source is taller than destination: pillarbox left and right
                let scaled_h = dst.height;
                let scaled_w = ((scaled_h as f64) * src_dar).round() as u32;
                let scaled_w = scaled_w.clamp(1, dst.width);
                let offset_x = (dst.width.saturating_sub(scaled_w)) / 2;
                Rect::new(offset_x, 0, scaled_w, scaled_h)
            }
        }
        ScaleMode::Fill => {
            let src_dar = src.display_aspect_ratio();
            let dst_dar = dst.display_aspect_ratio();

            if src_dar > dst_dar {
                // Fill vertically, crop horizontal sides
                let scaled_h = dst.height;
                let scaled_w = ((scaled_h as f64) * src_dar).round() as u32;
                Rect::new(0, 0, scaled_w.max(dst.width), scaled_h)
            } else {
                // Fill horizontally, crop vertical sides
                let scaled_w = dst.width;
                let scaled_h = ((scaled_w as f64) / src_dar).round() as u32;
                Rect::new(0, 0, scaled_w, scaled_h.max(dst.height))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dimensions_creation() {
        assert!(Dimensions::new(0, 100).is_err());
        assert!(Dimensions::new(100, 0).is_err());
        let dim = Dimensions::new(1920, 1080).unwrap();
        assert_eq!(dim.pixel_count().unwrap(), 2_073_600);
        assert!(dim.is_landscape());
    }

    #[test]
    fn test_macroblock_alignment() {
        let dim = Dimensions::new(1915, 1075).unwrap();
        let aligned = dim.macroblock_aligned();
        assert_eq!(aligned.width, 1920);
        assert_eq!(aligned.height, 1088);
    }

    #[test]
    fn test_scale_fit_letterbox() {
        let src = Dimensions::new(1920, 800).unwrap(); // 2.40:1 CinemaScope
        let dst = Dimensions::new(1920, 1080).unwrap(); // 16:9 1080p
        let rect = calculate_scale_placement(src, dst, ScaleMode::Fit).unwrap();
        assert_eq!(rect.width, 1920);
        assert_eq!(rect.height, 800);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 140);
    }

    #[test]
    fn test_scale_fit_pillarbox() {
        let src = Dimensions::new(1440, 1080).unwrap(); // 4:3 SD / HDV
        let dst = Dimensions::new(1920, 1080).unwrap(); // 16:9
        let rect = calculate_scale_placement(src, dst, ScaleMode::Fit).unwrap();
        assert_eq!(rect.height, 1080);
        assert_eq!(rect.width, 1440);
        assert_eq!(rect.x, 240);
        assert_eq!(rect.y, 0);
    }

    #[test]
    fn test_rect_validation() {
        let frame = Dimensions::new(1920, 1080).unwrap();
        let valid_crop = Rect::new(100, 100, 500, 500).unwrap();
        assert!(valid_crop.validate_within(frame).is_ok());

        let invalid_crop = Rect::new(1800, 100, 200, 200).unwrap();
        assert!(invalid_crop.validate_within(frame).is_err());
    }
}
