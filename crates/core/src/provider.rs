//! Codec provider manifest data structures, schema validation, and discovery.
//!
//! Enforces strict path containment, bounded manifest size, and zero-execution discovery (ADR 0002).

use std::path::{Component, Path, PathBuf};

use encodercraft_color::PixelFormat;
use serde::{Deserialize, Serialize};

use crate::job::ContainerType;

/// Current supported wire protocol version for out-of-process codec providers.
pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

/// Maximum allowable size for a `provider.json` manifest file (1 MB).
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Validation and parsing errors for codec provider manifests.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderValidationError {
    #[error("manifest file exceeds maximum allowed size ({0} bytes > {MAX_MANIFEST_BYTES} bytes)")]
    FileTooLarge(u64),

    #[error("I/O error reading provider manifest: {0}")]
    Io(String),

    #[error("failed to deserialize provider JSON: {0}")]
    Json(String),

    #[error("invalid provider identifier '{0}': must be 1..=64 characters of [a-zA-Z0-9_.-]")]
    InvalidId(String),

    #[error("invalid version string '{0}': must follow semantic versioning")]
    InvalidVersion(String),

    #[error("unsupported provider protocol version {0}: this build requires version {CURRENT_PROTOCOL_VERSION}")]
    UnsupportedProtocol(u32),

    #[error("missing executable path in manifest")]
    MissingExecutable,

    #[error("insecure executable path '{0}': must be a relative path strictly contained within the provider directory")]
    InsecureExecutablePath(String),

    #[error("declared executable '{0}' does not exist or is not a regular file")]
    ExecutableNotFound(PathBuf),

    #[error("manifest must declare at least one supported codec operation")]
    NoOperations,

    #[error("duplicate codec identifier '{0}' declared within provider operations")]
    DuplicateCodecId(String),

    #[error("invalid or unrecognised container format: {0}")]
    InvalidContainer(String),
}

/// Operational capability mode declared by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderOperationKind {
    VideoEncode,
    AudioEncode,
    VideoDecode,
    AudioDecode,
}

/// Parameter data type descriptor for custom codec settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProviderParamType {
    Integer {
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
        default: i64,
    },
    Float {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        default: f64,
    },
    String {
        default: String,
        #[serde(default)]
        choices: Vec<String>,
    },
    Boolean {
        default: bool,
    },
}

/// Specification for a custom configuration parameter exposed by a provider operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderParameterSpec {
    pub name: String,
    pub label: String,
    pub param_type: ProviderParamType,
    #[serde(default)]
    pub description: Option<String>,
}

/// Single codec operation provided by an external executable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOperation {
    pub operation_kind: ProviderOperationKind,
    pub codec_id: String,
    pub display_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_pixel_formats")]
    pub pixel_formats: Vec<PixelFormat>,
    #[serde(default)]
    pub sample_formats: Vec<String>,
    #[serde(default)]
    pub sample_rates: Vec<u32>,
    #[serde(default, deserialize_with = "deserialize_containers")]
    pub containers: Vec<ContainerType>,
    #[serde(default)]
    pub parameters: Vec<ProviderParameterSpec>,
}

fn parse_pixel_format_str(s: &str) -> Option<PixelFormat> {
    let clean = s.trim().to_ascii_lowercase();
    match clean.as_str() {
        "rgba8" | "rgba" => Some(PixelFormat::Rgba8),
        "bgra8" | "bgra" => Some(PixelFormat::Bgra8),
        "rgb8" | "rgb" | "rgb24" => Some(PixelFormat::Rgb8),
        "rgba16" | "rgba64" | "rgba16le" => Some(PixelFormat::Rgba16),
        "rgbaf32" | "rgba_f32" | "rgbafloat" => Some(PixelFormat::RgbaF32),
        "yuv420p" | "yuv420" | "i420" => Some(PixelFormat::Yuv420p),
        "yuv422p" | "yuv422" | "i422" => Some(PixelFormat::Yuv422p),
        "yuv420p10" | "yuv420p10le" | "yuv420_10" => Some(PixelFormat::Yuv420p10),
        "yuv422p10" | "yuv422p10le" | "yuv422_10" => Some(PixelFormat::Yuv422p10),
        "yuv444p12" | "yuv444p12le" | "yuv444_12" => Some(PixelFormat::Yuv444p12),
        "gray8" | "y8" | "gray" => Some(PixelFormat::Gray8),
        "gray16" | "y16" => Some(PixelFormat::Gray16),
        _ => None,
    }
}

