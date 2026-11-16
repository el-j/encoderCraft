#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};

/// Supported hardware acceleration backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareBackend {
    /// Apple VideoToolbox (macOS / iOS hardware engine).
    VideoToolbox,
    /// NVIDIA NVENC (via CUDA / NVEncode driver).
    Nvenc,
    /// Linux Video Acceleration API (VAAPI).
    Vaapi,
    /// AMD Advanced Media Framework (AMF).
    Amf,
    /// Microsoft Windows Media Foundation (MF).
    MediaFoundation,
    /// No hardware acceleration available (pure-Rust software fallback).
    SoftwareOnly,
}

impl HardwareBackend {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::VideoToolbox => "Apple VideoToolbox (Hardware)",
            Self::Nvenc => "NVIDIA NVENC (Hardware)",
            Self::Vaapi => "Linux VA-API (Hardware)",
            Self::Amf => "AMD AMF (Hardware)",
            Self::MediaFoundation => "Windows Media Foundation (Hardware)",
            Self::SoftwareOnly => "Pure-Rust Software (CPU)",
        }
    }

    pub fn is_hardware(&self) -> bool {
        !matches!(self, Self::SoftwareOnly)
    }
}

/// Comprehensive hardware acceleration capabilities report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    pub backend: HardwareBackend,
    pub device_name: String,
    pub supports_h264: bool,
    pub supports_hevc: bool,
    pub supports_prores: bool,
    pub supports_av1: bool,
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for HardwareCapabilities {
    fn default() -> Self {
        Self {
            backend: HardwareBackend::SoftwareOnly,
            device_name: "Generic CPU (Pure-Rust Fallback)".into(),
            supports_h264: false,
            supports_hevc: false,
            supports_prores: false,
            supports_av1: false,
            max_width: 8192,
            max_height: 8192,
        }
    }
}

/// Detects available platform hardware acceleration capabilities safely.
///
/// Guaranteed never to panic; falls back to pure-Rust software mode if detection fails.
pub fn detect_hardware() -> HardwareCapabilities {
    #[cfg(target_os = "macos")]
    {
        detect_macos_videotoolbox()
    }
    #[cfg(target_os = "linux")]
    {
        detect_linux_vaapi()
    }
    #[cfg(target_os = "windows")]
    {
        detect_windows_mf()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        HardwareCapabilities::default()
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_videotoolbox() -> HardwareCapabilities {
    // Apple Silicon / Intel Mac hardware detection via sysctl model query
    let model = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.model")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Apple Hardware Engine".to_string());

    // Apple Silicon M-series (Mac1*, MacBookPro*, Macmini*) natively support H.264, HEVC, and ProRes encode
    let is_apple_silicon = model.starts_with("Mac") && !model.starts_with("MacBookPro11") && !model.starts_with("MacPro5");

    HardwareCapabilities {
        backend: HardwareBackend::VideoToolbox,
        device_name: format!("Apple VideoToolbox [{model}]"),
        supports_h264: true,
        supports_hevc: true,
        supports_prores: is_apple_silicon,
        supports_av1: model.contains("Mac15") || model.contains("Mac16"), // M3 / M4 generation AV1 decode/encode
        max_width: 8192,
        max_height: 8192,
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_vaapi() -> HardwareCapabilities {
    let has_vaapi = std::path::Path::new("/dev/dri/renderD128").exists();
    if has_vaapi {
        HardwareCapabilities {
            backend: HardwareBackend::Vaapi,
            device_name: "Linux VA-API Device (/dev/dri/renderD128)".into(),
            supports_h264: true,
            supports_hevc: true,
            supports_prores: false,
            supports_av1: false,
            max_width: 4096,
            max_height: 4096,
        }
    } else {
        HardwareCapabilities::default()
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_mf() -> HardwareCapabilities {
    HardwareCapabilities {
        backend: HardwareBackend::MediaFoundation,
        device_name: "Windows Media Foundation Hardware Encoder".into(),
        supports_h264: true,
        supports_hevc: true,
        supports_prores: false,
        supports_av1: false,
        max_width: 4096,
        max_height: 4096,
    }
}

/// Returns the list of available renderer display names for the current host system.
///
/// If a hardware acceleration backend is detected, it is returned first as the recommended
/// engine, followed by the software CPU renderer fallback.
pub fn available_renderers() -> Vec<String> {
    let caps = detect_hardware();
    let mut renderers = Vec::new();
    if caps.backend.is_hardware() {
        renderers.push(caps.backend.display_name().to_string());
    }
    renderers.push(HardwareBackend::SoftwareOnly.display_name().to_string());
    renderers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hardware_no_panic() {
        let caps = detect_hardware();
        assert!(!caps.device_name.is_empty());
        assert!(caps.max_width > 0);
        assert!(caps.max_height > 0);
    }

    #[test]
    fn test_available_renderers_lists_at_least_software() {
        let renderers = available_renderers();
        assert!(!renderers.is_empty());
        assert!(renderers.iter().any(|r| r.contains("Software")));
    }
}
