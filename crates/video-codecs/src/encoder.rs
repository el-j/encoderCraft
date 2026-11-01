//! Generic video encoding interface and packet abstractions.

use encodercraft_core::{EncoderError, VideoFrame};

/// An encoded video frame packet produced by a video encoder.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub pts: i64,
    pub dts: i64,
    pub is_keyframe: bool,
    pub duration_ticks: u32,
}

/// Abstract video encoder trait implemented by all video compression engines.
pub trait VideoEncoder {
    /// Submits a video frame for compression.
    /// May return zero packets (if buffering for B-frames / temporal GOP), or one or more packets.
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError>;

    /// Flushes any pending frames buffered inside the encoder pipeline.
    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError>;
}
