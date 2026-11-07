//! Watch folder configuration and post-processing action models.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Action taken on the source media file after transcode finishes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "action", content = "path", rename_all = "camelCase")]
pub enum PostProcessAction {
    /// Leaves the source file untouched in the watch folder.
    #[default]
    LeaveInPlace,
    /// Moves the source file to a completed/processed directory.
    MoveToProcessed(PathBuf),
    /// Deletes the source file upon successful transcode.
    DeleteSource,
    /// Moves failed sources to an error directory.
    MoveToErrors(PathBuf),
    /// Moves the source file to a timestamped subfolder inside an archive directory.
    MoveToArchive(PathBuf),
}

/// Output target specification within a watch folder configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchTargetConfig {
    pub preset_name: String,
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
}

impl WatchTargetConfig {
    pub fn new(preset_name: impl Into<String>, output_dir: Option<PathBuf>) -> Self {
        Self { preset_name: preset_name.into(), output_dir }
    }
}

/// Configuration recipe for an automated watch folder daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolderConfig {
    pub id: String,
    pub watch_dir: PathBuf,
    pub output_dir: PathBuf,
    pub preset_name: String,
    #[serde(default)]
    pub targets: Vec<WatchTargetConfig>,
    #[serde(default)]
    pub post_action: PostProcessAction,
    #[serde(default = "default_settle_delay")]
    pub settle_delay_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_settle_delay() -> u64 {
    2000
}

fn default_true() -> bool {
    true
}

impl WatchFolderConfig {
    pub fn new(id: impl Into<String>, watch_dir: PathBuf, output_dir: PathBuf, preset_name: impl Into<String>) -> Self {
        let preset_str = preset_name.into();
        Self {
            id: id.into(),
            watch_dir,
            output_dir,
            preset_name: preset_str.clone(),
            targets: vec![WatchTargetConfig::new(preset_str, None)],
            post_action: PostProcessAction::LeaveInPlace,
            settle_delay_ms: 2000, // 2 second default settle delay
            enabled: true,
        }
    }

    /// Adds an extra output target to this watch folder configuration.
    pub fn add_target(&mut self, preset_name: impl Into<String>, output_dir: Option<PathBuf>) {
        self.targets.push(WatchTargetConfig::new(preset_name, output_dir));
    }
}
