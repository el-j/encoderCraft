//! Color spaces, pixel format layouts, and video colorimetry standards.
//!
//! Enforces zero-panic arithmetic for plane sizes, strides, and color conversions.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use encodercraft_geom::Dimensions;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Color and pixel format calculation errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ColorError {
    #[error("arithmetic overflow calculating pixel buffer size or stride")]
    Overflow,

    #[error("unsupported conversion between {from:?} and {to:?}")]
    UnsupportedConversion { from: PixelFormat, to: PixelFormat },
}

/// Pixel format layout and component organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PixelFormat {
    /// 8-bit per channel Red, Green, Blue, Alpha (packed 32-bit).
    Rgba8,
    /// 8-bit per channel Blue, Green, Red, Alpha (packed 32-bit, Windows / Metal native).
    Bgra8,
    /// 8-bit per channel Red, Green, Blue (packed 24-bit).
    Rgb8,
    /// 16-bit unsigned integer per channel RGBA (packed 64-bit).
    Rgba16,
    /// 32-bit IEEE float per channel RGBA (packed 128-bit, HDR / compositing).
    RgbaF32,
    /// Planar YUV 4:2:0 8-bit (standard H.264 / WebM / JPEG).
    Yuv420p,
    /// Planar YUV 4:2:0 10-bit little-endian (HDR10, HEVC Main 10).
    Yuv420p10,
    /// Planar YUV 4:2:2 10-bit little-endian (Apple ProRes 422, broadcast MXF).
    Yuv422p10,
    /// Planar YUV 4:4:4 12-bit little-endian (ProRes 4444, master archival).
    Yuv444p12,
    /// Single channel 8-bit grayscale.
    Gray8,
    /// Single channel 16-bit grayscale.
    Gray16,
}

impl PixelFormat {
    /// Returns true if this format includes an alpha channel.
    pub fn has_alpha(&self) -> bool {
        matches!(self, Self::Rgba8 | Self::Bgra8 | Self::Rgba16 | Self::RgbaF32 | Self::Yuv444p12)
    }

    /// Returns true if this format is planar (Y, U, V in separate memory buffers).
    pub fn is_planar(&self) -> bool {
        matches!(self, Self::Yuv420p | Self::Yuv420p10 | Self::Yuv422p10 | Self::Yuv444p12)
    }

    /// Bit depth per color component.
    pub fn bits_per_component(&self) -> u8 {
        match self {
            Self::Rgba8 | Self::Bgra8 | Self::Rgb8 | Self::Yuv420p | Self::Gray8 => 8,
            Self::Yuv420p10 | Self::Yuv422p10 => 10,
            Self::Yuv444p12 => 12,
            Self::Rgba16 | Self::Gray16 => 16,
            Self::RgbaF32 => 32,
        }
    }

    /// Bytes required for packed line stride given an image width, aligned to `alignment` bytes.
    pub fn aligned_stride(&self, width: u32, alignment: u32) -> Result<usize, ColorError> {
        let align = alignment.max(1) as usize;
        let bytes_per_pixel: usize = match self {
            Self::Rgba8 | Self::Bgra8 => 4,
            Self::Rgb8 => 3,
            Self::Rgba16 => 8,
            Self::RgbaF32 => 16,
            Self::Gray8 => 1,
            Self::Gray16 => 2,
            Self::Yuv420p | Self::Yuv420p10 | Self::Yuv422p10 | Self::Yuv444p12 => {
                // For planar, this calculates the Y-plane stride
                let bpc = self.bits_per_component().div_ceil(8);
                bpc as usize
            }
        };

        let raw_stride = (width as usize).checked_mul(bytes_per_pixel).ok_or(ColorError::Overflow)?;

        let remainder = raw_stride % align;
        if remainder == 0 { Ok(raw_stride) } else { raw_stride.checked_add(align - remainder).ok_or(ColorError::Overflow) }
    }

    /// Total byte size required to hold an uncompressed frame of `dimensions`.
    pub fn required_buffer_size(&self, dimensions: Dimensions) -> Result<usize, ColorError> {
        let w = dimensions.width as usize;
        let h = dimensions.height as usize;
        let pixels = w.checked_mul(h).ok_or(ColorError::Overflow)?;

        match self {
            Self::Rgba8 | Self::Bgra8 => pixels.checked_mul(4).ok_or(ColorError::Overflow),
            Self::Rgb8 => pixels.checked_mul(3).ok_or(ColorError::Overflow),
            Self::Rgba16 => pixels.checked_mul(8).ok_or(ColorError::Overflow),
            Self::RgbaF32 => pixels.checked_mul(16).ok_or(ColorError::Overflow),
            Self::Gray8 => Ok(pixels),
            Self::Gray16 => pixels.checked_mul(2).ok_or(ColorError::Overflow),
            Self::Yuv420p => {
                // Y: w*h, U: (w/2)*(h/2), V: (w/2)*(h/2) -> 1.5 * pixels
                let half_w = w.div_ceil(2);
                let half_h = h.div_ceil(2);
                let uv_pixels = half_w.checked_mul(half_h).ok_or(ColorError::Overflow)?;
                let total_uv = uv_pixels.checked_mul(2).ok_or(ColorError::Overflow)?;
                pixels.checked_add(total_uv).ok_or(ColorError::Overflow)
            }
            Self::Yuv420p10 => {
                // 10-bit YUV 4:2:0 stored as 16-bit little-endian words
                let half_w = w.div_ceil(2);
                let half_h = h.div_ceil(2);
                let uv_pixels = half_w.checked_mul(half_h).ok_or(ColorError::Overflow)?;
                let total_uv = uv_pixels.checked_mul(2).ok_or(ColorError::Overflow)?;
                let total_samples = pixels.checked_add(total_uv).ok_or(ColorError::Overflow)?;
                total_samples.checked_mul(2).ok_or(ColorError::Overflow)
            }
            Self::Yuv422p10 => {
                // 10-bit YUV 4:2:2: Y: w*h, U: (w/2)*h, V: (w/2)*h -> 2 * pixels
                let half_w = w.div_ceil(2);
                let uv_pixels = half_w.checked_mul(h).ok_or(ColorError::Overflow)?;
                let total_uv = uv_pixels.checked_mul(2).ok_or(ColorError::Overflow)?;
                let total_samples = pixels.checked_add(total_uv).ok_or(ColorError::Overflow)?;
                total_samples.checked_mul(2).ok_or(ColorError::Overflow)
            }
            Self::Yuv444p12 => {
                // 12-bit YUV 4:4:4: Y: w*h, U: w*h, V: w*h -> 3 * pixels (16-bit words)
                let total_samples = pixels.checked_mul(3).ok_or(ColorError::Overflow)?;
                total_samples.checked_mul(2).ok_or(ColorError::Overflow)
            }
        }
    }
}

