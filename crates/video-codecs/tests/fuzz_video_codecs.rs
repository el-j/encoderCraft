use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{Rational64, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_video_codecs::{MjpegEncoder, VideoCodecKind, VideoEncoder, create_video_encoder};
use proptest::prelude::*;

#[test]
fn test_all_sibling_encoders_frame() {
    let dims = Dimensions::new(128, 64).unwrap();
    let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

    // H.264
    let mut h264 = create_video_encoder(VideoCodecKind::H264, 128, 64, Rational64::FPS_24, Some(2000)).unwrap();
    let pkts = h264.encode_frame(&frame).unwrap();
    let flushed = h264.flush().unwrap();
    assert!(!pkts.is_empty() || !flushed.is_empty());

    // ProRes
    let mut prores = create_video_encoder(VideoCodecKind::ProRes, 128, 64, Rational64::FPS_24, None).unwrap();
    let pkts = prores.encode_frame(&frame).unwrap();
    assert_eq!(pkts.len(), 1);
    assert!(pkts[0].is_keyframe);

    // HEVC
    let mut hevc = create_video_encoder(VideoCodecKind::Hevc, 128, 64, Rational64::FPS_24, Some(1000)).unwrap();
    let pkts = hevc.encode_frame(&frame).unwrap();
    assert_eq!(pkts.len(), 1);
    assert!(pkts[0].is_keyframe);

    // AV1
    let mut av1 = create_video_encoder(VideoCodecKind::Av1, 128, 64, Rational64::FPS_24, Some(800)).unwrap();
    let pkts = av1.encode_frame(&frame).unwrap();
    assert_eq!(pkts.len(), 1);

    // VP9
    let mut vp9 = create_video_encoder(VideoCodecKind::Vp9, 128, 64, Rational64::FPS_24, Some(800)).unwrap();
    let pkts = vp9.encode_frame(&frame).unwrap();
    assert_eq!(pkts.len(), 1);

    // Avid DNxHR
    let mut dnx = create_video_encoder(VideoCodecKind::DnxHd, 128, 64, Rational64::FPS_24, None).unwrap();
    let pkts = dnx.encode_frame(&frame).unwrap();
    assert_eq!(pkts.len(), 1);
    assert!(pkts[0].is_keyframe);
    assert!(!pkts[0].data.is_empty());
}

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
