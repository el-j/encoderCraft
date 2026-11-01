//! Evaluates dynamic "Match Source" expressions against probed source media.

use std::path::PathBuf;

use encodercraft_core::{EncoderError, OutputTarget, Rational64};
use encodercraft_demux::MediaInfo;

use crate::model::{EncodingPreset, MatchSourceOr};

/// Evaluates a preset against source media metadata to produce a concrete `OutputTarget`.
pub fn evaluate_preset(preset: &EncodingPreset, source: &MediaInfo, output_path: PathBuf) -> Result<OutputTarget, EncoderError> {
    let primary_video = source.video_streams.first();
    let _primary_audio = source.audio_streams.first();

    // 1. Resolve video dimensions
    let (custom_width, custom_height, custom_frame_rate, target_bitrate_kbps) = if let Some(ref v_set) = preset.video {
        let width = match v_set.width {
            MatchSourceOr::MatchSource => primary_video.map(|v| v.dimensions.width).unwrap_or(1920),
            MatchSourceOr::Exact(w) => w,
        };

        let height = match v_set.height {
            MatchSourceOr::MatchSource => primary_video.map(|v| v.dimensions.height).unwrap_or(1080),
            MatchSourceOr::Exact(h) => h,
        };

        let frame_rate = match v_set.frame_rate {
            MatchSourceOr::MatchSource => primary_video.map(|v| v.frame_rate).unwrap_or(Rational64::FPS_24),
            MatchSourceOr::Exact(fps) => fps,
        };

        (Some(width), Some(height), Some(frame_rate), Some(v_set.target_bitrate_kbps))
    } else {
        (None, None, None, None)
    };

    // 2. Resolve audio settings
    let target_lufs = if let Some(ref a_set) = preset.audio { a_set.target_lufs.map(|l| l as f32) } else { None };

    Ok(OutputTarget {
        target_id: format!("{}-target", preset.name.to_ascii_lowercase().replace(' ', "-")),
        output_path,
        preset_name: preset.name.clone(),
        container: preset.container,
        custom_width,
        custom_height,
        custom_frame_rate,
        target_bitrate_kbps,
        target_lufs,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_demux::{ContainerKind, VideoStreamInfo};
    use encodercraft_geom::Dimensions;
    use std::collections::HashMap;

    #[test]
    fn test_match_source_resolution() {
        let presets = crate::factory::factory_presets();
        let match_source_preset = &presets[0]; // "Match Source - High Bitrate"

        let media_info = MediaInfo {
            container: ContainerKind::QuickTimeMov,
            file_size_bytes: 1_000_000,
            duration_seconds: Some(10.0),
            video_streams: vec![VideoStreamInfo {
                stream_index: 0,
                codec_name: "prores".into(),
                dimensions: Dimensions::new(3840, 2160).unwrap(),
                pixel_format: PixelFormat::Yuv422p10,
                color_space: ColorSpaceTag::REC709,
                frame_rate: Rational64::new(60, 1).unwrap(),
                bit_depth: 10,
                duration_seconds: Some(10.0),
            }],
            audio_streams: vec![],
            metadata: HashMap::new(),
        };

        let target = evaluate_preset(match_source_preset, &media_info, PathBuf::from("out.mp4")).unwrap();

        // Must inherit 3840x2160 at 60 fps
        assert_eq!(target.custom_width, Some(3840));
        assert_eq!(target.custom_height, Some(2160));
        assert_eq!(target.custom_frame_rate, Some(Rational64::new(60, 1).unwrap()));
    }
}
