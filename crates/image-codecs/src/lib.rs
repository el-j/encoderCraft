//! Pure-Rust image encoders for encodercraft.
//!
//! Includes PNG (with zlib DEFLATE), baseline TIFF 6.0 (RGB / RGBA), and single-part OpenEXR (half float).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod bmp;
pub mod dicom;
pub mod exr;
pub mod layered;
pub mod png;
pub mod still;
pub mod tiff;
pub mod vector;

pub use bmp::{decode_bmp, encode_bmp};
pub use dicom::encode_dicom;
pub use exr::encode_exr_half;
pub use layered::{ImageLayer, LayerBlendMode, LayeredColorMode, LayeredImage};
pub use png::encode_png;
pub use still::{StillFormat, StillOptions, encode_still};
pub use tiff::encode_tiff;
pub use vector::{Artboard, PathCommand, VectorCanvas, VectorCodecError, VectorColor, VectorPath, VectorStroke, VectorText};
