//! Color spaces, pixel format layouts, and video colorimetry standards.
//!
//! Enforces zero-panic arithmetic for plane sizes, strides, and color conversions.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod lut;

pub use lut::{InterpolationMode, Lut1D, Lut3D, LutError};

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
    /// Planar YUV 4:2:2 8-bit (Avid DNxHD/DNxHR SQ/HQ, broadcast standard).
    Yuv422p,
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
        matches!(self, Self::Yuv420p | Self::Yuv422p | Self::Yuv420p10 | Self::Yuv422p10 | Self::Yuv444p12)
    }

    /// Bit depth per color component.
    pub fn bits_per_component(&self) -> u8 {
        match self {
            Self::Rgba8 | Self::Bgra8 | Self::Rgb8 | Self::Yuv420p | Self::Yuv422p | Self::Gray8 => 8,
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
            Self::Yuv420p | Self::Yuv422p | Self::Yuv420p10 | Self::Yuv422p10 | Self::Yuv444p12 => {
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
            Self::Yuv422p => {
                // 8-bit YUV 4:2:2: Y: w*h, U: (w/2)*h, V: (w/2)*h -> 2 * pixels
                let half_w = w.div_ceil(2);
                let uv_pixels = half_w.checked_mul(h).ok_or(ColorError::Overflow)?;
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
    /// BT.601 SDTV primaries (approximated by BT.709 for RGB gamut transforms).
    Bt601 = 6,
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

/// Signal range used by integer component formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorRange {
    Full,
    #[default]
    Limited,
}

/// Complete color-space and range tag for a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ColorSpaceTag {
    pub primaries: ColorPrimaries,
    pub transfer: TransferFunction,
    pub matrix: MatrixCoefficients,
    #[serde(default)]
    pub range: ColorRange,
}

impl ColorSpaceTag {
    /// Standard Rec.709 colorimetry (HDTV, web SDR).
    pub const REC709: Self =
        Self { primaries: ColorPrimaries::Bt709, transfer: TransferFunction::Bt709, matrix: MatrixCoefficients::Bt709, range: ColorRange::Limited };

    /// sRGB colorimetry (standard computer graphics).
    pub const SRGB: Self =
        Self { primaries: ColorPrimaries::Bt709, transfer: TransferFunction::Srgb, matrix: MatrixCoefficients::Identity, range: ColorRange::Full };

    /// HDR10 colorimetry (BT.2020 WCG + PQ EOTF).
    pub const HDR10: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferFunction::PerceptualQuantizer,
        matrix: MatrixCoefficients::Bt2020NonConstant,
        range: ColorRange::Limited,
    };

    /// Apple Display P3 colorimetry.
    pub const DISPLAY_P3: Self = Self {
        primaries: ColorPrimaries::DisplayP3,
        transfer: TransferFunction::Srgb,
        matrix: MatrixCoefficients::Identity,
        range: ColorRange::Full,
    };
}

/// Options controlling pixel buffer conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    /// Solid background colour onto which transparent pixels are composited when converting to a format without alpha.
    pub background: [u8; 3],
    /// Apply ordered dither when reducing bit depth (e.g. 16-bit to 8-bit) to prevent banding.
    pub dither: bool,
    /// Whether source pixels have premultiplied alpha.
    pub premultiplied: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self { background: [255, 255, 255], dither: false, premultiplied: false }
    }
}

/// 4x4 Bayer dither matrix values normalized to -4..+4 offset for 8-bit conversions.
const BAYER_4X4: [i16; 16] = [-4, 0, -3, 1, 2, -2, 3, -1, -3, 1, -4, 0, 3, -1, 2, -2];

/// Converts straight-alpha RGBA8 pixels in-place to premultiplied alpha.
pub fn premultiply_alpha(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = ((u32::from(px[0]) * a + 127) / 255) as u8;
        px[1] = ((u32::from(px[1]) * a + 127) / 255) as u8;
        px[2] = ((u32::from(px[2]) * a + 127) / 255) as u8;
    }
}

