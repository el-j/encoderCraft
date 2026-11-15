//! Codec provider registry and deterministic selection engine.
//!
//! Enforces ADR 0002: built-in codecs remain first-class and are never silently shadowed;
//! missing or disabled providers return explicit, typed errors.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use encodercraft_core::{ContainerType, DiscoveredProvider, ProviderManifest, ProviderOperation, ProviderOperationKind, discover_providers_in};
use encodercraft_video_codecs::VideoCodecKind;

/// Resolved target codec implementation ready for pipeline instantiation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedCodec {
    /// Built-in first-party pure-Rust codec engine.
    BuiltIn(VideoCodecKind),
    /// External out-of-process provider operation.
    External { provider_id: String, operation: Box<ProviderOperation>, executable_path: PathBuf },
}

/// Resolved target audio codec implementation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedAudioCodec {
    /// Built-in first-party audio codec (AAC, Opus, FLAC, PCM, MP3, Vorbis).
    BuiltIn(String),
    /// External provider audio encode operation.
    External { provider_id: String, operation: Box<ProviderOperation>, executable_path: PathBuf },
}

/// Resolved source decoder implementation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedDecoder {
    /// Built-in demux/decode pipeline.
    BuiltIn,
    /// External provider decode operation.
    External { provider_id: String, operation: Box<ProviderOperation>, executable_path: PathBuf },
}

/// Errors occurring during runtime codec resolution and provider selection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecResolutionError {
    #[error("requested codec '{0}' is not supported by built-in engines or any registered provider")]
    NoSuchCodec(String),

    #[error("codec provider '{0}' was not found in registered providers")]
    ProviderNotFound(String),

    #[error("codec provider '{provider_name}' ('{provider_id}') is installed but currently disabled; enable it explicitly to use")]
    ProviderDisabled { provider_id: String, provider_name: String },

    #[error("codec provider '{provider_id}' does not support operation {operation:?} for codec '{codec_id}'")]
    UnsupportedOperation { provider_id: String, codec_id: String, operation: ProviderOperationKind },

    #[error("codec '{codec_id}' cannot be muxed into requested container {container:?}")]
    IncompatibleContainer { codec_id: String, container: ContainerType },

    #[error(
        "ambiguous codec '{codec_id}' is declared by multiple enabled providers ({providers:?}); specify provider namespace ('<provider_id>:<codec_id>')"
    )]
    AmbiguousCodec { codec_id: String, providers: Vec<String> },
}

/// In-memory record of an installed or discovered provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRecord {
    pub manifest: ProviderManifest,
    pub root_dir: PathBuf,
    pub executable_path: PathBuf,
    pub enabled: bool,
    pub discovered_at: SystemTime,
}

/// Central runtime registry managing out-of-process codec providers.
#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderRecord>,
}

impl ProviderRegistry {
    /// Creates a new empty provider registry.
    pub fn new() -> Self {
        Self { providers: HashMap::new() }
    }

    /// Discovers providers across specified directories, registering valid manifests.
    /// Returns the number of newly discovered or updated providers.
    pub fn discover(&mut self, search_dirs: &[PathBuf]) -> usize {
        let mut count = 0;
        for dir in search_dirs {
            if !dir.is_dir() {
                continue;
            }
            let discovered = discover_providers_in(dir);
            for DiscoveredProvider { manifest, root_dir, executable_path } in discovered.into_iter().flatten() {
                let id = manifest.id.clone();
                if let Some(existing) = self.providers.get_mut(&id) {
                    existing.manifest = manifest;
                    existing.root_dir = root_dir;
                    existing.executable_path = executable_path;
                } else {
                    self.providers.insert(
                        id,
                        ProviderRecord {
                            manifest,
                            root_dir,
                            executable_path,
                            enabled: false, // Default to disabled per ADR 0002
                            discovered_at: SystemTime::now(),
                        },
                    );
                    count += 1;
                }
            }
        }
        count
    }

    /// Registers a validated provider record manually (e.g. from tests or dynamic install).
    pub fn register(&mut self, manifest: ProviderManifest, root_dir: PathBuf, executable_path: PathBuf) -> Result<(), String> {
        let id = manifest.id.clone();
        self.providers.insert(id, ProviderRecord { manifest, root_dir, executable_path, enabled: false, discovered_at: SystemTime::now() });
        Ok(())
    }

