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
    AudioStemMode, CaptionExportFormat, ClientInfo, ContainerType, CraftJobSpec, JobPriority, JobStatus, MediaStreamKind, OutputTarget,
    SourceDescriptor, SourceSelection, StartPolicy, StitchedClipSource,
};
pub use metadata::XmpMetadata;
pub use timecode::{MediaTimecode, Rational64};