/// Converts premultiplied-alpha RGBA8 pixels in-place to straight alpha.
pub fn unpremultiply_alpha(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        let unmix = |c: u8| -> u8 { (u32::from(c) * 255 + a / 2).checked_div(a).unwrap_or(0).min(255) as u8 };
        px[0] = unmix(px[0]);
        px[1] = unmix(px[1]);
        px[2] = unmix(px[2]);
    }
}

/// Converts a pixel buffer from `src_format` to `dst_format` with canonical rounding and depth scaling.
pub fn convert_pixels(src: &[u8], src_format: PixelFormat, dst_format: PixelFormat, width: u32, height: u32) -> Result<Vec<u8>, ColorError> {
    convert_pixels_with_options(src, src_format, dst_format, width, height, &ConvertOptions::default())
}

/// Converts a pixel buffer from `src_format` to `dst_format` with explicit conversion options.
pub fn convert_pixels_with_options(
    src: &[u8],
    src_format: PixelFormat,
    dst_format: PixelFormat,
    width: u32,
    height: u32,
    options: &ConvertOptions,
) -> Result<Vec<u8>, ColorError> {
    if src_format == dst_format && !options.premultiplied {
        return Ok(src.to_vec());
    }
    let pixel_count = (width as usize).checked_mul(height as usize).ok_or(ColorError::Overflow)?;
    if pixel_count == 0 {
        return Ok(Vec::new());
    }

    // Step 1: Decode src into intermediate straight RGBA8
    let mut rgba8: Vec<u8> = match src_format {
        PixelFormat::Rgba8 => {
            if src.len() < pixel_count.saturating_mul(4) {
                return Err(ColorError::Overflow);
            }
            src[..pixel_count * 4].to_vec()
        }
        PixelFormat::Rgb8 => {
            if src.len() < pixel_count.saturating_mul(3) {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for px in src.chunks_exact(3).take(pixel_count) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        PixelFormat::Bgra8 => {
            if src.len() < pixel_count.saturating_mul(4) {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for px in src.chunks_exact(4).take(pixel_count) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            out
        }
        PixelFormat::Rgba16 => {
            if src.len() < pixel_count.saturating_mul(8) {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for (idx, px) in src.chunks_exact(8).take(pixel_count).enumerate() {
                let r = u16::from_le_bytes([px[0], px[1]]) as i32;
                let g = u16::from_le_bytes([px[2], px[3]]) as i32;
                let b = u16::from_le_bytes([px[4], px[5]]) as i32;
                let a = u16::from_le_bytes([px[6], px[7]]) as i32;
                let dither_val = if options.dither {
                    let x = idx % (width as usize);
                    let y = idx / (width as usize);
                    BAYER_4X4[(y % 4) * 4 + (x % 4)] as i32
                } else {
                    0
                };
                let scale = |v: i32| -> u8 {
                    let rounded = ((v + 128 + dither_val) / 257).clamp(0, 255);
                    rounded as u8
                };
                out.extend_from_slice(&[scale(r), scale(g), scale(b), scale(a)]);
            }
            out
        }
        PixelFormat::RgbaF32 => {
            if src.len() < pixel_count.saturating_mul(16) {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for px in src.chunks_exact(16).take(pixel_count) {
                let r = f32::from_le_bytes([px[0], px[1], px[2], px[3]]).clamp(0.0, 1.0);
                let g = f32::from_le_bytes([px[4], px[5], px[6], px[7]]).clamp(0.0, 1.0);
                let b = f32::from_le_bytes([px[8], px[9], px[10], px[11]]).clamp(0.0, 1.0);
                let a = f32::from_le_bytes([px[12], px[13], px[14], px[15]]).clamp(0.0, 1.0);
                out.extend_from_slice(&[(r * 255.0 + 0.5) as u8, (g * 255.0 + 0.5) as u8, (b * 255.0 + 0.5) as u8, (a * 255.0 + 0.5) as u8]);
            }
            out
        }
        PixelFormat::Gray8 => {
            if src.len() < pixel_count {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for &y in src.iter().take(pixel_count) {
                out.extend_from_slice(&[y, y, y, 255]);
            }
            out
        }
        PixelFormat::Gray16 => {
            if src.len() < pixel_count.saturating_mul(2) {
                return Err(ColorError::Overflow);
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for px in src.chunks_exact(2).take(pixel_count) {
                let y = u16::from_le_bytes([px[0], px[1]]);
                let y8 = ((y + 128) / 257) as u8;
                out.extend_from_slice(&[y8, y8, y8, 255]);
            }
            out
        }
        PixelFormat::Yuv420p => {
            let half_w = (width as usize).div_ceil(2);
            let half_h = (height as usize).div_ceil(2);
            let y_size = pixel_count;
            let uv_size = half_w.checked_mul(half_h).ok_or(ColorError::Overflow)?;
            if src.len() < y_size + uv_size * 2 {
                return Err(ColorError::Overflow);
            }
            let y_plane = &src[..y_size];
            let u_plane = &src[y_size..y_size + uv_size];
            let v_plane = &src[y_size + uv_size..y_size + uv_size * 2];
            let mut out = Vec::with_capacity(pixel_count * 4);
            let w = width as usize;
            for idx in 0..pixel_count {
                let py = idx / w;
                let px = idx % w;
                let uv_idx = (py / 2) * half_w + (px / 2);
                let y_val = i32::from(y_plane.get(idx).copied().unwrap_or(0));
                let u_val = i32::from(u_plane.get(uv_idx).copied().unwrap_or(128)) - 128;
                let v_val = i32::from(v_plane.get(uv_idx).copied().unwrap_or(128)) - 128;
                let r = (y_val + ((103_206 * v_val + 32_768) >> 16)).clamp(0, 255) as u8;
                let g = (y_val - ((12_276 * u_val + 30_679 * v_val + 32_768) >> 16)).clamp(0, 255) as u8;
                let b = (y_val + ((121_609 * u_val + 32_768) >> 16)).clamp(0, 255) as u8;
                out.extend_from_slice(&[r, g, b, 255]);
            }
            out
        }
        _ => return Err(ColorError::UnsupportedConversion { from: src_format, to: dst_format }),
    };

    if options.premultiplied {
        unpremultiply_alpha(&mut rgba8);
    }

    // Step 2: Encode intermediate straight RGBA8 into dst_format
    match dst_format {
        PixelFormat::Rgba8 => Ok(rgba8),
        PixelFormat::Rgb8 => {
            let mut out = Vec::with_capacity(pixel_count * 3);
            let bg = options.background;
            for px in rgba8.chunks_exact(4) {
                let (r, g, b, a) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]), u32::from(px[3]));
                if a == 255 {
                    out.extend_from_slice(&[px[0], px[1], px[2]]);
                } else {
                    let blend = |c: u32, bg_c: u8| ((c * a + u32::from(bg_c) * (255 - a) + 127) / 255) as u8;
                    out.extend_from_slice(&[blend(r, bg[0]), blend(g, bg[1]), blend(b, bg[2])]);
                }
            }
            Ok(out)
        }
        PixelFormat::Bgra8 => {
            let mut out = Vec::with_capacity(pixel_count * 4);
            for px in rgba8.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            Ok(out)
        }
        PixelFormat::Rgba16 => {
            let mut out = Vec::with_capacity(pixel_count * 8);
            for px in rgba8.chunks_exact(4) {
                let r16 = (px[0] as u16) * 257;
                let g16 = (px[1] as u16) * 257;
                let b16 = (px[2] as u16) * 257;
                let a16 = (px[3] as u16) * 257;
                out.extend_from_slice(&r16.to_le_bytes());
                out.extend_from_slice(&g16.to_le_bytes());
                out.extend_from_slice(&b16.to_le_bytes());
                out.extend_from_slice(&a16.to_le_bytes());
            }
            Ok(out)
        }
        PixelFormat::RgbaF32 => {
            let mut out = Vec::with_capacity(pixel_count * 16);
            for px in rgba8.chunks_exact(4) {
                let rf = (px[0] as f32) / 255.0;
                let gf = (px[1] as f32) / 255.0;
                let bf = (px[2] as f32) / 255.0;
                let af = (px[3] as f32) / 255.0;
                out.extend_from_slice(&rf.to_le_bytes());
                out.extend_from_slice(&gf.to_le_bytes());
                out.extend_from_slice(&bf.to_le_bytes());
                out.extend_from_slice(&af.to_le_bytes());
            }
            Ok(out)
        }
        PixelFormat::Gray8 => {
            let mut out = Vec::with_capacity(pixel_count);
            let bg = options.background;
            for px in rgba8.chunks_exact(4) {
                let (r, g, b, a) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]), u32::from(px[3]));
                let (fr, fg, fb) = if a == 255 {
                    (r, g, b)
                } else {
                    (
                        (r * a + u32::from(bg[0]) * (255 - a) + 127) / 255,
                        (g * a + u32::from(bg[1]) * (255 - a) + 127) / 255,
                        (b * a + u32::from(bg[2]) * (255 - a) + 127) / 255,
                    )
                };
                let y = ((fr * 54 + fg * 183 + fb * 19) / 256) as u8;
                out.push(y);
            }
            Ok(out)
        }
        PixelFormat::Gray16 => {
            let mut out = Vec::with_capacity(pixel_count * 2);
            let bg = options.background;
            for px in rgba8.chunks_exact(4) {
                let (r, g, b, a) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]), u32::from(px[3]));
                let (fr, fg, fb) = if a == 255 {
                    (r, g, b)
                } else {
                    (
                        (r * a + u32::from(bg[0]) * (255 - a) + 127) / 255,
                        (g * a + u32::from(bg[1]) * (255 - a) + 127) / 255,
                        (b * a + u32::from(bg[2]) * (255 - a) + 127) / 255,
                    )
                };
                let y8 = ((fr * 54 + fg * 183 + fb * 19) / 256) as u16;
                let y16 = y8 * 257;
                out.extend_from_slice(&y16.to_le_bytes());
            }
            Ok(out)
        }
        PixelFormat::Yuv420p => {
            let half_w = (width as usize).div_ceil(2);
            let half_h = (height as usize).div_ceil(2);
            let uv_size = half_w.checked_mul(half_h).ok_or(ColorError::Overflow)?;
            let mut y_plane = Vec::with_capacity(pixel_count);
            let mut u_plane = vec![0u8; uv_size];
            let mut v_plane = vec![0u8; uv_size];
            let mut uv_counts = vec![0u32; uv_size];
            let mut u_acc = vec![0u32; uv_size];
            let mut v_acc = vec![0u32; uv_size];

            let w = width as usize;
            for (idx, px) in rgba8.chunks_exact(4).enumerate() {
                let (r, g, b) = (i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
                let y_val = ((13933 * r + 46871 * g + 4732 * b + 32768) >> 16).clamp(0, 255) as u8;
                y_plane.push(y_val);

                let py = idx / w;
                let px_x = idx % w;
                let uv_idx = (py / 2) * half_w + (px_x / 2);
                let u_val = (((-7510 * r - 25258 * g + 32768 * b + 32768) >> 16) + 128).clamp(0, 255) as u32;
                let v_val = (((32768 * r - 29766 * g - 3002 * b + 32768) >> 16) + 128).clamp(0, 255) as u32;
                if let (Some(u_slot), Some(v_slot), Some(cnt_slot)) = (u_acc.get_mut(uv_idx), v_acc.get_mut(uv_idx), uv_counts.get_mut(uv_idx)) {
                    *u_slot += u_val;
                    *v_slot += v_val;
                    *cnt_slot += 1;
                }
            }

            for i in 0..uv_size {
                let count = uv_counts.get(i).copied().unwrap_or(1).max(1);
                let avg_u = (u_acc.get(i).copied().unwrap_or(128) / count) as u8;
                let avg_v = (v_acc.get(i).copied().unwrap_or(128) / count) as u8;
                if let (Some(u_slot), Some(v_slot)) = (u_plane.get_mut(i), v_plane.get_mut(i)) {
                    *u_slot = avg_u;
                    *v_slot = avg_v;
                }
            }

            let mut out = y_plane;
            out.extend_from_slice(&u_plane);
            out.extend_from_slice(&v_plane);
            Ok(out)
        }
        _ => Err(ColorError::UnsupportedConversion { from: src_format, to: dst_format }),
    }
}

