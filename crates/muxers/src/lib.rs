//! Media container multiplexing engines for encodercraft.
//!
//! Provides progressive MP4 / QuickTime MOV muxing (with faststart web optimization),
//! Matroska / WebM EBML chunked cluster muxing, and Broadcast Wave Format (BWF) writing.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod mkv;
pub mod mp4;
pub mod muxer;
pub mod wav;

pub use mkv::{MkvMuxer, MkvMuxerBuilder};
pub use mp4::{Mp4Muxer, append_xmp_uuid_box};
pub use muxer::{AudioSample, AudioStreamConfig, BextMetadata, VideoSample, VideoStreamConfig};
pub use wav::{BwfWavWriter, WavFormatTag};
