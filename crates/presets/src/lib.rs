//! Universal preset system and dynamic "Match Source" evaluator for encodercraft.
//!
//! Includes factory presets for web, broadcast, social, and audio, along with user preset persistence.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod catalog;
pub mod factory;
pub mod match_source;
pub mod model;
pub mod store;

pub use factory::factory_presets;
pub use match_source::evaluate_preset;
pub use model::{EncodingPreset, MatchSourceOr, PresetAudioSettings, PresetCategory, PresetVideoSettings};
pub use store::PresetRegistry;
