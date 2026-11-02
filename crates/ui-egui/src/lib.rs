//! egui desktop user interface for encoderCraft.
//!
//! Provides the primary Queue panel, Preset browser, Preview monitor,
//! Export Settings inspector, and ArtCraft theme tokens.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod app;
pub mod icons;
pub mod inspector;
pub mod log_dialog;
pub mod prefs_dialog;
pub mod preset_browser;
pub mod preset_editor;
pub mod preview_panel;
pub mod queue_panel;
pub mod theme;
pub mod watch_panel;

pub use app::EncoderCraftApp;
pub use preset_editor::{PresetEditorState, render_preset_editor};
pub use theme::{Brightness, Tokens, apply_theme};
pub use watch_panel::{WatchPanelState, render_watch_panel};
