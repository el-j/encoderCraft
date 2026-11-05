//! Typed error definitions for the entire transcoding engine.
//!
//! Enforces that no panic path can escape into client or user-facing code.

use thiserror::Error;

/// Comprehensive error enumeration for encoderCraft.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EncoderError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("Geometry error: {0}")]
    Geom(#[from] encodercraft_geom::GeomError),

    #[error("Color / Pixel format error: {0}")]
    Color(#[from] encodercraft_color::ColorError),

    #[error("Invalid media container: {0}")]
    InvalidContainer(String),

    #[error("Unsupported codec or format: {0}")]
    UnsupportedCodec(String),

    #[error("Invalid job specification: {0}")]
    InvalidJobSpec(String),

    #[error("Lossy conversion requires approval: {0}")]
    ConversionApprovalRequired(String),

    #[error("Job not found in queue: {0}")]
    JobNotFound(String),

    #[error("Transcoding cancelled by user or control signal")]
    Cancelled,

    #[error("Media stream truncated or unexpected end of file")]
    UnexpectedEof,

    #[error("Loudness normalization out of range: {0}")]
    LoudnessError(String),

    #[error("Watch folder error: {0}")]
    WatchError(String),

    #[error("IPC communication error: {0}")]
    IpcError(String),

    #[error("Internal engine error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for EncoderError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}
