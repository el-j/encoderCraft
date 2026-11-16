//! Core data models, timecode representations, buffers, and job schemas.
//!
//! Enforces zero unwrap/panic and safe memory limits across all transcode pipelines.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod audio;
pub mod error;
pub mod frame;
pub mod job;
pub mod metadata;
pub mod provider;
pub mod timecode;

pub use audio::{AudioChannelMapping, AudioSampleBuffer, AudioTrackLayout, ChannelLayout};
pub use error::EncoderError;
pub use frame::{MAX_FRAME_BUFFER_BYTES, PlaneDescriptor, VideoFrame};
pub use job::{
    AudioEncodeSettings, AudioStemMode, CaptionExportFormat, ClientInfo, ColorTag, ContainerType, CraftJobSpec, CropConfig, DeinterlaceMode,
    FlipMode, FrameRateMode, JobPriority, JobStatus, MediaStreamKind, NameOverlayConfig, OutputTarget, OverlayAnchor, PixelAspectRatioConform,
    RateMode, Rotation, ScalingFilter, SdrConformConfig, SourceDescriptor, SourceSelection, StartPolicy, StitchedClipSource, TimecodeOverlayConfig,
    VideoEncodeSettings, VideoLimiterClipLevel, VideoLimiterConfig, VideoTransform, WatermarkOverlayConfig,
};
pub use metadata::XmpMetadata;
pub use provider::{
    CURRENT_PROTOCOL_VERSION, DiscoveredProvider, MAX_MANIFEST_BYTES, ProviderManifest, ProviderOperation, ProviderOperationKind, ProviderParamType,
    ProviderParameterSpec, ProviderValidationError, default_provider_directories, discover_providers_in,
};
pub use timecode::{MediaTimecode, Rational64};
