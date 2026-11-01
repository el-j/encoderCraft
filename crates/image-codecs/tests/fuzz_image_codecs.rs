use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{Rational64, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_image_codecs::{encode_exr_half, encode_png, encode_tiff};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_image_encoders_no_panic(
        w in 1u32..64u32,
        h in 1u32..64u32,
        format_rgba in any::<bool>(),
    ) {
        if let Ok(dims) = Dimensions::new(w, h) {
            let fmt = if format_rgba { PixelFormat::Rgba8 } else { PixelFormat::Rgb8 };
            if let Ok(frame) = VideoFrame::new_allocated(dims, fmt, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24) {
                let _ = encode_png(&frame);
                let _ = encode_tiff(&frame);
                let _ = encode_exr_half(&frame);
            }
        }
    }
}
