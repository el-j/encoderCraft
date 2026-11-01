//! Demuxed compressed sample packets and stream descriptors.

use encodercraft_core::Rational64;
use serde::{Deserialize, Serialize};

/// Type of elementary media stream within a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Attachment,
    Data,
}

/// A demuxed packet containing compressed or raw sample bitstream data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemuxPacket {
    /// Zero-based index of the stream this packet belongs to.
    pub stream_index: usize,
    /// Presentation timestamp (in stream timebase units).
    pub pts: i64,
    /// Decode timestamp (in stream timebase units).
    pub dts: i64,
    /// Sample duration (in stream timebase units).
    pub duration: i64,
    /// Timebase defining ticks per second.
    pub timebase: Rational64,
    /// True if this packet is an independent synchronization / random access point (IDR/I-frame).
    pub is_keyframe: bool,
    /// Raw payload bytes of the compressed packet.
    pub data: Vec<u8>,
}

/// An entry in a stream's random-access keyframe index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyframeIndexEntry {
    /// Presentation timestamp of the keyframe.
    pub pts: i64,
    /// Byte offset in the source file or container.
    pub byte_offset: u64,
    /// Size of the keyframe in bytes.
    pub size_bytes: u32,
}
