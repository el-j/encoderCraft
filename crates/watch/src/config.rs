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
}

/// Configuration recipe for an automated watch folder daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolderConfig {
    pub id: String,
    pub watch_dir: PathBuf,
    pub output_dir: PathBuf,
    pub preset_name: String,
    pub post_action: PostProcessAction,
    pub settle_delay_ms: u64,
    pub enabled: bool,
}

impl WatchFolderConfig {
    pub fn new(id: impl Into<String>, watch_dir: PathBuf, output_dir: PathBuf, preset_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            watch_dir,
            output_dir,
            preset_name: preset_name.into(),
            post_action: PostProcessAction::LeaveInPlace,
            settle_delay_ms: 2000, // 2 second default settle delay
            enabled: true,
        }
    }
}
