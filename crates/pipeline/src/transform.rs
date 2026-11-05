//! Geometric and temporal frame conversions applied before scaling: rotation and frame blending.

use std::sync::OnceLock;

use encodercraft_color::{ColorPrimaries, ColorRange, ColorSpaceTag, MatrixCoefficients, PixelFormat, TransferFunction};
use encodercraft_core::{ColorTag, EncoderError, Rotation, VideoFrame};
use encodercraft_geom::Dimensions;
use filmcraft_color::{ColorPipeline, ColorSpace, InputTransform, OutputTransform, Range};

/// Converts decoded frame signals from their source transfer/gamut into the target encoding.
pub(crate) struct ColorTransform {
    source: OnceLock<ColorSpace>,
    input: OnceLock<InputTransform>,
    output: OutputTransform,
    target_tag: ColorSpaceTag,
}

impl ColorTransform {
    pub(crate) fn new(target: Option<ColorTag>) -> Self {
        let target = target.unwrap_or(ColorTag::Bt709);
        let target_space = match target {
            ColorTag::Bt709 => ColorSpace::Rec709,
            ColorTag::Pq => ColorSpace::Rec2100Pq,
            ColorTag::Hlg => ColorSpace::Rec2100Hlg,
        };
        let pipeline = ColorPipeline::REC709;
        Self {
            source: OnceLock::new(),
            input: OnceLock::new(),
            output: OutputTransform::new(&pipeline, target_space),
            target_tag: Self::target_tag(Some(target)),
        }
    }

    pub(crate) fn target_tag(target: Option<ColorTag>) -> ColorSpaceTag {
        match target.unwrap_or(ColorTag::Bt709) {
            ColorTag::Bt709 => ColorSpaceTag::REC709,
            ColorTag::Pq => ColorSpaceTag::HDR10,
            ColorTag::Hlg => ColorSpaceTag {
                primaries: ColorPrimaries::Bt2020,
                transfer: TransferFunction::HybridLogGamma,
                matrix: MatrixCoefficients::Bt2020NonConstant,
                range: ColorRange::Limited,
            },
        }
    }

