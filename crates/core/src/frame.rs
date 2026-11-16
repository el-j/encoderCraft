//! Uncompressed video frame buffers and strided plane management.
//!
//! Enforces bounded allocations, safe slice access, and clean stride alignment.

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_geom::Dimensions;

use crate::error::EncoderError;
use crate::timecode::Rational64;

/// Maximum allowed single-frame allocation size (512 MB, handles up to 8K 12-bit uncompressed).
pub const MAX_FRAME_BUFFER_BYTES: usize = 512 * 1024 * 1024;

/// Description of a single plane in a video frame (e.g. Y, U, V or packed RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDescriptor {
    pub offset: usize,
    pub stride: usize,
    pub width: u32,
    pub height: u32,
}

/// An uncompressed video frame in memory.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub dimensions: Dimensions,
    pub format: PixelFormat,
    pub color_space: ColorSpaceTag,
    pub pts: i64,
    pub duration: i64,
    pub timebase: Rational64,
    pub keyframe: bool,
    data: Vec<u8>,
    planes: Vec<PlaneDescriptor>,
}

impl VideoFrame {
    /// Creates a newly allocated video frame, validating buffer size limits.
    pub fn new_allocated(
        dimensions: Dimensions,
        format: PixelFormat,
        color_space: ColorSpaceTag,
        pts: i64,
        duration: i64,
        timebase: Rational64,
    ) -> Result<Self, EncoderError> {
        let buffer_size = format.required_buffer_size(dimensions)?;
        if buffer_size > MAX_FRAME_BUFFER_BYTES {
            return Err(EncoderError::InvalidJobSpec(format!(
                "frame buffer size ({} MB) exceeds maximum limit ({} MB)",
                buffer_size / (1024 * 1024),
                MAX_FRAME_BUFFER_BYTES / (1024 * 1024)
            )));
        }

        let mut data = vec![0u8; buffer_size];
        let planes = compute_plane_descriptors(dimensions, format, &mut data)?;

        Ok(Self { dimensions, format, color_space, pts, duration, timebase, keyframe: false, data, planes })
    }

    /// Creates a video frame from existing owned bytes, verifying that the byte length matches.
    pub fn from_raw_bytes(
        dimensions: Dimensions,
        format: PixelFormat,
        color_space: ColorSpaceTag,
        pts: i64,
        duration: i64,
        timebase: Rational64,
        mut data: Vec<u8>,
    ) -> Result<Self, EncoderError> {
        let expected_size = format.required_buffer_size(dimensions)?;
        if data.len() < expected_size {
            return Err(EncoderError::InvalidJobSpec(format!(
                "provided frame buffer ({} bytes) is smaller than required ({} bytes)",
                data.len(),
                expected_size
            )));
        }

        let planes = compute_plane_descriptors(dimensions, format, &mut data)?;

        Ok(Self { dimensions, format, color_space, pts, duration, timebase, keyframe: false, data, planes })
    }

    /// Read-only access to the contiguous raw frame bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Mutable access to the contiguous raw frame bytes.
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Number of planes in this frame (1 for packed RGBA, 3 for planar YUV).
    pub fn plane_count(&self) -> usize {
        self.planes.len()
    }

    /// Read-only slice access to a specific plane by index.
    pub fn plane(&self, index: usize) -> Result<(&[u8], usize), EncoderError> {
        let desc = self.planes.get(index).ok_or_else(|| EncoderError::InvalidJobSpec(format!("plane index {index} out of bounds")))?;

        let end = desc
            .offset
            .checked_add(desc.stride.checked_mul(desc.height as usize).unwrap_or(0))
            .ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

        let slice =
            self.data.get(desc.offset..end).ok_or_else(|| EncoderError::InvalidJobSpec(format!("plane {index} memory region out of bounds")))?;

        Ok((slice, desc.stride))
    }

    /// Mutable slice access to a specific plane by index.
    pub fn plane_mut(&mut self, index: usize) -> Result<(&mut [u8], usize), EncoderError> {
        let desc = self.planes.get(index).copied().ok_or_else(|| EncoderError::InvalidJobSpec(format!("plane index {index} out of bounds")))?;

        let end = desc
            .offset
            .checked_add(desc.stride.checked_mul(desc.height as usize).unwrap_or(0))
            .ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

        let stride = desc.stride;
        let slice =
            self.data.get_mut(desc.offset..end).ok_or_else(|| EncoderError::InvalidJobSpec(format!("plane {index} memory region out of bounds")))?;

        Ok((slice, stride))
    }
}