    /// Enables an installed provider by its identifier.
    pub fn enable(&mut self, provider_id: &str) -> Result<(), CodecResolutionError> {
        let record = self.providers.get_mut(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;
        record.enabled = true;
        Ok(())
    }

    /// Disables an installed provider by its identifier.
    pub fn disable(&mut self, provider_id: &str) -> Result<(), CodecResolutionError> {
        let record = self.providers.get_mut(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;
        record.enabled = false;
        Ok(())
    }

    /// Removes a provider from the registry. Returns its root directory for optional deletion.
    pub fn remove(&mut self, provider_id: &str) -> Result<PathBuf, CodecResolutionError> {
        let record = self.providers.remove(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;
        Ok(record.root_dir)
    }

    /// Gets a reference to an installed provider record.
    pub fn get(&self, provider_id: &str) -> Option<&ProviderRecord> {
        self.providers.get(provider_id)
    }

    /// Lists all registered providers.
    pub fn list(&self) -> Vec<&ProviderRecord> {
        let mut list: Vec<&ProviderRecord> = self.providers.values().collect();
        list.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        list
    }

    /// Synchronizes provider enabled states with persistent user preferences.
    pub fn sync_with_preferences(&mut self, enabled_set: &HashSet<String>) {
        for (id, record) in &mut self.providers {
            record.enabled = enabled_set.contains(id);
        }
    }

    /// Exports the current set of enabled provider identifiers for saving in user preferences.
    pub fn export_enabled_set(&self) -> HashSet<String> {
        self.providers.iter().filter_map(|(id, r)| if r.enabled { Some(id.clone()) } else { None }).collect()
    }

    /// Resolves a video codec request against built-ins and registered providers.
    ///
    /// Resolution rules:
    /// 1. Explicit namespace (`provider_id:codec_id`): routes to that provider; errors if disabled/missing/incompatible.
    /// 2. Generic string matching a built-in codec (`h264`, `hevc`, `prores`, `dnxhd`, `av1`, `vp9`, `mjpeg`, `gif`):
    ///    ALWAYS resolves to the built-in pure-Rust engine. Never silently shadowed.
    /// 3. Generic string matching an enabled external provider operation: resolves if unambiguous.
    /// 4. Container compatibility is strictly checked against declared container support.
    pub fn resolve_video_codec(&self, requested_codec: &str, target_container: ContainerType) -> Result<ResolvedCodec, CodecResolutionError> {
        let clean = requested_codec.trim();

        // Case 1: Explicit namespace `provider_id:codec_id`
        if let Some((provider_id, codec_id)) = clean.split_once(':') {
            let record = self.providers.get(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;

            if !record.enabled {
                return Err(CodecResolutionError::ProviderDisabled {
                    provider_id: provider_id.to_string(),
                    provider_name: record.manifest.name.clone(),
                });
            }

            let op = record
                .manifest
                .operations
                .iter()
                .find(|o| {
                    o.operation_kind == ProviderOperationKind::VideoEncode
                        && (o.codec_id.eq_ignore_ascii_case(codec_id) || o.aliases.iter().any(|a| a.eq_ignore_ascii_case(codec_id)))
                })
                .ok_or_else(|| CodecResolutionError::UnsupportedOperation {
                    provider_id: provider_id.to_string(),
                    codec_id: codec_id.to_string(),
                    operation: ProviderOperationKind::VideoEncode,
                })?;

            if !op.containers.is_empty() && !op.containers.contains(&target_container) {
                return Err(CodecResolutionError::IncompatibleContainer { codec_id: codec_id.to_string(), container: target_container });
            }

            return Ok(ResolvedCodec::External {
                provider_id: provider_id.to_string(),
                operation: Box::new(op.clone()),
                executable_path: record.executable_path.clone(),
            });
        }

        // Case 2: Built-in pure-Rust engines take absolute precedence
        if let Some(built_in) = VideoCodecKind::parse(clean) {
            return Ok(ResolvedCodec::BuiltIn(built_in));
        }

        // Case 3: Search enabled providers for matching codec_id or alias
        let mut matches = Vec::new();
        let mut disabled_matches = Vec::new();

        for record in self.providers.values() {
            for op in &record.manifest.operations {
                if op.operation_kind == ProviderOperationKind::VideoEncode
                    && (op.codec_id.eq_ignore_ascii_case(clean) || op.aliases.iter().any(|a| a.eq_ignore_ascii_case(clean)))
                {
                    if record.enabled {
                        matches.push((record, op));
                    } else {
                        disabled_matches.push(record);
                    }
                }
            }
        }

        if matches.len() > 1 {
            let providers = matches.iter().map(|(r, _)| r.manifest.id.clone()).collect();
            return Err(CodecResolutionError::AmbiguousCodec { codec_id: clean.to_string(), providers });
        }

        if let Some((record, op)) = matches.into_iter().next() {
            if !op.containers.is_empty() && !op.containers.contains(&target_container) {
                return Err(CodecResolutionError::IncompatibleContainer { codec_id: clean.to_string(), container: target_container });
            }

            return Ok(ResolvedCodec::External {
                provider_id: record.manifest.id.clone(),
                operation: Box::new(op.clone()),
                executable_path: record.executable_path.clone(),
            });
        }

        // If found in a disabled provider, give a clear actionable error
        if let Some(record) = disabled_matches.into_iter().next() {
            return Err(CodecResolutionError::ProviderDisabled {
                provider_id: record.manifest.id.clone(),
                provider_name: record.manifest.name.clone(),
            });
        }

        Err(CodecResolutionError::NoSuchCodec(clean.to_string()))
    }

    /// Resolves an audio codec request against built-ins and registered providers.
    pub fn resolve_audio_codec(&self, requested_codec: &str, target_container: ContainerType) -> Result<ResolvedAudioCodec, CodecResolutionError> {
        let clean = requested_codec.trim();

        // Case 1: Explicit namespace `provider_id:codec_id`
        if let Some((provider_id, codec_id)) = clean.split_once(':') {
            let record = self.providers.get(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;
            if !record.enabled {
                return Err(CodecResolutionError::ProviderDisabled {
                    provider_id: provider_id.to_string(),
                    provider_name: record.manifest.name.clone(),
                });
            }

            let op = record
                .manifest
                .operations
                .iter()
                .find(|o| {
                    o.operation_kind == ProviderOperationKind::AudioEncode
                        && (o.codec_id.eq_ignore_ascii_case(codec_id) || o.aliases.iter().any(|a| a.eq_ignore_ascii_case(codec_id)))
                })
                .ok_or_else(|| CodecResolutionError::UnsupportedOperation {
                    provider_id: provider_id.to_string(),
                    codec_id: codec_id.to_string(),
                    operation: ProviderOperationKind::AudioEncode,
                })?;

            if !op.containers.is_empty() && !op.containers.contains(&target_container) {
                return Err(CodecResolutionError::IncompatibleContainer { codec_id: codec_id.to_string(), container: target_container });
            }

            return Ok(ResolvedAudioCodec::External {
                provider_id: provider_id.to_string(),
                operation: Box::new(op.clone()),
                executable_path: record.executable_path.clone(),
            });
        }

        // Case 2: Built-in audio codecs take absolute precedence
        let lower = clean.to_ascii_lowercase();
        if matches!(lower.as_str(), "aac" | "mp4a" | "opus" | "flac" | "pcm_s16le" | "pcm_s24le" | "pcm_f32le" | "mp3" | "vorbis") {
            return Ok(ResolvedAudioCodec::BuiltIn(lower));
        }

        // Case 3: Search enabled providers for audio encoding
        let mut matches = Vec::new();
        let mut disabled_matches = Vec::new();

        for record in self.providers.values() {
            for op in &record.manifest.operations {
                if op.operation_kind == ProviderOperationKind::AudioEncode
                    && (op.codec_id.eq_ignore_ascii_case(clean) || op.aliases.iter().any(|a| a.eq_ignore_ascii_case(clean)))
                {
                    if record.enabled {
                        matches.push((record, op));
                    } else {
                        disabled_matches.push(record);
                    }
                }
            }
        }

        if matches.len() > 1 {
            let providers = matches.iter().map(|(r, _)| r.manifest.id.clone()).collect();
            return Err(CodecResolutionError::AmbiguousCodec { codec_id: clean.to_string(), providers });
        }

        if let Some((record, op)) = matches.into_iter().next() {
            if !op.containers.is_empty() && !op.containers.contains(&target_container) {
                return Err(CodecResolutionError::IncompatibleContainer { codec_id: clean.to_string(), container: target_container });
            }

            return Ok(ResolvedAudioCodec::External {
                provider_id: record.manifest.id.clone(),
                operation: Box::new(op.clone()),
                executable_path: record.executable_path.clone(),
            });
        }

        if let Some(record) = disabled_matches.into_iter().next() {
            return Err(CodecResolutionError::ProviderDisabled {
                provider_id: record.manifest.id.clone(),
                provider_name: record.manifest.name.clone(),
            });
        }

        Err(CodecResolutionError::NoSuchCodec(clean.to_string()))
    }

    /// Resolves a video or audio decoder request against built-ins and registered providers.
    pub fn resolve_decoder(&self, source_codec: &str, operation: ProviderOperationKind) -> Result<ResolvedDecoder, CodecResolutionError> {
        let clean = source_codec.trim();

        // Explicit namespace `provider_id:codec_id`
        if let Some((provider_id, codec_id)) = clean.split_once(':') {
            let record = self.providers.get(provider_id).ok_or_else(|| CodecResolutionError::ProviderNotFound(provider_id.to_string()))?;
            if !record.enabled {
                return Err(CodecResolutionError::ProviderDisabled {
                    provider_id: provider_id.to_string(),
                    provider_name: record.manifest.name.clone(),
                });
            }

            let op = record
                .manifest
                .operations
                .iter()
                .find(|o| {
                    o.operation_kind == operation
                        && (o.codec_id.eq_ignore_ascii_case(codec_id) || o.aliases.iter().any(|a| a.eq_ignore_ascii_case(codec_id)))
                })
                .ok_or_else(|| CodecResolutionError::UnsupportedOperation {
                    provider_id: provider_id.to_string(),
                    codec_id: codec_id.to_string(),
                    operation,
                })?;

            return Ok(ResolvedDecoder::External {
                provider_id: provider_id.to_string(),
                operation: Box::new(op.clone()),
                executable_path: record.executable_path.clone(),
            });
        }

        // Built-ins take precedence
        Ok(ResolvedDecoder::BuiltIn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_dummy_manifest(id: &str, name: &str, codec: &str, containers: Vec<ContainerType>) -> ProviderManifest {
        ProviderManifest {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            protocol_version: 1,
            encodercraft_version: None,
            description: None,
            author: None,
            license: "MIT".to_string(),
            homepage: None,
            executable: PathBuf::from("bin/worker"),
            operations: vec![ProviderOperation {
                operation_kind: ProviderOperationKind::VideoEncode,
                codec_id: codec.to_string(),
                display_name: format!("{name} Codec"),
                aliases: vec![],
                profiles: vec![],
                pixel_formats: vec![],
                sample_formats: vec![],
                sample_rates: vec![],
                containers,
                parameters: vec![],
            }],
        }
    }

    #[test]
    fn built_in_codecs_are_never_shadowed() {
        let mut registry = ProviderRegistry::new();
        let manifest = create_dummy_manifest("evil.corp", "Evil Corp", "h264", vec![ContainerType::IsoBmffMp4]);
        registry.register(manifest, PathBuf::from("/tmp"), PathBuf::from("/tmp/bin/worker")).unwrap();
        registry.enable("evil.corp").unwrap();

        // Requesting generic 'h264' must ALWAYS yield BuiltIn(H264), NOT Evil Corp!
        let resolved = registry.resolve_video_codec("h264", ContainerType::IsoBmffMp4).unwrap();
        assert_eq!(resolved, ResolvedCodec::BuiltIn(VideoCodecKind::H264));

        // It is reachable only when explicitly namespaced
        let explicit = registry.resolve_video_codec("evil.corp:h264", ContainerType::IsoBmffMp4).unwrap();
        match explicit {
            ResolvedCodec::External { provider_id, .. } => assert_eq!(provider_id, "evil.corp"),
            _ => panic!("expected external codec"),
        }
    }

    #[test]
    fn disabled_provider_returns_actionable_error() {
        let mut registry = ProviderRegistry::new();
        let manifest = create_dummy_manifest("org.svt.av1", "SVT-AV1", "svt-av1", vec![ContainerType::IsoBmffMp4]);
        registry.register(manifest, PathBuf::from("/tmp"), PathBuf::from("/tmp/bin/worker")).unwrap();

        let err = registry.resolve_video_codec("svt-av1", ContainerType::IsoBmffMp4).unwrap_err();
        assert!(matches!(err, CodecResolutionError::ProviderDisabled { .. }));
    }

    #[test]
    fn container_compatibility_is_enforced() {
        let mut registry = ProviderRegistry::new();
        let manifest = create_dummy_manifest("org.svt.av1", "SVT-AV1", "svt-av1", vec![ContainerType::IsoBmffMp4]);
        registry.register(manifest, PathBuf::from("/tmp"), PathBuf::from("/tmp/bin/worker")).unwrap();
        registry.enable("org.svt.av1").unwrap();

        // Compatible container
        let ok = registry.resolve_video_codec("svt-av1", ContainerType::IsoBmffMp4);
        assert!(ok.is_ok());

        // Incompatible container
        let err = registry.resolve_video_codec("svt-av1", ContainerType::QuickTimeMov).unwrap_err();
        assert!(matches!(err, CodecResolutionError::IncompatibleContainer { .. }));
    }
}
