//! Per-user settings files: preferences, watch folders and workspaces live in one config directory.

use encodercraft_watch::WatchFolderConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const WATCH_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct WatchFile {
    version: u32,
    folders: Vec<WatchFolderConfig>,
}

/// The per-user configuration directory, if the platform exposes one.
pub fn default_config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("encoderCraft"))
}

pub fn preferences_path(dir: &Path) -> PathBuf {
    dir.join("preferences.json")
}

pub fn workspaces_path(dir: &Path) -> PathBuf {
    dir.join("workspaces.json")
}

pub fn favorites_path(dir: &Path) -> PathBuf {
    dir.join("favorites.json")
}

/// Loads favourite folders, dropping duplicates. Missing or corrupt files give an empty list.
pub fn load_favorites(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut list: Vec<PathBuf> = serde_json::from_str(&text).unwrap_or_default();
    let mut seen = Vec::new();
    list.retain(|p| {
        let fresh = !seen.contains(p);
        seen.push(p.clone());
        fresh
    });
    list
}

pub fn save_favorites(path: &Path, favorites: &[PathBuf]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(favorites).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn watch_folders_path(dir: &Path) -> PathBuf {
    dir.join("watch_folders.json")
}

/// Loads saved watch folders. Missing or unreadable files yield an empty list; folders on
/// disconnected volumes are kept so they work again when the volume returns.
pub fn load_watch_folders(path: &Path) -> Vec<WatchFolderConfig> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    match serde_json::from_str::<WatchFile>(&text) {
        Ok(file) => {
            let mut seen = Vec::new();
            file.folders
                .into_iter()
                .filter(|f| {
                    let fresh = !f.id.is_empty() && !seen.contains(&f.id);
                    seen.push(f.id.clone());
                    fresh
                })
                .collect()
        }
        Err(e) => {
            log::warn!("ignoring unreadable watch folder list: {e}");
            Vec::new()
        }
    }
}

pub fn save_watch_folders(path: &Path, folders: &[WatchFolderConfig]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&WatchFile { version: WATCH_SCHEMA_VERSION, folders: folders.to_vec() }).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(id: &str) -> WatchFolderConfig {
        WatchFolderConfig::new(id, "/watch".into(), "/out".into(), "Preset")
    }

    #[test]
    fn watch_folders_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = watch_folders_path(dir.path());
        let list = vec![cfg("a"), cfg("b")];
        save_watch_folders(&path, &list).unwrap();
        assert_eq!(load_watch_folders(&path), list);
    }

    #[test]
    fn favorites_round_trip_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let path = favorites_path(dir.path());
        assert!(load_favorites(&path).is_empty());
        save_favorites(&path, &["/a".into(), "/b".into(), "/a".into()]).unwrap();
        assert_eq!(load_favorites(&path), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        std::fs::write(&path, "oops").unwrap();
        assert!(load_favorites(&path).is_empty());
    }

    #[test]
    fn corrupt_missing_and_duplicate_entries_are_handled() {
        let dir = tempfile::tempdir().unwrap();
        let path = watch_folders_path(dir.path());
        assert!(load_watch_folders(&path).is_empty());
        std::fs::write(&path, "{nope").unwrap();
        assert!(load_watch_folders(&path).is_empty());
        save_watch_folders(&path, &[cfg("a"), cfg("a"), cfg("")]).unwrap();
        assert_eq!(load_watch_folders(&path).len(), 1);
    }
}
