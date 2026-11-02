#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Hardware acceleration quarantine layer (ADR 0001).
//!
//! Provides hardware-accelerated capability discovery and platform bridges
//! (Apple VideoToolbox, NVIDIA NVENC, Linux VA-API, Windows Media Foundation)
//! while guaranteeing pure-Rust memory safety and zero-panic boundaries.

pub mod capabilities;

pub use capabilities::{HardwareBackend, HardwareCapabilities, detect_hardware};
