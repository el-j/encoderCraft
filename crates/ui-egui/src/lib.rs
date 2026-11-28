//! egui desktop user interface for encoderCraft.
//!
//! Provides the primary Queue panel, Preset browser, Preview monitor,
//! Export Settings inspector, and ArtCraft theme tokens.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod app;
pub mod commands;
pub mod encoding_panel;
pub mod fonts;
pub mod icons;
pub mod inspector;
pub mod log_dialog;
pub mod media_browser;
pub mod media_model;
pub mod menus;
pub mod monitor_geom;
pub mod output_render;
pub mod prefs_dialog;
pub mod preset_browser;
pub mod preset_editor;
pub mod preview_decoder;
pub mod preview_panel;
pub mod queue_footer;
pub mod queue_panel;
pub mod scope_view;
pub mod settings;
pub mod source_properties;
pub mod stitch_dialog;
pub mod theme;
pub mod titlebar;
pub mod transport;
pub mod watch_panel;
pub mod workspace;

pub use app::EncoderCraftApp;
pub use preset_editor::{PresetEditorState, render_preset_editor};
pub use source_properties::{SourcePropertiesState, render_source_properties_dialog};
pub use stitch_dialog::{StitchDialogState, render_stitch_dialog};
pub use theme::{Brightness, Tokens, apply_theme};
pub use watch_panel::{WatchPanelState, render_watch_panel};

/// Installs the `craft-ui` runtime onto `ctx` (idempotent if already installed).
pub fn install_ui_runtime(ctx: &egui::Context) {
    let spec = craft_ui::InstallSpec::new(craft_host::target::ShellInfo::default(), std::sync::Arc::new(craft_window::system::NoWindows));
    let _ = craft_ui::install(ctx, spec);
}

/// Feeds the per-frame `craft-ui` runtime input hook before an `egui` frame begins, with the app's root values
/// ([`EncoderCraftApp::root_env`]).
pub fn prepare_ui_frame(ctx: &egui::Context, root: craft_ui::runtime::env::RootEnv) {
    craft_ui::shell::embed::before_input(ctx, |_| craft_ui::runtime::frame::FrameInput::new(root));
}

/// Renders one headless test frame with the `craft-ui` runtime installed.
pub fn run_test_ui(mut f: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    install_ui_runtime(&ctx);
    prepare_ui_frame(&ctx, craft_ui::runtime::env::RootEnv::default());
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| f(ui));
    output.textures_delta.clear();
}
