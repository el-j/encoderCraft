//! Pure-Rust image encoders for encodercraft.
//!
//! Includes PNG (with zlib DEFLATE), baseline TIFF 6.0 (RGB / RGBA), and single-part OpenEXR (half float).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod exr;
pub mod png;
pub mod tiff;

pub use exr::encode_exr_half;
pub use png::encode_png;
pub use tiff::encode_tiff;
