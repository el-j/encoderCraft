//! Preset schema supporting dynamic "Match Source" expressions and multi-domain targets.

use encodercraft_color::PixelFormat;
use encodercraft_core::{ChannelLayout, ContainerType, Rational64};
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