/// CIE 1931 chromaticity coordinates (x, y).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Chromaticity {
    pub x: f64,
    pub y: f64,
}

/// Color Primaries specification (ITU-T H.273 / ISO 23091-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorPrimaries {
    /// Rec. ITU-R BT.709-6 (HDTV, sRGB standard).
    #[default]
    Bt709 = 1,
    /// Unspecified.
    Unspecified = 2,
    /// Rec. ITU-R BT.2020 (Ultra HD, Wide Color Gamut).
    Bt2020 = 9,
    /// DCI-P3 (Theatrical Digital Cinema).
    DciP3 = 11,
    /// Display P3 (Apple Wide Color Gamut).
    DisplayP3 = 12,
}

/// Transfer Characteristics / Electro-Optical Transfer Function (EOTF).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransferFunction {
    /// Rec. ITU-R BT.709-6 standard gamma (~2.2).
    #[default]
    Bt709 = 1,
    /// Unspecified.
    Unspecified = 2,
    /// Linear optical transfer.
    Linear = 8,
    /// sRGB IEC 61966-2-1.
    Srgb = 13,
    /// SMPTE ST 2084 / PQ (HDR10, up to 10,000 nits).
    PerceptualQuantizer = 16,
    /// Hybrid Log-Gamma (HLG / ARIB STD-B67).
    HybridLogGamma = 18,
}

/// Matrix Coefficients for RGB to YUV derivation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MatrixCoefficients {
    /// Identity (RGB colorspace).
    Identity = 0,
    /// Rec. ITU-R BT.709-6.
    #[default]
    Bt709 = 1,
    /// Unspecified.
    Unspecified = 2,
    /// FCC Title 47.
    Fcc = 4,
    /// SMPTE 170M (NTSC).
    Smpte170M = 6,
    /// Rec. ITU-R BT.2020 Non-Constant Luminance.
    Bt2020NonConstant = 9,
}

/// Complete Color Space Tag specifying Primaries, Transfer, and Matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ColorSpaceTag {
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
    pub matrix: MatrixCoefficients,
}

impl ColorSpaceTag {
    /// Standard Rec.709 colorimetry (HDTV, web SDR).
    pub const REC709: Self = Self { primaries: ColorPrimaries::Bt709, transfer: TransferFunction::Bt709, matrix: MatrixCoefficients::Bt709 };

    /// sRGB colorimetry (standard computer graphics).
    pub const SRGB: Self = Self { primaries: ColorPrimaries::Bt709, transfer: TransferFunction::Srgb, matrix: MatrixCoefficients::Identity };

    /// HDR10 colorimetry (BT.2020 WCG + PQ EOTF).
    pub const HDR10: Self =
        Self { primaries: ColorPrimaries::Bt2020, transfer: TransferFunction::PerceptualQuantizer, matrix: MatrixCoefficients::Bt2020NonConstant };

    /// Apple Display P3 colorimetry.
    pub const DISPLAY_P3: Self =
        Self { primaries: ColorPrimaries::DisplayP3, transfer: TransferFunction::Srgb, matrix: MatrixCoefficients::Identity };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_buffer_sizes() {
        let dim = Dimensions::new(1920, 1080).unwrap();
        // 1920 * 1080 * 4 = 8,294,400 bytes
        assert_eq!(PixelFormat::Rgba8.required_buffer_size(dim).unwrap(), 8_294_400);

        // YUV 4:2:0 8-bit: 1920 * 1080 * 1.5 = 3,110,400 bytes
        assert_eq!(PixelFormat::Yuv420p.required_buffer_size(dim).unwrap(), 3_110_400);

        // YUV 4:2:2 10-bit: 1920 * 1080 * 2 * 2 bytes = 8,294,400 bytes
        assert_eq!(PixelFormat::Yuv422p10.required_buffer_size(dim).unwrap(), 8_294_400);
    }

    #[test]
    fn test_stride_alignment() {
        // 1920 width RGBA8 = 7680 bytes (already 64-byte aligned)
        assert_eq!(PixelFormat::Rgba8.aligned_stride(1920, 64).unwrap(), 7680);

        // 100 width RGB8 = 300 bytes; aligned to 64 bytes = 320 bytes
        assert_eq!(PixelFormat::Rgb8.aligned_stride(100, 64).unwrap(), 320);
    }

    #[test]
    fn test_color_space_tags() {
        assert_eq!(ColorSpaceTag::REC709.primaries, ColorPrimaries::Bt709);
        assert_eq!(ColorSpaceTag::HDR10.transfer, TransferFunction::PerceptualQuantizer);
    }
}