fn deserialize_pixel_formats<'de, D>(deserializer: D) -> Result<Vec<PixelFormat>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let strings: Vec<String> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();
    for s in strings {
        if let Some(pf) = parse_pixel_format_str(&s) {
            result.push(pf);
        } else if let Ok(pf) = serde_json::from_value::<PixelFormat>(serde_json::Value::String(s.clone())) {
            result.push(pf);
        } else {
            return Err(serde::de::Error::custom(format!("unrecognised pixel format '{s}'")));
        }
    }
    Ok(result)
}

fn deserialize_containers<'de, D>(deserializer: D) -> Result<Vec<ContainerType>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let strings: Vec<String> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();
    for s in strings {
        if let Some(c) = ContainerType::from_extension(&s) {
            result.push(c);
        } else if let Ok(c) = serde_json::from_value::<ContainerType>(serde_json::Value::String(s.clone())) {
            result.push(c);
        } else {
            return Err(serde::de::Error::custom(format!("unrecognised container format '{s}'")));
        }
    }
    Ok(result)
}

/// Declarative metadata manifest (`provider.json`) describing an external codec provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderManifest {
    /// Unique machine identifier slug (e.g. `org.svt.av1`, `com.vendor.prores`).
    pub id: String,
    /// Human-friendly display title.
    pub name: String,
    /// Semantic version of the provider package (e.g. `1.2.0`).
    pub version: String,
    /// Supported wire protocol version (e.g. 1).
    pub protocol_version: u32,
    /// Minimum/compatible encoderCraft version range (optional).
    #[serde(default)]
    pub encodercraft_version: Option<String>,
    /// Brief description of the codec engine and capabilities.
    #[serde(default)]
    pub description: Option<String>,
    /// Author, organisation, or maintainer attribution.
    #[serde(default)]
    pub author: Option<String>,
    /// Software license identifier (e.g. `BSD-3-Clause`, `LGPL-3.0-or-later`, `Proprietary`).
    pub license: String,
    /// Official website or source repository link.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Relative path to the executable binary inside the provider package.
    pub executable: PathBuf,
    /// Declared list of codec operations.
    pub operations: Vec<ProviderOperation>,
}

