use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::Rational64;
use encodercraft_demux::{ContainerKind, MediaInfo, VideoStreamInfo};
use encodercraft_geom::Dimensions;
use encodercraft_presets::{PresetRegistry, evaluate_preset, factory_presets};
use proptest::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

proptest! {
    #[test]
    fn test_match_source_fuzz_no_panic(
        w in 16u32..4096u32,
        h in 16u32..4096u32,
        fps_num in 1i64..120i64,
    ) {
        let presets = factory_presets();
        if let (Ok(dims), Ok(fps)) = (Dimensions::new(w, h), Rational64::new(fps_num, 1)) {
            let media_info = MediaInfo {
                container: ContainerKind::QuickTimeMov,
                file_size_bytes: 1_000_000,
                duration_seconds: Some(10.0),
                video_streams: vec![VideoStreamInfo {
                    stream_index: 0,
                    codec_name: "prores".into(),
                    dimensions: dims,
                    pixel_format: PixelFormat::Yuv422p10,
                    color_space: ColorSpaceTag::REC709,
                    frame_rate: fps,
                    bit_depth: 10,
                    bitrate: None,
                    duration_seconds: Some(10.0),
                }],
                audio_streams: vec![],
                metadata: HashMap::new(),
            };

            for p in &presets {
                let _ = evaluate_preset(p, &media_info, PathBuf::from("out.mp4"));
            }
        }
    }

    #[test]
    fn test_preset_registry_fuzz_lookup(name in "[a-zA-Z0-9 -]{1,30}") {
        let reg = PresetRegistry::default();
        let _ = reg.find_by_name(&name);
    }
}
