//! Canonical read/write filesystem root sandboxing for MCP automation (`AGENTS.md` Rule 18).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use encodercraft_core::EncoderError;
use serde_json::Value;

/// Canonical read and write directory boundaries enforced on all MCP tool calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRoots {
    read_root: PathBuf,
    write_root: PathBuf,
}

impl AutomationRoots {
    /// Creates an [`AutomationRoots`] sandbox after verifying that both directories exist.
    pub fn new(read_root: impl Into<PathBuf>, write_root: impl Into<PathBuf>) -> Result<Self, EncoderError> {
        let read_root = canonical_root_dir("automation read root", read_root.into())?;
        let write_root = canonical_root_dir("automation write root", write_root.into())?;
        Ok(Self { read_root, write_root })
    }

    /// Returns the canonicalized read root directory.
    pub fn read_root(&self) -> &Path {
        &self.read_root
    }

    /// Returns the canonicalized write root directory.
    pub fn write_root(&self) -> &Path {
        &self.write_root
    }

    /// Validates that `candidate` resolves within the configured read root.
    pub fn check_read(&self, candidate: &Path) -> Result<PathBuf, EncoderError> {
        resolve_within_root(&self.read_root, candidate, "read")
    }

    /// Validates that `candidate` resolves within the configured write root.
    pub fn check_write(&self, candidate: &Path) -> Result<PathBuf, EncoderError> {
        resolve_within_root(&self.write_root, candidate, "write")
    }

    /// Validates any filesystem paths carried in `encodercraft_execute_command` arguments.
    pub fn check_command_params(&self, command_id: &str, params: &Value) -> Result<(), EncoderError> {
        if command_id == "preset.export" {
            if let Some(path) = params.get("path").and_then(Value::as_str) {
                self.check_write(Path::new(path))?;
            }
            return Ok(());
        }
        if command_id == "watch.add_folder"
            && let Some(watch_dir) = params.get("watchDir").and_then(Value::as_str)
        {
            self.check_read(Path::new(watch_dir))?;
            let out = params.get("outputDir").and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(watch_dir).join("_encoded"));
            self.check_write(&out)?;
        }
        self.check_value_paths(params)
    }

    fn check_value_paths(&self, value: &Value) -> Result<(), EncoderError> {
        let Some(obj) = value.as_object() else {
            return Ok(());
        };
        for (key, val) in obj {
            let lower = key.to_ascii_lowercase();
            if is_write_key(&lower) {
                self.check_json_paths(val, true)?;
            } else if is_read_key(&lower) {
                self.check_json_paths(val, false)?;
            } else if let Some(arr) = val.as_array() {
                for item in arr {
                    self.check_value_paths(item)?;
                }
            } else if val.is_object() {
                self.check_value_paths(val)?;
            }
        }
        Ok(())
    }

    fn check_json_paths(&self, value: &Value, is_write: bool) -> Result<(), EncoderError> {
        match value {
            Value::String(raw) => {
                if !raw.is_empty() {
                    if is_write {
                        self.check_write(Path::new(raw))?;
                    } else {
                        self.check_read(Path::new(raw))?;
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.check_json_paths(item, is_write)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn is_write_key(lower: &str) -> bool {
    matches!(lower, "output_path" | "outputpath" | "output_dir" | "outputdir" | "archive_dir" | "archivedir")
}

fn is_read_key(lower: &str) -> bool {
    matches!(
        lower,
        "path"
            | "source_path"
            | "sourcepath"
            | "source_paths"
            | "sourcepaths"
            | "clip_paths"
            | "clippaths"
            | "paths"
            | "favorites"
            | "watch_dir"
            | "watchdir"
            | "lut_path"
            | "lutpath"
            | "caption_path"
            | "captionpath"
            | "icc_profile_path"
            | "iccprofilepath"
            | "project_path"
            | "projectpath"
            | "host_cli_path"
            | "hostclipath"
    )
}

fn canonical_root_dir(label: &str, path: PathBuf) -> Result<PathBuf, EncoderError> {
    if path.as_os_str().is_empty() {
        return Err(EncoderError::InvalidJobSpec(format!("{label} cannot be empty")));
    }
    let canonical = std::fs::canonicalize(&path).map_err(|e| EncoderError::InvalidJobSpec(format!("invalid {label} '{}': {e}", path.display())))?;
    if !canonical.is_dir() {
        return Err(EncoderError::InvalidJobSpec(format!("{label} '{}' is not a directory", canonical.display())));
    }
    Ok(canonical)
}

fn resolve_within_root(root: &Path, candidate: &Path, access: &str) -> Result<PathBuf, EncoderError> {
    let raw = candidate.to_string_lossy();
    if candidate.as_os_str().is_empty() || raw.contains('\0') {
        return Err(EncoderError::InvalidJobSpec(format!("invalid empty or null path for MCP {access} access")));
    }
    if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(EncoderError::InvalidJobSpec(format!(
            "refusing MCP {access} access with '..' traversal outside '{}': '{}'",
            root.display(),
            candidate.display()
        )));
    }

    let anchored = if candidate.is_absolute() { candidate.to_path_buf() } else { root.join(candidate) };
    let mut probe = anchored.clone();
    let mut tail: Vec<OsString> = Vec::new();

    while std::fs::symlink_metadata(&probe).is_err() {
        let Some(name) = probe.file_name().map(ToOwned::to_owned) else {
            return Err(EncoderError::InvalidJobSpec(format!("MCP {access} path '{}' is outside '{}'", candidate.display(), root.display())));
        };
        tail.push(name);
        if !probe.pop() {
            return Err(EncoderError::InvalidJobSpec(format!("MCP {access} path '{}' is outside '{}'", candidate.display(), root.display())));
        }
    }

    let mut resolved = std::fs::canonicalize(&probe).map_err(|e| {
        EncoderError::InvalidJobSpec(format!("MCP {access} path '{}' cannot be resolved inside '{}': {e}", candidate.display(), root.display()))
    })?;
    for segment in tail.iter().rev() {
        resolved.push(segment);
    }

    if !resolved.starts_with(root) {
        return Err(EncoderError::InvalidJobSpec(format!(
            "MCP {access} path '{}' resolves outside allowed root '{}'",
            candidate.display(),
            root.display()
        )));
    }
    Ok(resolved)
}
