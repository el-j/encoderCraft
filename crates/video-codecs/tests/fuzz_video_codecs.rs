use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{Rational64, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_video_codecs::{MjpegEncoder, VideoEncoder};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_mjpeg_encoder_no_panic(
        w in 8u32..64u32,
        h in 8u32..64u32,
        quality in 1u8..100u8,
    ) {
        if let Ok(dims) = Dimensions::new(w, h)
            && let Ok(frame) = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)
        {
            let mut encoder = MjpegEncoder::new(quality);
            let _ = encoder.encode_frame(&frame);
            let _ = encoder.flush();
        }
    }
}
