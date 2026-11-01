use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{Rational64, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_pipeline::{FrameScaler, JobProgress};
use proptest::prelude::*;
use std::time::Instant;

proptest! {
    #[test]
    fn test_progress_fuzz_no_panic(
        advance_count in 0u64..100u64,
        total in 1u64..500u64,
    ) {
        let progress = JobProgress::new(total);
        let start = Instant::now();
        for _ in 0..advance_count {
            progress.advance_frame(start);
        }
        let _ = progress.percent_complete();
        let _ = progress.current_fps();
    }

    #[test]
    fn test_scale_fuzz_no_panic(
        w1 in 2u32..32u32,
        h1 in 2u32..32u32,
        w2 in 2u32..32u32,
        h2 in 2u32..32u32,
        scale_mode in prop::sample::select(vec![ScaleMode::Stretch, ScaleMode::Fit, ScaleMode::Fill, ScaleMode::Center]),
    ) {
        if let (Ok(d1), Ok(d2)) = (Dimensions::new(w1, h1), Dimensions::new(w2, h2))
            && let Ok(src) = VideoFrame::new_allocated(d1, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)
        {
            let scaler = FrameScaler::new(scale_mode);
            let _ = scaler.scale(&src, d2);
        }
    }
}