    pub(crate) fn apply(&self, frame: &mut VideoFrame) -> Result<(), EncoderError> {
        let source_space = filmcraft_space(frame.color_space);
        let known_source = self.source.get_or_init(|| source_space);
        if *known_source != source_space {
            return Err(EncoderError::InvalidContainer("decoded video color space changed during the stream".into()));
        }
        if source_space != self.output.target {
            let input = self.input.get_or_init(|| InputTransform::new(source_space, Range::Full, &ColorPipeline::REC709, None));
            match frame.format {
                PixelFormat::Rgb8 | PixelFormat::Rgba8 => {
                    let channels = if frame.format == PixelFormat::Rgb8 { 3 } else { 4 };
                    for pixel in frame.data_mut().chunks_exact_mut(channels) {
                        let linear = input.convert([f32::from(pixel[0]) / 255.0, f32::from(pixel[1]) / 255.0, f32::from(pixel[2]) / 255.0]);
                        let encoded = self.output.encode(linear);
                        for (channel, value) in encoded.into_iter().enumerate() {
                            pixel[channel] = (value * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                PixelFormat::RgbaF32 => apply_float_color_transform(frame, input, &self.output)?,
                format => {
                    return Err(EncoderError::UnsupportedCodec(format!("color conversion requires RGB8, RGBA8, or RGBAF32 frames, got {format:?}")));
                }
            }
        } else if !matches!(frame.format, PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::RgbaF32) {
            return Err(EncoderError::UnsupportedCodec(format!("color conversion requires RGB8, RGBA8, or RGBAF32 frames, got {:?}", frame.format)));
        }
        frame.color_space = ColorSpaceTag { range: ColorRange::Full, ..self.target_tag };
        Ok(())
    }
}

fn apply_float_color_transform(frame: &mut VideoFrame, input: &InputTransform, output: &OutputTransform) -> Result<(), EncoderError> {
    for pixel in frame.data_mut().as_chunks_mut::<16>().0 {
        let rgba = pixel.as_chunks_mut::<4>().0;
        if rgba.len() != 4 {
            return Err(EncoderError::Internal("RGBAF32 color transform received an incomplete pixel".into()));
        }
        let alpha = f32::from_ne_bytes(rgba[3]);
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err(EncoderError::InvalidContainer("RGBAF32 color transform received invalid alpha".into()));
        }
        let r = f32::from_ne_bytes(rgba[0]);
        let g = f32::from_ne_bytes(rgba[1]);
        let b = f32::from_ne_bytes(rgba[2]);
        let encoded = output.encode(input.convert([r, g, b]));
        for (channel, value) in encoded.into_iter().enumerate() {
            let Some(dest) = rgba.get_mut(channel) else {
                return Err(EncoderError::Internal("RGBAF32 color channel is out of range".into()));
            };
            *dest = value.to_ne_bytes();
        }
    }
    Ok(())
}

fn filmcraft_space(tag: ColorSpaceTag) -> ColorSpace {
    match (tag.primaries, tag.transfer) {
        (_, TransferFunction::PerceptualQuantizer) => ColorSpace::Rec2100Pq,
        (_, TransferFunction::HybridLogGamma) => ColorSpace::Rec2100Hlg,
        (ColorPrimaries::Bt2020, _) => ColorSpace::Rec2020,
        (ColorPrimaries::DisplayP3 | ColorPrimaries::DciP3, _) => ColorSpace::P3D65,
        (_, TransferFunction::Srgb) => ColorSpace::Srgb,
        (_, TransferFunction::Linear) => ColorSpace::Linear709,
        _ => ColorSpace::Rec709,
    }
}

fn bytes_per_pixel(format: PixelFormat) -> Result<usize, EncoderError> {
    match format {
        PixelFormat::Rgba8 => Ok(4),
        PixelFormat::Rgb8 => Ok(3),
        PixelFormat::RgbaF32 => Ok(16),
        other => Err(EncoderError::UnsupportedCodec(format!("frame transforms require RGB8, RGBA8, or RGBAF32 frames, got {other:?}"))),
    }
}

/// Rotates a packed RGB frame clockwise by `rotation`.
pub(crate) fn rotate(src: &VideoFrame, rotation: Rotation) -> Result<VideoFrame, EncoderError> {
    if rotation == Rotation::None {
        return Ok(src.clone());
    }
    let bpp = bytes_per_pixel(src.format)?;
    let sw = src.dimensions.width as usize;
    let sh = src.dimensions.height as usize;
    let dims = if rotation.swaps_axes() { Dimensions::new(src.dimensions.height, src.dimensions.width)? } else { src.dimensions };
    let mut dest = VideoFrame::new_allocated(dims, src.format, src.color_space, src.pts, src.duration, src.timebase)?;
    dest.keyframe = src.keyframe;
    let (src_data, src_stride) = src.plane(0)?;
    let (dest_data, dest_stride) = dest.plane_mut(0)?;
    let dw = dims.width as usize;
    let dh = dims.height as usize;

    for y in 0..dh {
        let row = dest_data
            .get_mut(y * dest_stride..y * dest_stride + dw * bpp)
            .ok_or_else(|| EncoderError::Internal("rotate: destination row out of range".into()))?;
        for (x, px) in row.chunks_exact_mut(bpp).enumerate() {
            let (sx, sy) = match rotation {
                Rotation::Cw90 => (y, sh - 1 - x),
                Rotation::Cw180 => (sw - 1 - x, sh - 1 - y),
                Rotation::Cw270 => (sw - 1 - y, x),
                Rotation::None => (x, y),
            };
            let off = sy * src_stride + sx * bpp;
            let from = src_data.get(off..off + bpp).ok_or_else(|| EncoderError::Internal("rotate: source pixel out of range".into()))?;
            px.copy_from_slice(from);
        }
    }
    Ok(dest)
}

/// Weighted mix of two equally sized frames: `weight_b` of 256 is all `b`.
pub(crate) fn blend(a: &VideoFrame, b: &VideoFrame, weight_b: u32) -> Result<VideoFrame, EncoderError> {
    if a.dimensions != b.dimensions || a.format != b.format {
        return Err(EncoderError::Internal("blend: frames differ in size or format".into()));
    }
    let bpp = bytes_per_pixel(a.format)?;
    let weight_b = weight_b.min(256);
    let weight_a = 256 - weight_b;
    let mut dest = VideoFrame::new_allocated(a.dimensions, a.format, a.color_space, a.pts, a.duration, a.timebase)?;
    let (a_data, a_stride) = a.plane(0)?;
    let (b_data, b_stride) = b.plane(0)?;
    let (dest_data, dest_stride) = dest.plane_mut(0)?;
    let row_bytes = a.dimensions.width as usize * bpp;
    for y in 0..a.dimensions.height as usize {
        let ra = a_data.get(y * a_stride..y * a_stride + row_bytes);
        let rb = b_data.get(y * b_stride..y * b_stride + row_bytes);
        let rd = dest_data.get_mut(y * dest_stride..y * dest_stride + row_bytes);
        let (Some(ra), Some(rb), Some(rd)) = (ra, rb, rd) else {
            return Err(EncoderError::Internal("blend: row out of range".into()));
        };
        if a.format == PixelFormat::RgbaF32 {
            for ((dst, pa), pb) in rd.as_chunks_mut::<4>().0.iter_mut().zip(ra.as_chunks::<4>().0).zip(rb.as_chunks::<4>().0) {
                let av = f32::from_ne_bytes(*pa);
                let bv = f32::from_ne_bytes(*pb);
                let value = (av * weight_a as f32 + bv * weight_b as f32) / 256.0;
                dst.copy_from_slice(&value.to_ne_bytes());
            }
        } else {
            for ((d, &pa), &pb) in rd.iter_mut().zip(ra).zip(rb) {
                *d = ((u32::from(pa) * weight_a + u32::from(pb) * weight_b + 128) >> 8).min(255) as u8;
            }
        }
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;

    /// A 3x2 RGB frame whose pixel (x, y) holds [x, y, 7].
    fn coordinate_frame() -> VideoFrame {
        let dims = Dimensions::new(3, 2).unwrap();
        let mut f = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        let (data, stride) = f.plane_mut(0).unwrap();
        for y in 0..2 {
            for x in 0..3 {
                data[y * stride + x * 3..y * stride + x * 3 + 3].copy_from_slice(&[x as u8, y as u8, 7]);
            }
        }
        f
    }

    fn px(f: &VideoFrame, x: usize, y: usize) -> [u8; 3] {
        let (d, s) = f.plane(0).unwrap();
        [d[y * s + x * 3], d[y * s + x * 3 + 1], d[y * s + x * 3 + 2]]
    }

    #[test]
    fn rotate_90_moves_top_left_to_top_right() {
        let r = rotate(&coordinate_frame(), Rotation::Cw90).unwrap();
        assert_eq!((r.dimensions.width, r.dimensions.height), (2, 3));
        assert_eq!(px(&r, 1, 0), [0, 0, 7]);
        assert_eq!(px(&r, 0, 0), [0, 1, 7]);
        assert_eq!(px(&r, 1, 2), [2, 0, 7]);
    }

    #[test]
    fn rotate_180_reverses_both_axes() {
        let r = rotate(&coordinate_frame(), Rotation::Cw180).unwrap();
        assert_eq!((r.dimensions.width, r.dimensions.height), (3, 2));
        assert_eq!(px(&r, 0, 0), [2, 1, 7]);
        assert_eq!(px(&r, 2, 1), [0, 0, 7]);
    }

    #[test]
    fn rotate_270_moves_top_left_to_bottom_left() {
        let r = rotate(&coordinate_frame(), Rotation::Cw270).unwrap();
        assert_eq!((r.dimensions.width, r.dimensions.height), (2, 3));
        assert_eq!(px(&r, 0, 2), [0, 0, 7]);
        assert_eq!(px(&r, 1, 0), [2, 1, 7]);
    }

    #[test]
    fn four_quarter_turns_restore_the_picture() {
        let src = coordinate_frame();
        let mut f = src.clone();
        for _ in 0..4 {
            f = rotate(&f, Rotation::Cw90).unwrap();
        }
        assert_eq!(f.data(), src.data());
    }

    #[test]
    fn blend_mixes_by_weight() {
        let a = coordinate_frame();
        let mut b = a.clone();
        b.data_mut().iter_mut().for_each(|v| *v = 200);
        let half = blend(&a, &b, 128).unwrap();
        assert_eq!(px(&half, 0, 0), [100, 100, 104]);
        assert_eq!(blend(&a, &b, 0).unwrap().data(), a.data());
        assert_eq!(blend(&a, &b, 256).unwrap().data(), b.data());
    }

    #[test]
    fn blend_rejects_mismatched_frames() {
        let a = coordinate_frame();
        let other =
            VideoFrame::new_allocated(Dimensions::new(4, 4).unwrap(), PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();
        assert!(blend(&a, &other, 128).is_err());
    }

    #[test]
    fn float_hdr_conversion_tone_maps_and_preserves_alpha() {
        let dims = Dimensions::new(1, 1).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::RgbaF32, ColorSpaceTag::HDR10, 0, 1, Rational64::FPS_24).unwrap();
        for (channel, value) in [0.5_f32, 0.5, 0.5, 0.5].into_iter().enumerate() {
            frame.data_mut()[channel * 4..channel * 4 + 4].copy_from_slice(&value.to_ne_bytes());
        }

        ColorTransform::new(Some(ColorTag::Bt709)).apply(&mut frame).unwrap();

        let alpha = f32::from_ne_bytes(frame.data()[12..16].try_into().unwrap());
        let red = f32::from_ne_bytes(frame.data()[0..4].try_into().unwrap());
        assert_eq!(alpha, 0.5);
        assert!(red.is_finite() && (0.0..=1.0).contains(&red), "tone-mapped encoded signal must remain finite and in gamut: {red}");
        assert_eq!(frame.color_space, ColorSpaceTag { range: ColorRange::Full, ..ColorSpaceTag::REC709 });
    }
}
