//! Pure-Rust digital signal processing (DSP) filters for encodercraft.
//!
//! Includes bandlimited sinc sample rate conversion, broadcast channel remixing,
//! ITU-R BS.1770-4 / EBU R128 loudness measurement and normalization, and True-Peak limiting.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod limiter;
pub mod loudness;
pub mod remix;
pub mod resample;

pub use limiter::PeakLimiter;
pub use loudness::{LoudnessStats, measure_loudness, normalize_loudness};
pub use remix::remix_channels;
pub use resample::SincResampler;