impl ProviderManifest {
    /// Parses a provider manifest from raw JSON string, validating schema boundaries.
    pub fn parse_str(json: &str) -> Result<Self, ProviderValidationError> {
        if json.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(ProviderValidationError::FileTooLarge(json.len() as u64));
        }
        let manifest: Self = serde_json::from_str(json).map_err(|e| ProviderValidationError::Json(e.to_string()))?;
        manifest.validate_schema()?;
        Ok(manifest)
    }

    /// Reads and parses a `provider.json` file from disk.
    pub fn from_file(path: &Path) -> Result<Self, ProviderValidationError> {
        let meta = std::fs::metadata(path).map_err(|e| ProviderValidationError::Io(e.to_string()))?;
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err(ProviderValidationError::FileTooLarge(meta.len()));
        }
        let content = std::fs::read_to_string(path).map_err(|e| ProviderValidationError::Io(e.to_string()))?;
        Self::parse_str(&content)
    }

    /// Validates self-contained fields (IDs, versioning, protocol, non-empty operations).
    pub fn validate_schema(&self) -> Result<(), ProviderValidationError> {
        if self.id.is_empty() || self.id.len() > 64 {
            return Err(ProviderValidationError::InvalidId(self.id.clone()));
        }
        for b in self.id.bytes() {
            let valid = b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_';
            if !valid {
                return Err(ProviderValidationError::InvalidId(self.id.clone()));
            }
        }

        if self.version.trim().is_empty() {
            return Err(ProviderValidationError::InvalidVersion(self.version.clone()));
        }

        if self.protocol_version != CURRENT_PROTOCOL_VERSION {
            return Err(ProviderValidationError::UnsupportedProtocol(self.protocol_version));
        }

        if self.executable.as_os_str().is_empty() {
            return Err(ProviderValidationError::MissingExecutable);
        }

        // Path containment check: executable MUST be relative and cannot use `..`
        for component in self.executable.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(ProviderValidationError::InsecureExecutablePath(self.executable.display().to_string()));
                }
                Component::Normal(_) | Component::CurDir => {}
            }
        }

        if self.operations.is_empty() {
            return Err(ProviderValidationError::NoOperations);
        }

        let mut seen = std::collections::HashSet::new();
        for op in &self.operations {
            let key = (op.operation_kind, op.codec_id.to_ascii_lowercase());
            if !seen.insert(key) {
                return Err(ProviderValidationError::DuplicateCodecId(op.codec_id.clone()));
            }
        }

        Ok(())
    }

    /// Validates manifest against its filesystem directory (checks executable existence & security).
    pub fn validate(&self, provider_root: &Path) -> Result<PathBuf, ProviderValidationError> {
        self.validate_schema()?;

        let full_exe = provider_root.join(&self.executable);
        if !full_exe.is_file() {
            return Err(ProviderValidationError::ExecutableNotFound(full_exe));
        }

        Ok(full_exe)
    }
}

/// Discovered provider record containing validated manifest and host file locations.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredProvider {
    pub manifest: ProviderManifest,
    pub root_dir: PathBuf,
    pub executable_path: PathBuf,
}

/// Standard per-user directories scanned for external codec providers.
pub fn default_provider_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(env_path) = std::env::var("ENCODERCRAFT_PROVIDERS_DIR") {
        let p = PathBuf::from(env_path);
        if p.is_dir() {
            dirs.push(p);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home).join("Library/Application Support/encoderCraft/providers");
            dirs.push(p);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(appdata).join("encoderCraft").join("providers");
            dirs.push(p);
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            let p = PathBuf::from(data_home).join("encodercraft/providers");
            dirs.push(p);
        } else if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home).join(".local/share/encodercraft/providers");
            dirs.push(p);
        }
    }

    dirs
}