/// Tone-maps a high dynamic range linear luminance value to standard dynamic range (Rec.709).
/// Uses an extended Reinhard tone-mapping curve with highlight compression:
/// `f(L) = L * (1 + L / L_white^2) / (1 + L)`
pub fn tonemap_reinhard(val: f32, max_white: f32) -> f32 {
    if !val.is_finite() || val <= 0.0 {
        return 0.0;
    }
    let white2 = (max_white * max_white).max(1.0);
    let mapped = (val * (1.0 + val / white2)) / (1.0 + val);
    mapped.clamp(0.0, 1.0)
}

/// Applies SDR conform tone mapping to an RGBA8 buffer.
pub fn apply_sdr_conform_rgba8(data: &mut [u8], target_nits: f32) {
    let scale = (target_nits / 100.0).clamp(0.25, 4.0);
    for chunk in data.chunks_exact_mut(4) {
        let r = tonemap_reinhard((chunk[0] as f32 / 255.0) * scale, 2.0) * 255.0;
        let g = tonemap_reinhard((chunk[1] as f32 / 255.0) * scale, 2.0) * 255.0;
        let b = tonemap_reinhard((chunk[2] as f32 / 255.0) * scale, 2.0) * 255.0;
        chunk[0] = r.round().clamp(0.0, 255.0) as u8;
        chunk[1] = g.round().clamp(0.0, 255.0) as u8;
        chunk[2] = b.round().clamp(0.0, 255.0) as u8;
    }
}

