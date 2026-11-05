//! Core data models, timecode representations, buffers, and job schemas.
//!
//! Enforces zero unwrap/panic and safe memory limits across all transcode pipelines.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod audio;
pub mod error;
pub mod frame;
pub mod job;
pub mod metadata;
pub mod timecode;

pub use audio::{AudioSampleBuffer, ChannelLayout};
pub use error::EncoderError;
pub use frame::{MAX_FRAME_BUFFER_BYTES, PlaneDescriptor, VideoFrame};
pub use job::{
    AudioEncodeSettings, AudioStemMode, CaptionExportFormat, ClientInfo, ColorTag, ContainerType, CraftJobSpec, FrameRateMode, JobPriority,
    JobStatus, MediaStreamKind, OutputTarget, RateMode, Rotation, ScalingFilter, SourceDescriptor, SourceSelection, StartPolicy, StitchedClipSource,
    VideoEncodeSettings, VideoTransform,
};
pub use metadata::XmpMetadata;
pub use timecode::{MediaTimecode, Rational64};
