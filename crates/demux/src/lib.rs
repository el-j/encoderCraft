//! Media container sniffing, stream probing, and demuxing engine.
//!
//! Enforces zero unwrap/panic and safe memory limits across all demuxing pipelines.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod aiff;
pub mod image_probe;
pub mod packet;
pub mod probe;
pub mod sniff;
pub mod wav;

pub use aiff::{AiffHeaderInfo, parse_aiff_header};
pub use image_probe::{ImageProbeInfo, probe_jpeg, probe_png, probe_webp};
pub use packet::{DemuxPacket, KeyframeIndexEntry, StreamKind};
pub use probe::{AudioStreamInfo, MediaInfo, VideoStreamInfo, classify_stream_kind, probe_bytes, probe_file};
pub use sniff::{ContainerKind, sniff_container};
pub use wav::{BroadcastWaveMetadata, WavAudioFormat, WavHeaderInfo, decode_wav_samples, parse_wav_header};
