//! Preset schema supporting dynamic "Match Source" expressions and multi-domain targets.

use encodercraft_color::PixelFormat;
use encodercraft_core::{ChannelLayout, ColorTag, ContainerType, RateMode, Rational64, VideoEncodeSettings};
use encodercraft_geom::ScaleMode;
use serde::{Deserialize, Serialize};

/// Either a fixed parameter value, or dynamic inheritance from source media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum MatchSourceOr<T> {
    #[default]
    MatchSource,
    Exact(T),
}

/// Broad categorization of presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresetCategory {
    #[default]
    Web,
    Broadcast,
    SocialMedia,
    Audio,
    StillImage,
    Spatial3D,
    Vector,
    Document,
    Custom,
}

impl PresetCategory {
    pub const ALL_SYSTEM: &'static [PresetCategory] =
        &[Self::Broadcast, Self::Web, Self::SocialMedia, Self::Audio, Self::StillImage, Self::Spatial3D, Self::Vector, Self::Document];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Custom => "User Presets & Groups",
            Self::Broadcast => "Broadcast & Production",
            Self::Web => "Web Video & Streaming",
            Self::SocialMedia => "Social Media",
            Self::Audio => "Audio Formats",
            Self::StillImage => "Still Image & Sequences",
            Self::Spatial3D => "3D & Spatial Media",
            Self::Vector => "Vector & Drafting",
            Self::Document => "Document & Publishing",
        }
    }
}

impl std::fmt::Display for PresetCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Video configuration settings inside a preset recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetVideoSettings {
    pub codec: String,
    pub width: MatchSourceOr<u32>,
    pub height: MatchSourceOr<u32>,
    pub frame_rate: MatchSourceOr<Rational64>,
    pub scale_mode: ScaleMode,
    pub pixel_format: MatchSourceOr<PixelFormat>,
    pub target_bitrate_kbps: u32,
    pub keyframe_interval: u32,
    /// Codec profile (ProRes `hq`, DNx `hqx`, H.264 `main`, ...); `None` uses the codec default.
    #[serde(default)]
    pub profile: Option<String>,
    /// Consecutive B-frames; `None` uses the codec default.
    #[serde(default)]
    pub bframes: Option<u8>,
    #[serde(default)]
    pub rate_mode: RateMode,
    /// Colour tag written into the bitstream; `None` keeps the codec default (BT.709).
    #[serde(default)]
    pub color: Option<ColorTag>,
    /// Match the source video bitrate when it is available; otherwise use `target_bitrate_kbps`.
    #[serde(default)]
    pub match_source_bitrate: bool,
}

/// Audio configuration settings inside a preset recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetAudioSettings {
    pub codec: String,
    pub sample_rate: MatchSourceOr<u32>,
    pub layout: MatchSourceOr<ChannelLayout>,
    pub bitrate_kbps: Option<u32>,
    pub target_lufs: Option<f64>,
}

/// Universal preset recipe for batch transcoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodingPreset {
    pub name: String,
    pub description: String,
    pub category: PresetCategory,
    pub container: ContainerType,
    pub video: Option<PresetVideoSettings>,
    pub audio: Option<PresetAudioSettings>,
}

impl EncodingPreset {
    /// Encoder settings of the video stream; default (inferred) settings for presets without video.
    pub fn video_encode_settings(&self) -> VideoEncodeSettings {
        self.video.as_ref().map_or_else(VideoEncodeSettings::default, |v| VideoEncodeSettings {
            codec: Some(v.codec.clone()),
            keyframe_interval: (v.keyframe_interval > 0).then_some(v.keyframe_interval),
            profile: v.profile.clone(),
            bframes: v.bframes,
            rate_mode: v.rate_mode,
            color: v.color,
            pixel_format: match v.pixel_format {
                MatchSourceOr::MatchSource => None,
                MatchSourceOr::Exact(format) => Some(format),
            },
        })
    }
}
