//! Evaluates dynamic "Match Source" expressions against probed source media.

use std::path::PathBuf;

use encodercraft_core::{AudioEncodeSettings, ChannelLayout, EncoderError, OutputTarget, Rational64};
use encodercraft_demux::MediaInfo;

use crate::model::{EncodingPreset, MatchSourceOr};

/// Evaluates a preset against source media metadata to produce a concrete `OutputTarget`.
pub fn evaluate_preset(preset: &EncodingPreset, source: &MediaInfo, output_path: PathBuf) -> Result<OutputTarget, EncoderError> {
    let primary_video = source.video_streams.first();
    let primary_audio = source.audio_streams.first();

    // 1. Resolve video dimensions
    let (custom_width, custom_height, custom_frame_rate, mut target_bitrate_kbps) = if let Some(ref v_set) = preset.video {
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
    let audio = preset.audio.as_ref().map(|settings| AudioEncodeSettings {
        codec: Some(settings.codec.clone()),
        sample_rate: Some(match settings.sample_rate {
            MatchSourceOr::MatchSource => primary_audio.map_or(48_000, |audio| audio.sample_rate),
            MatchSourceOr::Exact(rate) => rate,
        }),
        layout: Some(match settings.layout {
            MatchSourceOr::MatchSource => primary_audio.map_or(ChannelLayout::Stereo, |audio| audio.layout),
            MatchSourceOr::Exact(layout) => layout,
        }),
        bitrate_kbps: settings.bitrate_kbps,
    });

    let mut video = preset.video_encode_settings();
    if let Some(v_set) = &preset.video {
        if v_set.match_source_bitrate {
            target_bitrate_kbps = primary_video
                .and_then(|v| v.bitrate)
                .map(|bps| (bps.saturating_mul(110) / 100_000).clamp(1, u32::MAX as u64) as u32)
                .or(target_bitrate_kbps);
        }
        if matches!(v_set.pixel_format, MatchSourceOr::MatchSource)
            && primary_video.is_some_and(|source| source.bit_depth > 8)
            && video.codec.as_deref().is_some_and(|codec| codec.eq_ignore_ascii_case("h264"))
        {
            video.codec = Some("hevc".into());
        }
    }

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
        audio,
        video,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_demux::{AudioStreamInfo, ContainerKind, VideoStreamInfo};
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
                bitrate: Some(50_000_000),
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
        assert_eq!(target.video.codec.as_deref(), Some("hevc"));
        assert_eq!(target.video.pixel_format, None);
        assert_eq!(target.target_bitrate_kbps, Some(55_000));
    }

    #[test]
    fn resolves_preset_audio_format_rate_and_layout() {
        let media_info = MediaInfo {
            container: ContainerKind::Wav,
            file_size_bytes: 10_000,
            duration_seconds: Some(2.0),
            video_streams: Vec::new(),
            audio_streams: vec![AudioStreamInfo {
                stream_index: 0,
                codec_name: "pcm_s16le".into(),
                sample_rate: 48_000,
                channels: 1,
                layout: ChannelLayout::Mono,
                bits_per_sample: 16,
                duration_seconds: Some(2.0),
            }],
            metadata: HashMap::new(),
        };
        let presets = crate::factory::factory_presets();
        let find = |name: &str| presets.iter().find(|preset| preset.name == name).expect("factory preset exists");

        let broadcast = evaluate_preset(find("Broadcast Wave BWF (-23 LUFS)"), &media_info, PathBuf::from("broadcast.wav")).unwrap();
        let broadcast_audio = broadcast.audio.expect("BWF preset has audio settings");
        assert_eq!(broadcast_audio.codec.as_deref(), Some("pcm_s24le"));
        assert_eq!(broadcast_audio.sample_rate, Some(48_000));
        assert_eq!(broadcast_audio.layout, Some(ChannelLayout::Mono));

        let podcast = evaluate_preset(find("Podcast Master (-16 LUFS)"), &media_info, PathBuf::from("podcast.wav")).unwrap();
        let podcast_audio = podcast.audio.expect("podcast preset has audio settings");
        assert_eq!(podcast_audio.codec.as_deref(), Some("pcm_s16le"));
        assert_eq!(podcast_audio.sample_rate, Some(44_100));
        assert_eq!(podcast_audio.layout, Some(ChannelLayout::Stereo));
    }
}
