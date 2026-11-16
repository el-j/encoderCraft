//! Video encoding engines and unified codec dispatcher for encodercraft.
//!
//! Includes GIF89a animated image encoder, Motion JPEG (MJPEG) frame encoder,
//! and dispatcher factory for ArtCraft sibling codec pipelines.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod av1;
pub mod capabilities;
pub mod decoder;
pub mod dispatcher;
pub mod dnx;
pub mod encoder;
pub mod gif;
pub mod h264;
pub mod hevc;
pub mod mjpeg;
pub mod params;
pub mod prores;
pub mod vp9;
pub mod yuv;

pub use av1::Av1EncoderWrapper;
pub use decoder::{Vp8DecoderWrapper, decode_dnx_packet, decode_prores_packet, decode_video_packet_auto};
pub use dispatcher::{VideoCodecKind, create_video_encoder, create_video_encoder_with};
pub use dnx::DnxEncoderWrapper;
pub use encoder::{EncodedPacket, VideoEncoder};
pub use gif::GifEncoder;
pub use h264::H264EncoderWrapper;
pub use hevc::HevcEncoderWrapper;
pub use mjpeg::MjpegEncoder;
pub use params::EncodeParams;
pub use prores::ProResEncoderWrapper;
pub use vp9::Vp9EncoderWrapper;