fn compute_plane_descriptors(dim: Dimensions, format: PixelFormat, _data: &mut [u8]) -> Result<Vec<PlaneDescriptor>, EncoderError> {
    let mut descriptors = Vec::new();
    let w = dim.width as usize;
    let h = dim.height as usize;

    match format {
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
            let stride = w.checked_mul(4).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: 0, stride, width: dim.width, height: dim.height });
        }
        PixelFormat::Rgb8 => {
            let stride = w.checked_mul(3).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: 0, stride, width: dim.width, height: dim.height });
        }
        PixelFormat::Rgba16 => {
            let stride = w.checked_mul(8).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: 0, stride, width: dim.width, height: dim.height });
        }
        PixelFormat::RgbaF32 => {
            let stride = w.checked_mul(16).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: 0, stride, width: dim.width, height: dim.height });
        }
        PixelFormat::Gray8 => {
            descriptors.push(PlaneDescriptor { offset: 0, stride: w, width: dim.width, height: dim.height });
        }
        PixelFormat::Gray16 => {
            let stride = w.checked_mul(2).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: 0, stride, width: dim.width, height: dim.height });
        }
        PixelFormat::Yuv420p => {
            let y_stride = w;
            let y_size = w.checked_mul(h).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            let uv_w = dim.width.div_ceil(2);
            let uv_h = dim.height.div_ceil(2);
            let uv_stride = uv_w as usize;
            let uv_size = (uv_w as usize).checked_mul(uv_h as usize).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            // Y plane
            descriptors.push(PlaneDescriptor { offset: 0, stride: y_stride, width: dim.width, height: dim.height });
            // U plane
            descriptors.push(PlaneDescriptor { offset: y_size, stride: uv_stride, width: uv_w, height: uv_h });
            // V plane
            let v_offset = y_size.checked_add(uv_size).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: v_offset, stride: uv_stride, width: uv_w, height: uv_h });
        }
        PixelFormat::Yuv422p => {
            let y_stride = w;
            let y_size = w.checked_mul(h).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            let uv_w = dim.width.div_ceil(2);
            let uv_h = dim.height;
            let uv_stride = uv_w as usize;
            let uv_size = (uv_w as usize).checked_mul(uv_h as usize).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            // Y plane
            descriptors.push(PlaneDescriptor { offset: 0, stride: y_stride, width: dim.width, height: dim.height });
            // U plane
            descriptors.push(PlaneDescriptor { offset: y_size, stride: uv_stride, width: uv_w, height: uv_h });
            // V plane
            let v_offset = y_size.checked_add(uv_size).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: v_offset, stride: uv_stride, width: uv_w, height: uv_h });
        }
        PixelFormat::Yuv420p10 | PixelFormat::Yuv422p10 | PixelFormat::Yuv444p12 => {
            // Planar 16-bit word samples
            let y_stride = w.checked_mul(2).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            let y_size = y_stride.checked_mul(h).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            let (uv_w, uv_h) = match format {
                PixelFormat::Yuv420p10 => (dim.width.div_ceil(2), dim.height.div_ceil(2)),
                PixelFormat::Yuv422p10 => (dim.width.div_ceil(2), dim.height),
                PixelFormat::Yuv444p12 => (dim.width, dim.height),
                _ => (dim.width, dim.height),
            };

            let uv_stride = (uv_w as usize).checked_mul(2).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            let uv_size = uv_stride.checked_mul(uv_h as usize).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;

            descriptors.push(PlaneDescriptor { offset: 0, stride: y_stride, width: dim.width, height: dim.height });
            descriptors.push(PlaneDescriptor { offset: y_size, stride: uv_stride, width: uv_w, height: uv_h });
            let v_offset = y_size.checked_add(uv_size).ok_or(EncoderError::Color(encodercraft_color::ColorError::Overflow))?;
            descriptors.push(PlaneDescriptor { offset: v_offset, stride: uv_stride, width: uv_w, height: uv_h });
        }
    }

    Ok(descriptors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_frame_allocation() {
        let dim = Dimensions::new(1280, 720).unwrap();
        let frame = VideoFrame::new_allocated(dim, PixelFormat::Rgba8, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_30).unwrap();

        assert_eq!(frame.plane_count(), 1);
        let (slice, stride) = frame.plane(0).unwrap();
        assert_eq!(stride, 1280 * 4);
        assert_eq!(slice.len(), 1280 * 720 * 4);
    }

    #[test]
    fn test_yuv_planar_allocation() {
        let dim = Dimensions::new(1920, 1080).unwrap();
        let frame = VideoFrame::new_allocated(dim, PixelFormat::Yuv420p, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_24).unwrap();

        assert_eq!(frame.plane_count(), 3);
        let (y_slice, y_stride) = frame.plane(0).unwrap();
        assert_eq!(y_stride, 1920);
        assert_eq!(y_slice.len(), 1920 * 1080);

        let (u_slice, u_stride) = frame.plane(1).unwrap();
        assert_eq!(u_stride, 960);
        assert_eq!(u_slice.len(), 960 * 540);
    }

    #[test]
    fn rejects_frame_dimensions_over_the_allocation_limit() {
        let dim = Dimensions::new(16_000, 9_000).unwrap();
        let error = VideoFrame::new_allocated(dim, PixelFormat::Rgba8, ColorSpaceTag::REC709, 0, 1, Rational64::FPS_30);
        assert!(error.is_err());
    }
}