/// Scans a root directory for subfolders containing `provider.json` files.
/// Strictly non-executing: parses and validates declarations only.
pub fn discover_providers_in(root: &Path) -> Vec<Result<DiscoveredProvider, ProviderValidationError>> {
    let mut results = Vec::new();

    let Ok(entries) = std::fs::read_dir(root) else {
        return results;
    };

    for entry in entries.flatten() {
        let sub_dir = entry.path();
        if !sub_dir.is_dir() {
            continue;
        }

        let manifest_path = sub_dir.join("provider.json");
        if !manifest_path.is_file() {
            continue;
        }

        match ProviderManifest::from_file(&manifest_path) {
            Ok(manifest) => match manifest.validate(&sub_dir) {
                Ok(executable_path) => {
                    results.push(Ok(DiscoveredProvider { manifest, root_dir: sub_dir, executable_path }));
                }
                Err(e) => results.push(Err(e)),
            },
            Err(e) => results.push(Err(e)),
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_VALID_MANIFEST: &str = r#"{
        "id": "org.artcraft.svt-av1",
        "name": "SVT-AV1 Research Encoder",
        "version": "1.8.0",
        "protocolVersion": 1,
        "description": "Scalable Video Technology AV1 encoder provider",
        "author": "ArtCraft Contributors",
        "license": "BSD-3-Clause",
        "homepage": "https://github.com/storytold/encodercraft",
        "executable": "bin/svt_worker",
        "operations": [
            {
                "operationKind": "videoEncode",
                "codecId": "svt-av1",
                "displayName": "SVT-AV1 Professional",
                "aliases": ["av1-svt", "av01"],
                "profiles": ["main", "high"],
                "pixelFormats": ["yuv420p", "yuv420p10le"],
                "sampleFormats": [],
                "sampleRates": [],
                "containers": ["mp4", "mkv"],
                "parameters": [
                    {
                        "name": "preset",
                        "label": "Encoder Speed Preset",
                        "paramType": {
                            "type": "integer",
                            "min": 0,
                            "max": 13,
                            "default": 7
                        },
                        "description": "CPU/efficiency tradeoff"
                    }
                ]
            }
        ]
    }"#;

    #[test]
    fn parses_valid_manifest() {
        let manifest = ProviderManifest::parse_str(SAMPLE_VALID_MANIFEST).unwrap();
        assert_eq!(manifest.id, "org.artcraft.svt-av1");
        assert_eq!(manifest.protocol_version, 1);
        assert_eq!(manifest.operations.len(), 1);
        assert_eq!(manifest.operations[0].codec_id, "svt-av1");
        assert_eq!(manifest.operations[0].containers, vec![ContainerType::IsoBmffMp4, ContainerType::MatroskaMkv]);
    }

    #[test]
    fn rejects_insecure_relative_path() {
        let bad = SAMPLE_VALID_MANIFEST.replace("bin/svt_worker", "../../../bin/evil");
        let err = ProviderManifest::parse_str(&bad).unwrap_err();
        assert!(matches!(err, ProviderValidationError::InsecureExecutablePath(_)));
    }

    #[test]
    fn rejects_unsupported_protocol_version() {
        let bad = SAMPLE_VALID_MANIFEST.replace("\"protocolVersion\": 1", "\"protocolVersion\": 99");
        let err = ProviderManifest::parse_str(&bad).unwrap_err();
        assert!(matches!(err, ProviderValidationError::UnsupportedProtocol(99)));
    }

    #[test]
    fn rejects_invalid_id() {
        let bad = SAMPLE_VALID_MANIFEST.replace("org.artcraft.svt-av1", "invalid id with spaces!");
        let err = ProviderManifest::parse_str(&bad).unwrap_err();
        assert!(matches!(err, ProviderValidationError::InvalidId(_)));
    }

    #[test]
    fn rejects_empty_operations() {
        let bad = SAMPLE_VALID_MANIFEST.replace(
            r#""operations": [
            {
                "operationKind": "videoEncode",
                "codecId": "svt-av1",
                "displayName": "SVT-AV1 Professional",
                "aliases": ["av1-svt", "av01"],
                "profiles": ["main", "high"],
                "pixelFormats": ["yuv420p", "yuv420p10le"],
                "sampleFormats": [],
                "sampleRates": [],
                "containers": ["mp4", "mkv"],
                "parameters": [
                    {
                        "name": "preset",
                        "label": "Encoder Speed Preset",
                        "paramType": {
                            "type": "integer",
                            "min": 0,
                            "max": 13,
                            "default": 7
                        },
                        "description": "CPU/efficiency tradeoff"
                    }
                ]
            }
        ]"#,
            r#""operations": []"#,
        );
        let err = ProviderManifest::parse_str(&bad).unwrap_err();
        assert!(matches!(err, ProviderValidationError::NoOperations));
    }

    #[test]
    fn rejects_duplicate_codec_ids_in_same_provider() {
        let bad = SAMPLE_VALID_MANIFEST.replace(
            r#""operations": ["#,
            r#""operations": [
            {
                "operationKind": "videoEncode",
                "codecId": "svt-av1",
                "displayName": "SVT-AV1 Clone",
                "aliases": [],
                "profiles": [],
                "pixelFormats": [],
                "sampleFormats": [],
                "sampleRates": [],
                "containers": ["mp4"],
                "parameters": []
            },"#,
        );
        let err = ProviderManifest::parse_str(&bad).unwrap_err();
        assert!(matches!(err, ProviderValidationError::DuplicateCodecId(_)));
    }
}
