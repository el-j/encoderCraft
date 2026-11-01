//! Video encoding engines and unified codec dispatcher for encodercraft.
//!
//! Includes GIF89a animated image encoder, Motion JPEG (MJPEG) frame encoder,
//! and dispatcher factory for ArtCraft sibling codec pipelines.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod dispatcher;
pub mod encoder;
pub mod gif;
pub mod mjpeg;

pub use dispatcher::{VideoCodecKind, create_video_encoder};
pub use encoder::{EncodedPacket, VideoEncoder};
pub use gif::GifEncoder;
pub use mjpeg::MjpegEncoder;
