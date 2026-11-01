//! Transcoding execution pipeline and multi-threaded concurrency coordination.
//!
//! Includes bounded frame channels, bilinear frame scaling with aspect-ratio preservation,
//! lockless atomic progress reporters, and transcode pipeline execution.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod captions;
pub mod channel_buffer;
pub mod lut;
pub mod naming;
pub mod overlay;
pub mod pipeline;
pub mod progress;
pub mod scale;
pub mod stitch;

pub use captions::{CaptionBurner, CaptionTrack, NativeCaptionFormat};
pub use channel_buffer::{DEFAULT_RING_BUFFER_CAPACITY, FrameRingBuffer};
pub use lut::Lut3D;
pub use naming::{CollisionPolicy, NamingContext, expand_template, resolve_output_path};
pub use overlay::{OverlayPosition, TimecodeOverlay, WatermarkOverlay};
pub use pipeline::TranscodePipeline;
pub use progress::JobProgress;
pub use scale::FrameScaler;
pub use stitch::{ClipStitcher, StitchOptions, stitch_audio_buffers};