/// Applies SDR conform tone mapping to an RGBAF32 buffer.
pub fn apply_sdr_conform_rgbaf32(data: &mut [f32], target_nits: f32) {
    let scale = (target_nits / 100.0).clamp(0.25, 4.0);
    for chunk in data.chunks_exact_mut(4) {
        chunk[0] = tonemap_reinhard(chunk[0] * scale, 2.0);
        chunk[1] = tonemap_reinhard(chunk[1] * scale, 2.0);
        chunk[2] = tonemap_reinhard(chunk[2] * scale, 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_conversions() {
        let rgba = [255, 128, 64, 255];
        let rgb = convert_pixels(&rgba, PixelFormat::Rgba8, PixelFormat::Rgb8, 1, 1).unwrap();
        assert_eq!(rgb, vec![255, 128, 64]);

        let back_rgba = convert_pixels(&rgb, PixelFormat::Rgb8, PixelFormat::Rgba8, 1, 1).unwrap();
        assert_eq!(back_rgba, vec![255, 128, 64, 255]);

        let rgba16 = convert_pixels(&rgba, PixelFormat::Rgba8, PixelFormat::Rgba16, 1, 1).unwrap();
        assert_eq!(rgba16.len(), 8);

        let roundtrip_rgba = convert_pixels(&rgba16, PixelFormat::Rgba16, PixelFormat::Rgba8, 1, 1).unwrap();
        assert_eq!(roundtrip_rgba, vec![255, 128, 64, 255]);
    }

    #[test]
    fn test_known_color_fixtures() {
        // Red, Green, Blue, White, Black
        let colors = [
            ([255u8, 0, 0, 255], [255u8, 0, 0]),
            ([0, 255, 0, 255], [0, 255, 0]),
            ([0, 0, 255, 255], [0, 0, 255]),
            ([255, 255, 255, 255], [255, 255, 255]),
            ([0, 0, 0, 255], [0, 0, 0]),
        ];
        for (rgba, rgb) in colors {
            let res_rgb = convert_pixels(&rgba, PixelFormat::Rgba8, PixelFormat::Rgb8, 1, 1).unwrap();
            assert_eq!(res_rgb, rgb);
            let res_bgra = convert_pixels(&rgba, PixelFormat::Rgba8, PixelFormat::Bgra8, 1, 1).unwrap();
            assert_eq!(res_bgra, vec![rgba[2], rgba[1], rgba[0], rgba[3]]);
        }

        // 2x2 Checker
        let checker = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let rgb_checker = convert_pixels(&checker, PixelFormat::Rgba8, PixelFormat::Rgb8, 2, 2).unwrap();
        assert_eq!(rgb_checker.len(), 12);
        assert_eq!(&rgb_checker[0..3], &[255, 0, 0]);
        assert_eq!(&rgb_checker[3..6], &[0, 255, 0]);
        assert_eq!(&rgb_checker[6..9], &[0, 0, 255]);
        assert_eq!(&rgb_checker[9..12], &[255, 255, 255]);
    }

    #[test]
    fn test_alpha_flattening_on_background() {
        // Half transparent red [255, 0, 0, 128] over solid black [0, 0, 0] background -> [128, 0, 0]
        let semi_red = [255, 0, 0, 128];
        let opts_black = ConvertOptions { background: [0, 0, 0], ..Default::default() };
        let flattened = convert_pixels_with_options(&semi_red, PixelFormat::Rgba8, PixelFormat::Rgb8, 1, 1, &opts_black).unwrap();
        assert_eq!(flattened[0], 128);
        assert_eq!(flattened[1], 0);
        assert_eq!(flattened[2], 0);

        // Half transparent green [0, 255, 0, 128] over solid white [255, 255, 255] background -> [127, 255, 127]
        let semi_green = [0, 255, 0, 128];
        let opts_white = ConvertOptions { background: [255, 255, 255], ..Default::default() };
        let flattened_white = convert_pixels_with_options(&semi_green, PixelFormat::Rgba8, PixelFormat::Rgb8, 1, 1, &opts_white).unwrap();
        assert_eq!(flattened_white[0], 127);
        assert_eq!(flattened_white[1], 255);
        assert_eq!(flattened_white[2], 127);
    }

    #[test]
    fn test_premultiply_unpremultiply() {
        let mut straight = [200, 100, 50, 128];
        premultiply_alpha(&mut straight);
        assert_eq!(straight[3], 128);
        assert_eq!(straight[0], 100);
        assert_eq!(straight[1], 50);
        assert_eq!(straight[2], 25);

        unpremultiply_alpha(&mut straight);
        assert_eq!(straight[3], 128);
        assert_eq!(straight[0], 199); // 1-step rounding error typical of 8-bit roundtrip
        assert_eq!(straight[1], 100);
        assert_eq!(straight[2], 50);
    }

    #[test]
    fn test_dither_depth_reduction() {
        // High bit-depth 16-bit ramp
        let mut rgba16 = Vec::new();
        for x in 0..16u32 {
            let val = (x * 4096) as u16;
            rgba16.extend_from_slice(&val.to_le_bytes());
            rgba16.extend_from_slice(&val.to_le_bytes());
            rgba16.extend_from_slice(&val.to_le_bytes());
            rgba16.extend_from_slice(&65535u16.to_le_bytes());
        }
        let opts = ConvertOptions { dither: true, ..Default::default() };
        let dithered = convert_pixels_with_options(&rgba16, PixelFormat::Rgba16, PixelFormat::Rgba8, 16, 1, &opts).unwrap();
        assert_eq!(dithered.len(), 64);
    }

    #[test]
    fn test_yuv_rgb_roundtrip() {
        // Test with uniform 2x2 blocks so 4:2:0 chroma subsampling does not smear adjacent pixels
        let rgb_in = [180, 40, 40, 180, 40, 40, 180, 40, 40, 180, 40, 40];
        let yuv = convert_pixels(&rgb_in, PixelFormat::Rgb8, PixelFormat::Yuv420p, 2, 2).unwrap();
        let rgb_out = convert_pixels(&yuv, PixelFormat::Yuv420p, PixelFormat::Rgb8, 2, 2).unwrap();
        assert_eq!(rgb_out.len(), rgb_in.len());
        for (a, b) in rgb_in.iter().zip(rgb_out.iter()) {
            assert!(a.abs_diff(*b) <= 3, "YUV roundtrip diff too large: in={a}, out={b}");
        }
    }

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

    #[test]
    fn test_sdr_conform_tonemapping() {
        assert_eq!(tonemap_reinhard(0.0, 2.0), 0.0);
        let mid = tonemap_reinhard(0.5, 2.0);
        assert!(mid > 0.0 && mid <= 0.5);
        let peak = tonemap_reinhard(10.0, 2.0);
        assert!(peak <= 1.0);

        let mut pixels = [255u8, 200u8, 150u8, 255u8];
        apply_sdr_conform_rgba8(&mut pixels, 100.0);
        assert!(pixels[0] > 0);
        assert_eq!(pixels[3], 255); // Alpha preserved
    }
}
