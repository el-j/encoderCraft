//! encoderCraft Desktop Application Root State & egui View.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use craft_ui::docking::{Action as DockAction, DockArea, DockContent, DockStyle};
use egui::{RichText, Ui};
use encodercraft_core::CraftJobSpec;
use encodercraft_engine::{LossyConversion, LossyConversionKey, QueueManager};
use encodercraft_presets::PresetRegistry;

use crate::icons;
use crate::inspector::{InspectorState, render_inspector_modal};
use crate::log_dialog::{LogDialogState, render_log_dialog};
use crate::prefs_dialog::{PrefsDialogState, render_prefs_dialog};
use crate::preset_browser::{PresetBrowserState, render_preset_browser};
use crate::preview_panel::{PreviewState, render_preview_panel};
use crate::queue_panel::{QueuePanelState, render_queue_panel};
use crate::theme::{Brightness, Tokens, apply_theme};
use crate::watch_panel::{WatchPanelState, render_watch_panel, trigger_watch_scan};

/// The root egui application for encoderCraft.
pub struct EncoderCraftApp {
    pub queue_mgr: Arc<QueueManager>,
    pub preset_registry: Arc<Mutex<PresetRegistry>>,
    engine_commands: encodercraft_engine::cmd::CommandRegistry,
    pub tokens: Tokens,
    /// Who draws the window frame; the menu bar doubles as the title bar.
    pub title_bar: crate::titlebar::TitleBarStyle,
    pub brightness: Brightness,
    pub queue_state: QueuePanelState,
    pub shutdown_watch: crate::queue_footer::ShutdownWatch,
    pub shutdown_error: Option<String>,
    pub watch_state: WatchPanelState,
    pub media_state: crate::media_browser::MediaBrowserState,
    pending_media_item: Option<std::path::PathBuf>,
    pub preset_state: PresetBrowserState,
    pub preview_state: PreviewState,
    pub encoding_state: crate::encoding_panel::EncodingPanelState,
    pub inspector_state: InspectorState,
    pub log_state: LogDialogState,
    pub prefs_state: PrefsDialogState,
    pub show_about_dialog: bool,
    /// The workspace name dialog (Save As or Rename) is open for this command.
    name_dialog: Option<crate::commands::AppCommand>,
    /// Set by the Quit command; the next frame asks the window to close.
    quit_requested: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<craft_menu::NativeMenu>,
    pub last_watch_tick: Option<std::time::Instant>,
    pub was_worker_running: bool,
    pub completion_banner_timer: Option<std::time::Instant>,
    pub workspaces: crate::workspace::WorkspaceSet,
    pub workspace_shortcuts: crate::settings::WorkspaceShortcuts,
    pub focused_panel: crate::workspace::PanelId,
    config_dir: Option<std::path::PathBuf>,
    last_saved_prefs: Option<encodercraft_engine::AppPreferences>,
    last_saved_shortcuts: Option<crate::settings::WorkspaceShortcuts>,
    last_saved_watch: Option<Vec<encodercraft_watch::WatchFolderConfig>>,
    last_saved_favorites: Option<Vec<std::path::PathBuf>>,
    workspace_name_input: String,
    last_saved_workspaces: Option<crate::workspace::WorkspaceSet>,
    fonts_installed: bool,
    pending_conversion_approvals: VecDeque<PendingConversionApproval>,
    conversion_status: Option<String>,
}

#[derive(Clone)]
struct PendingConversionApproval {
    existing_job_id: Option<String>,
    spec: CraftJobSpec,
    conversions: Vec<LossyConversion>,
    remember_choice: bool,
}

impl EncoderCraftApp {
    /// Creates a new encoderCraft application instance.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        let brightness = Brightness::Dark;
        let tokens = Tokens::for_brightness(brightness);

        Self {
            queue_mgr,
            engine_commands: encodercraft_engine::cmd::CommandRegistry::with_preset_registry(Arc::clone(&preset_registry)),
            preset_registry,
            tokens,
            title_bar: crate::titlebar::TitleBarStyle::default(),
            brightness,
            queue_state: QueuePanelState::default(),
            shutdown_watch: crate::queue_footer::ShutdownWatch::default(),
            shutdown_error: None,
            watch_state: WatchPanelState::default(),
            media_state: crate::media_browser::MediaBrowserState::default(),
            pending_media_item: None,
            preset_state: PresetBrowserState::default(),
            preview_state: PreviewState::default(),
            encoding_state: crate::encoding_panel::EncodingPanelState::default(),
            inspector_state: InspectorState::default(),
            log_state: LogDialogState::default(),
            prefs_state: PrefsDialogState::default(),
            show_about_dialog: false,
            name_dialog: None,
            quit_requested: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            last_watch_tick: None,
            was_worker_running: false,
            completion_banner_timer: None,
            workspaces: crate::workspace::WorkspaceSet::default(),
            workspace_shortcuts: crate::settings::WorkspaceShortcuts::default(),
            focused_panel: crate::workspace::PanelId::Queue,
            config_dir: None,
            last_saved_prefs: None,
            last_saved_shortcuts: None,
            last_saved_watch: None,
            last_saved_favorites: None,
            workspace_name_input: String::new(),
            last_saved_workspaces: None,
            fonts_installed: false,
            pending_conversion_approvals: VecDeque::new(),
            conversion_status: None,
        }
    }

    /// Chooses who draws the window frame (decided by the desktop runner before the window opens).
    pub fn with_title_bar(mut self, style: crate::titlebar::TitleBarStyle) -> Self {
        self.title_bar = style;
        self
    }

    /// Loads preferences, watch folders and workspaces from `dir` and keeps saving changes back.
    pub fn with_config_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.queue_mgr.set_conversion_preferences_path(crate::settings::preferences_path(&dir));
        if let Some(value) = self.dispatch_engine_command_value("workspace.get", serde_json::json!({}))
            && !value.is_null()
        {
            self.workspaces = crate::workspace::WorkspaceSet::from_json(&value.to_string());
        }
        self.last_saved_workspaces = Some(self.workspaces.clone());
        if let Some(value) = self.dispatch_engine_command_value("keyboard_shortcuts.get", serde_json::json!({}))
            && !value.is_null()
        {
            self.workspace_shortcuts = crate::settings::WorkspaceShortcuts::from_json(&value.to_string());
        }
        self.last_saved_shortcuts = Some(self.workspace_shortcuts.clone());
        if let Some(value) = self.dispatch_engine_command_value("preferences.get", serde_json::json!({})) {
            match serde_json::from_value(value) {
                Ok(prefs) => self.prefs_state.prefs = prefs,
                Err(error) => {
                    log::error!("engine returned invalid preferences: {error}");
                    self.conversion_status = Some(format!("Could not load preferences: {error}"));
                }
            }
        }
        self.last_saved_prefs = Some(self.prefs_state.prefs.clone());
        let brightness = Self::brightness_from_theme(&self.prefs_state.prefs.theme);
        self.set_brightness(brightness);
        if let Some(value) = self.dispatch_engine_command_value("watch.list_folders", serde_json::json!({})) {
            match serde_json::from_value(value) {
                Ok(configs) => self.watch_state.configs = configs,
                Err(error) => {
                    log::error!("engine returned invalid watch folders: {error}");
                    self.conversion_status = Some(format!("Could not load watch folders: {error}"));
                }
            }
        }
        self.last_saved_watch = Some(self.watch_state.configs.clone());
        if let Some(value) = self.dispatch_engine_command_value("media.list_favorites", serde_json::json!({})) {
            match serde_json::from_value(value) {
                Ok(favorites) => self.media_state.model.favorites = favorites,
                Err(error) => {
                    log::error!("engine returned invalid favorites: {error}");
                    self.conversion_status = Some(format!("Could not load favorites: {error}"));
                }
            }
        }
        self.last_saved_favorites = Some(self.media_state.model.favorites.clone());
        self.dispatch_engine_command("queue.set_parallelism", serde_json::json!({ "workers": self.prefs_state.prefs.max_parallel_jobs }));
        self.config_dir = Some(dir);
        self
    }

    fn command_state(&self) -> crate::commands::CommandState {
        let jobs = self.queue_mgr.list_jobs();
        let worker_running = self.queue_mgr.is_worker_running();
        let preset_deletable =
            self.preset_state.selected_preset.as_ref().is_some_and(|preset| preset.category == encodercraft_presets::PresetCategory::Custom);
        crate::commands::CommandState {
            selected: self.queue_state.selected_job_ids.len(),
            jobs: jobs.len(),
            finished: jobs.iter().filter(|j| j.status == encodercraft_core::JobStatus::Completed).count(),
            worker_running,
            worker_active: worker_running && !self.queue_mgr.is_paused(),
            preset_selected: self.preset_state.selected_preset.is_some(),
            preset_deletable,
            has_source: false,
            has_output: false,
        }
    }

    fn pick_media(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Select Media Files")
            .add_filter(
                "Media Files",
                &[
                    "mov", "mp4", "mkv", "avi", "webm", "mxf", "wav", "aiff", "flac", "mp3", "ogg", "opus", "png", "jpg", "jpeg", "webp", "exr",
                    "tiff", "bmp", "gif", "svg", "pdf", "docx", "idml", "gltf", "glb", "usdz", "obj", "stl", "ply",
                ],
            )
            .pick_files()
        {
            for path in files {
                self.add_file_job(path);
            }
        }
    }

    /// Runs a command from a menu, shortcut or context menu. All entry points share this.
    pub fn run_command(&mut self, cmd: crate::commands::AppCommand) {
        use crate::commands::AppCommand as C;
        if !cmd.enabled(&self.command_state()) {
            return;
        }
        let ids = self.queue_state.selected_ids_list();
        match cmd {
            C::AddMedia => self.pick_media(),
            C::AddMediaItem => {
                if let Some(path) = self.pending_media_item.take() {
                    self.add_file_job(path);
                } else {
                    self.conversion_status = Some("No media item is available to add.".into());
                }
            }
            C::AddStitched => {
                if let Some(files) = rfd::FileDialog::new().set_title("Select Clips to Stitch Together").pick_files()
                    && files.len() > 1
                    && let Some(first) = files.first()
                {
                    let out = first.with_file_name("stitched_sequence.mp4");
                    if let Some(spec) = self.dispatch_prepared_app_job_spec(
                        C::AddStitched,
                        serde_json::json!({
                            "paths": files,
                            "outputPath": out,
                            "presetName": "Match Source - High Bitrate",
                        }),
                    ) {
                        self.submit_job_for_review(spec);
                    }
                }
            }
            C::AddWatchFolder => {
                if let Some(watch_dir) = rfd::FileDialog::new().set_title("Select Directory to Watch").pick_folder() {
                    if let Some(value) = self.dispatch_app_command_value(C::AddWatchFolder, serde_json::json!({ "watchDir": watch_dir })) {
                        match serde_json::from_value::<encodercraft_watch::WatchFolderConfig>(value) {
                            Ok(config) => self.watch_state.configs.push(config),
                            Err(error) => self.conversion_status = Some(format!("Could not add watch folder: {error}")),
                        }
                    }
                    if self.workspaces.current().is_some() {
                        self.set_workspace_panel_visible(crate::workspace::PanelId::WatchFolders, true);
                    }
                }
            }
            C::ShowLog => {
                if self.dispatch_app_command_value(C::ShowLog, serde_json::json!({})).is_some() {
                    self.log_state.is_open = !self.log_state.is_open;
                }
            }
            C::Preferences => {
                if let Some(value) = self.dispatch_app_command_value(C::Preferences, serde_json::json!({})) {
                    match serde_json::from_value(value) {
                        Ok(prefs) => self.prefs_state.prefs = prefs,
                        Err(error) => self.conversion_status = Some(format!("Could not load preferences: {error}")),
                    }
                }
                self.prefs_state.is_open = !self.prefs_state.is_open;
            }
            C::ThemeDark => self.set_theme(C::ThemeDark, "Dark", Brightness::Dark),
            C::ThemeLight => self.set_theme(C::ThemeLight, "Light", Brightness::Light),
            C::ThemeHighContrast => self.set_theme(C::ThemeHighContrast, "High Contrast", Brightness::HighContrast),
            C::StartQueue
            | C::PauseQueue
            | C::StopQueue
            | C::ResetSelected
            | C::DuplicateSelected
            | C::RemoveSelected
            | C::MoveSelectedUp
            | C::MoveSelectedDown
            | C::SelectAll
            | C::StitchSelected
            | C::SetDestination
            | C::ClearFinished
            | C::AddOutputTarget
            | C::InspectSettings => self.run_queue_app_command(cmd, ids),
            C::CreatePreset => self.preset_state.editor.open_new(),
            C::ImportPresets => {
                if let Some(path) = rfd::FileDialog::new().set_title("Import Presets").add_filter("Preset bundle", &["json"]).pick_file()
                    && let Some(result) = self.dispatch_app_command_value(C::ImportPresets, serde_json::json!({ "path": path }))
                {
                    match result.get("importedCount").and_then(serde_json::Value::as_u64) {
                        Some(count) => self.preset_state.status = Some(format!("Imported {count} preset(s)")),
                        None => self.preset_state.status = Some("Preset import returned no imported count.".into()),
                    }
                }
            }
            C::ExportPresets => {
                if let Some(path) = rfd::FileDialog::new().set_title("Export Presets").set_file_name("encodercraft-presets.json").save_file()
                    && let Some(result) = self.dispatch_app_command_value(C::ExportPresets, serde_json::json!({ "path": path }))
                {
                    match result.get("exportedCount").and_then(serde_json::Value::as_u64) {
                        Some(count) => self.preset_state.status = Some(format!("Exported {count} preset(s)")),
                        None => self.preset_state.status = Some("Preset export returned no exported count.".into()),
                    }
                }
            }
            C::ApplyPreset | C::AddPresetTarget => {
                let Some(preset) = self.preset_state.selected_preset.clone() else { return };
                if crate::preset_browser::preset_incompatible_with_jobs(&preset, &ids, &self.queue_mgr) {
                    self.preset_state.status = Some("Preset is incompatible with the selected source media.".into());
                    return;
                }
                for id in &ids {
                    if let Some(spec) = self.dispatch_prepared_app_job_spec(cmd, serde_json::json!({ "jobId": id, "presetName": preset.name })) {
                        self.preset_state.pending_updates.push((id.clone(), spec));
                    }
                }
            }
            C::DeletePreset => {
                let Some(preset_name) = self.preset_state.selected_preset.as_ref().map(|preset| preset.name.clone()) else { return };
                if self.dispatch_app_command_value(C::DeletePreset, serde_json::json!({ "name": preset_name })).is_some() {
                    self.preset_state.selected_preset = None;
                    self.preset_state.status = Some(format!("Deleted '{preset_name}'"));
                }
            }
            C::FocusNextPanel
            | C::FocusPreviousPanel
            | C::ToggleFocusedPanel
            | C::ActivateWorkspace
            | C::SaveWorkspaceAs
            | C::RenameWorkspace
            | C::ResetWorkspace
            | C::DeleteWorkspace
            | C::SetPanelVisibility => self.run_workspace_app_command(cmd),
            C::Quit => {
                if self.dispatch_app_command_value(C::Quit, serde_json::json!({})).is_some() {
                    self.quit_requested = true;
                }
            }
            C::About => {
                if self.dispatch_app_command_value(C::About, serde_json::json!({})).is_some() {
                    self.show_about_dialog = true;
                }
            }
            C::RevealOutput | C::CopyOutputPath | C::RevealSource | C::CopySourcePath | C::ExportSettings => {
                self.conversion_status = Some("This action requires a queue-row context menu.".into());
            }
            C::OpenMediaFolder | C::ToggleMediaFavorite => {
                self.conversion_status = Some("This action requires a media-item context menu.".into());
            }
        }
    }

    fn run_queue_app_command(&mut self, command: crate::commands::AppCommand, ids: Vec<String>) {
        use crate::commands::AppCommand as C;
        match command {
            C::StartQueue | C::PauseQueue | C::StopQueue | C::ClearFinished => {
                self.dispatch_app_command_value(command, serde_json::json!({}));
            }
            C::ResetSelected | C::RemoveSelected | C::MoveSelectedUp => {
                for id in ids {
                    self.dispatch_app_command_value(command, serde_json::json!({ "jobId": id }));
                }
            }
            C::MoveSelectedDown => {
                for id in ids.into_iter().rev() {
                    self.dispatch_app_command_value(command, serde_json::json!({ "jobId": id }));
                }
            }
            C::DuplicateSelected => {
                for id in ids {
                    if let Some(spec) = self.dispatch_prepared_app_job_spec(command, serde_json::json!({ "jobId": id })) {
                        self.submit_job_for_review(spec);
                    }
                }
            }
            C::AddOutputTarget => {
                for id in ids {
                    if let Some(spec) = self.dispatch_prepared_app_job_spec(command, serde_json::json!({ "jobId": id })) {
                        self.queue_state.pending_updates.push((id, spec));
                    }
                }
            }
            C::InspectSettings => {
                if let Some(id) = ids.first()
                    && let Some(spec) = self.dispatch_prepared_app_job_spec(command, serde_json::json!({ "jobId": id }))
                {
                    self.inspector_state.job_spec = Some(spec);
                    self.inspector_state.is_open = true;
                }
            }
            C::SelectAll => {
                if let Some(value) = self.dispatch_app_command_value(command, serde_json::json!({})) {
                    let parsed = value
                        .get("jobIds")
                        .cloned()
                        .ok_or_else(|| "Select All returned no jobIds field.".to_owned())
                        .and_then(|job_ids| serde_json::from_value::<Vec<String>>(job_ids).map_err(|error| error.to_string()));
                    match parsed {
                        Ok(job_ids) => {
                            self.queue_state.selected_job_ids = job_ids.into_iter().collect();
                            self.queue_state.selected_job_id = self.queue_state.selected_job_ids.iter().next().cloned();
                        }
                        Err(error) => self.conversion_status = Some(format!("Could not apply Select All result: {error}")),
                    }
                }
            }
            C::StitchSelected => {
                let output_path = std::env::temp_dir().join("stitched_batch.mp4");
                if let Some(spec) = self.dispatch_prepared_app_job_spec(
                    command,
                    serde_json::json!({ "jobIds": ids, "outputPath": output_path, "presetName": "Match Source - High Bitrate" }),
                ) {
                    self.submit_job_for_review(spec);
                }
            }
            C::SetDestination => {
                if let Some(folder) = rfd::FileDialog::new().set_title("Select Output Directory").pick_folder() {
                    self.dispatch_app_command_value(command, serde_json::json!({ "jobIds": ids, "outputDir": folder }));
                }
            }
            _ => self.conversion_status = Some(format!("Command {command:?} is not a queue action.")),
        }
    }

    fn run_workspace_app_command(&mut self, command: crate::commands::AppCommand) {
        use crate::commands::AppCommand as C;
        match command {
            C::FocusNextPanel => self.run_workspace_action(crate::settings::WorkspaceShortcutAction::FocusNext),
            C::FocusPreviousPanel => self.run_workspace_action(crate::settings::WorkspaceShortcutAction::FocusPrevious),
            C::ToggleFocusedPanel => self.run_workspace_action(crate::settings::WorkspaceShortcutAction::ToggleFocusedPanel),
            C::ActivateWorkspace => {
                let name = self.workspace_name_input.clone();
                if self.dispatch_workspace_mutation(command, serde_json::json!({ "name": name })).is_some() {
                    self.workspace_name_input.clear();
                    self.reconcile_focused_panel();
                }
            }
            C::SaveWorkspaceAs | C::RenameWorkspace => {
                let name = self.workspace_name_input.clone();
                if self.dispatch_workspace_mutation(command, serde_json::json!({ "name": name })).is_some() {
                    self.workspace_name_input.clear();
                    self.reconcile_focused_panel();
                }
            }
            C::ResetWorkspace => {
                if let Some(default_workspace) = self.default_workspace_for_reset()
                    && self.dispatch_workspace_mutation(command, serde_json::json!({ "defaultWorkspace": default_workspace })).is_some()
                {
                    self.reconcile_focused_panel();
                }
            }
            C::DeleteWorkspace => {
                if self.dispatch_workspace_mutation(command, serde_json::json!({})).is_some() {
                    self.reconcile_focused_panel();
                }
            }
            C::SetPanelVisibility => {
                let visible = self.workspaces.current().is_some_and(|workspace| !workspace.is_visible(self.focused_panel));
                self.set_workspace_panel_visible(self.focused_panel, visible);
            }
            _ => self.conversion_status = Some(format!("Command {command:?} is not a workspace action.")),
        }
    }

    fn dispatch_engine_command(&mut self, command_id: &str, params: serde_json::Value) {
        self.dispatch_engine_command_value(command_id, params);
    }

    fn dispatch_engine_command_value(&mut self, command_id: &str, params: serde_json::Value) -> Option<serde_json::Value> {
        match self.engine_commands.dispatch(&self.queue_mgr, command_id, params) {
            Ok(value) => Some(value),
            Err(error) => {
                self.conversion_status = Some(format!("Command '{command_id}' failed: {error}"));
                None
            }
        }
    }

    fn dispatch_app_command_value(&mut self, command: crate::commands::AppCommand, params: serde_json::Value) -> Option<serde_json::Value> {
        let Some(info) = command.info() else {
            self.conversion_status = Some(format!("Application command {command:?} is not registered."));
            return None;
        };
        let Some(engine_id) = info.engine_id else {
            self.conversion_status = Some(format!("Command '{}' has no engine binding.", info.id));
            return None;
        };
        self.dispatch_engine_command_value(engine_id, params)
    }

    fn dispatch_workspace_mutation(&mut self, command: crate::commands::AppCommand, mut params: serde_json::Value) -> Option<serde_json::Value> {
        let workspaces = match serde_json::to_value(&self.workspaces) {
            Ok(value) => value,
            Err(error) => {
                self.conversion_status = Some(format!("Could not encode workspaces: {error}"));
                return None;
            }
        };
        params["workspaces"] = workspaces;
        let value = self.dispatch_app_command_value(command, params)?;
        match serde_json::from_value(value.clone()) {
            Ok(workspaces) => {
                self.workspaces = workspaces;
                self.last_saved_workspaces = Some(self.workspaces.clone());
                Some(value)
            }
            Err(error) => {
                self.conversion_status = Some(format!("Workspace command returned invalid data: {error}"));
                None
            }
        }
    }

    fn dispatch_prepared_app_job_spec(&mut self, command: crate::commands::AppCommand, params: serde_json::Value) -> Option<CraftJobSpec> {
        let value = self.dispatch_app_command_value(command, params)?;
        match serde_json::from_value(value) {
            Ok(spec) => Some(spec),
            Err(error) => {
                let id = command.info().map_or("unknown", |info| info.id);
                self.conversion_status = Some(format!("Command '{id}' returned an invalid job specification: {error}"));
                None
            }
        }
    }

    fn default_workspace_for_reset(&mut self) -> Option<serde_json::Value> {
        let active = &self.workspaces.active;
        let name = if matches!(
            active.as_str(),
            crate::workspace::DEFAULT_WORKSPACE | crate::workspace::QUEUE_WORKSPACE | crate::workspace::PREVIEW_WORKSPACE
        ) {
            active.as_str()
        } else {
            crate::workspace::DEFAULT_WORKSPACE
        };
        let defaults = crate::workspace::WorkspaceSet::default();
        match defaults.workspaces.into_iter().find(|workspace| workspace.name == name) {
            Some(workspace) => match serde_json::to_value(workspace) {
                Ok(value) => Some(value),
                Err(error) => {
                    self.conversion_status = Some(format!("Could not encode the default workspace: {error}"));
                    None
                }
            },
            None => {
                self.conversion_status = Some(format!("No default layout is available for workspace '{name}'."));
                None
            }
        }
    }

    fn set_brightness(&mut self, brightness: Brightness) {
        self.prefs_state.prefs.theme = match brightness {
            Brightness::Dark => "Dark",
            Brightness::MediumDark => "Medium Dark",
            Brightness::Light => "Light",
            Brightness::HighContrast => "High Contrast",
        }
        .into();
        self.brightness = brightness;
        self.tokens = Tokens::for_brightness(brightness);
    }

    fn set_theme(&mut self, command: crate::commands::AppCommand, theme: &str, brightness: Brightness) {
        if self.dispatch_app_command_value(command, serde_json::json!({ "theme": theme })).is_some() {
            self.set_brightness(brightness);
        }
    }

    fn brightness_from_theme(theme: &str) -> Brightness {
        match theme {
            "Medium Dark" => Brightness::MediumDark,
            "Light" => Brightness::Light,
            "High Contrast" => Brightness::HighContrast,
            _ => Brightness::Dark,
        }
    }

    fn reconcile_focused_panel(&mut self) {
        if self.workspaces.current().is_some_and(|workspace| !workspace.is_visible(self.focused_panel)) {
            self.focused_panel = self
                .workspaces
                .current()
                .and_then(|workspace| crate::workspace::PanelId::ALL.into_iter().find(|panel| workspace.is_visible(*panel)))
                .unwrap_or(crate::workspace::PanelId::Queue);
        }
    }

    fn handle_workspace_shortcuts(&mut self, ctx: &egui::Context) {
        if self.prefs_state.is_open || self.prefs_state.shortcut_recording.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event else { return None };
                crate::settings::WorkspaceShortcutAction::ALL
                    .into_iter()
                    .find(|action| self.workspace_shortcuts.matches(*action, *key, modifiers))
                    .map(|action| (action, *key, *modifiers))
            })
        });
        let Some((action, key, modifiers)) = pressed else { return };
        ctx.input_mut(|input| {
            let _ = input.consume_key(modifiers, key);
        });

        let command = match action {
            crate::settings::WorkspaceShortcutAction::FocusNext => crate::commands::AppCommand::FocusNextPanel,
            crate::settings::WorkspaceShortcutAction::FocusPrevious => crate::commands::AppCommand::FocusPreviousPanel,
            crate::settings::WorkspaceShortcutAction::ToggleFocusedPanel => crate::commands::AppCommand::ToggleFocusedPanel,
        };
        self.run_command(command);
    }

    fn run_workspace_action(&mut self, action: crate::settings::WorkspaceShortcutAction) {
        let command = match action {
            crate::settings::WorkspaceShortcutAction::FocusNext => crate::commands::AppCommand::FocusNextPanel,
            crate::settings::WorkspaceShortcutAction::FocusPrevious => crate::commands::AppCommand::FocusPreviousPanel,
            crate::settings::WorkspaceShortcutAction::ToggleFocusedPanel => crate::commands::AppCommand::ToggleFocusedPanel,
        };
        let visible_panels = self.visible_panel_values();
        let current_panel = match serde_json::to_value(self.focused_panel) {
            Ok(value) => value,
            Err(error) => {
                self.conversion_status = Some(format!("Could not encode focused panel: {error}"));
                return;
            }
        };
        let Some(result) =
            self.dispatch_app_command_value(command, serde_json::json!({ "currentPanel": current_panel, "visiblePanels": visible_panels }))
        else {
            return;
        };
        if action == crate::settings::WorkspaceShortcutAction::ToggleFocusedPanel {
            match result.get("visible").and_then(serde_json::Value::as_bool) {
                Some(visible) => self.set_workspace_panel_visible(self.focused_panel, visible),
                None => {
                    self.conversion_status =
                        Some(format!("Command '{}' returned no visibility state.", command.info().map_or("unknown", |info| info.id)))
                }
            }
        } else if let Some(panel) = result.get("panel").cloned().and_then(|value| serde_json::from_value(value).ok()) {
            self.focused_panel = panel;
        } else {
            self.conversion_status =
                Some(format!("Command '{}' returned an invalid focused panel.", command.info().map_or("unknown", |info| info.id)));
        }
    }

    fn visible_panel_values(&self) -> Vec<serde_json::Value> {
        self.workspaces
            .current()
            .into_iter()
            .flat_map(|workspace| crate::workspace::PanelId::ALL.into_iter().filter(|panel| workspace.is_visible(*panel)))
            .filter_map(|panel| serde_json::to_value(panel).ok())
            .collect()
    }

    fn set_workspace_panel_visible(&mut self, panel: crate::workspace::PanelId, visible: bool) {
        let panel_value = match serde_json::to_value(panel) {
            Ok(value) => value,
            Err(error) => {
                self.conversion_status = Some(format!("Could not encode panel visibility: {error}"));
                return;
            }
        };
        let Some(result) = self.dispatch_app_command_value(
            crate::commands::AppCommand::SetPanelVisibility,
            serde_json::json!({ "panel": panel_value, "visible": visible, "visiblePanels": self.visible_panel_values() }),
        ) else {
            return;
        };
        let Some(visible) = result.get("visible").and_then(serde_json::Value::as_bool) else {
            self.conversion_status = Some("Panel visibility command returned no visibility state.".into());
            return;
        };
        match self.workspaces.current_mut().map(|workspace| workspace.set_visible(panel, visible)) {
            Some(Ok(())) if visible => self.focused_panel = panel,
            Some(Ok(())) => self.reconcile_focused_panel(),
            Some(Err(error)) => self.conversion_status = Some(format!("Could not change panel visibility: {error}")),
            None => self.conversion_status = Some("No workspace is available.".into()),
        }
    }

    #[cfg(target_os = "macos")]
    fn menu_inputs_snapshot(&self) -> (Vec<String>, String, Vec<(crate::workspace::PanelId, bool)>) {
        let panels = crate::workspace::PanelId::ALL
            .into_iter()
            .map(|panel| (panel, self.workspaces.current().is_some_and(|ws| ws.is_visible(panel))))
            .collect();
        (self.workspaces.names(), self.workspaces.active.clone(), panels)
    }

    #[cfg(target_os = "macos")]
    fn menu_bar(&self) -> craft_menu::MenuBar {
        let state = self.command_state();
        let (workspaces, active_workspace, panels) = self.menu_inputs_snapshot();
        crate::menus::build_bar(&crate::menus::MenuInputs {
            shortcuts: &self.workspace_shortcuts,
            state: &state,
            workspaces,
            active_workspace,
            panels,
        })
    }

    /// Installs the native macOS menu bar; returns whether it is in use. Call once from the
    /// eframe creator, on the main thread.
    #[cfg(target_os = "macos")]
    pub fn install_native_menu(&mut self, ctx: &egui::Context) -> bool {
        let layout = craft_menu::mac_layout(&self.menu_bar(), &craft_menu::LayoutOptions::new("encoderCraft"));
        self.native_menu = craft_menu::mac::install_with(ctx, &layout.bar, craft_menu::mac::MacOptions::default().window_list(false));
        self.native_menu.is_some()
    }

    /// Whether the menus live in the system menu bar instead of the window.
    pub fn has_native_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native_menu.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Feeds native menu key equivalents to egui; call from `eframe::App::raw_input_hook`.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = self.native_menu.as_mut() {
            menu.raw_input(raw);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = raw;
    }

    /// Runs the native menu's clicks and brings its items up to date.
    fn drive_native_menu(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            let Some(mut menu) = self.native_menu.take() else { return };
            menu.set_gate(craft_menu::Gate {
                modal: self.prefs_state.is_open
                    || self.show_about_dialog
                    || self.name_dialog.is_some()
                    || !self.pending_conversion_approvals.is_empty(),
                recording_shortcut: self.prefs_state.shortcut_recording.is_some(),
                text_editing: ctx.egui_wants_keyboard_input(),
            });
            for click in menu.take_clicks(ctx) {
                match crate::menus::resolve(&click) {
                    Some(action) => self.run_menu_action(action),
                    None => log::warn!("unknown menu command {}", click.id),
                }
            }
            let layout = craft_menu::mac_layout(&self.menu_bar(), &craft_menu::LayoutOptions::new("encoderCraft"));
            menu.sync(layout);
            self.native_menu = Some(menu);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    #[cfg(target_os = "macos")]
    fn run_menu_action(&mut self, action: crate::menus::MenuAction) {
        use crate::commands::AppCommand as C;
        use crate::menus::MenuAction;
        match action {
            MenuAction::Command(command @ (C::SaveWorkspaceAs | C::RenameWorkspace)) => {
                self.workspace_name_input.clear();
                self.name_dialog = Some(command);
            }
            MenuAction::Command(command) => {
                if command.enabled(&self.command_state()) {
                    self.run_command(command);
                }
            }
            MenuAction::ActivateWorkspace(name) => {
                if self.dispatch_workspace_mutation(C::ActivateWorkspace, serde_json::json!({ "name": name })).is_some() {
                    self.reconcile_focused_panel();
                }
            }
            MenuAction::TogglePanel(panel) => {
                let visible = self.workspaces.current().is_some_and(|ws| !ws.is_visible(panel));
                self.set_workspace_panel_visible(panel, visible);
            }
        }
    }

    fn render_name_dialog(&mut self, ctx: &egui::Context) {
        let Some(command) = self.name_dialog else { return };
        let title = if command == crate::commands::AppCommand::RenameWorkspace { "Rename Workspace" } else { "Save Workspace As" };
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.workspace_name_input).hint_text("Workspace name"));
            edit.request_focus();
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                confirmed = (ui.add_enabled(!self.workspace_name_input.trim().is_empty(), egui::Button::new("OK")).clicked()) || entered;
                cancelled = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if confirmed && !self.workspace_name_input.trim().is_empty() {
            self.name_dialog = None;
            self.run_workspace_app_command(command);
        } else if cancelled {
            self.name_dialog = None;
            self.workspace_name_input.clear();
        }
    }

    fn window_menu(&mut self, ui: &mut Ui) {
        ui.separator();
        ui.label(RichText::new("Workspace").small().color(self.tokens.text_dim));
        for name in self.workspaces.names() {
            let active = name == self.workspaces.active;
            if ui.radio(active, &name).clicked()
                && self.dispatch_workspace_mutation(crate::commands::AppCommand::ActivateWorkspace, serde_json::json!({ "name": name })).is_some()
            {
                self.reconcile_focused_panel();
            }
        }
        ui.separator();
        ui.label(RichText::new("Panels").small().color(self.tokens.text_dim));
        for panel in crate::workspace::PanelId::ALL {
            if let Some(ws) = self.workspaces.current() {
                let mut shown = ws.is_visible(panel);
                if ui.checkbox(&mut shown, panel.label()).changed() {
                    self.set_workspace_panel_visible(panel, shown);
                }
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.workspace_name_input).hint_text("Workspace name").desired_width(120.0));
            if ui.button("Save As").clicked() {
                let name = self.workspace_name_input.clone();
                if self.dispatch_workspace_mutation(crate::commands::AppCommand::SaveWorkspaceAs, serde_json::json!({ "name": name })).is_some() {
                    self.workspace_name_input.clear();
                    self.reconcile_focused_panel();
                }
            }
        });
        if ui.button("Rename Active").clicked() {
            let name = self.workspace_name_input.clone();
            if self.dispatch_workspace_mutation(crate::commands::AppCommand::RenameWorkspace, serde_json::json!({ "name": name })).is_some() {
                self.workspace_name_input.clear();
                self.reconcile_focused_panel();
            }
        }
        if ui.button("Reset Active").clicked()
            && let Some(default_workspace) = self.default_workspace_for_reset()
            && self
                .dispatch_workspace_mutation(
                    crate::commands::AppCommand::ResetWorkspace,
                    serde_json::json!({ "defaultWorkspace": default_workspace }),
                )
                .is_some()
        {
            self.reconcile_focused_panel();
        }
        if ui.button("Delete Active").clicked()
            && self.dispatch_workspace_mutation(crate::commands::AppCommand::DeleteWorkspace, serde_json::json!({})).is_some()
        {
            self.reconcile_focused_panel();
        }
    }

    fn save_settings_if_changed(&mut self) {
        if self.config_dir.is_none() {
            return;
        }
        if self.last_saved_workspaces.as_ref() != Some(&self.workspaces) {
            let result = self.workspaces.to_json().map_err(|error| error.to_string()).and_then(|text| {
                serde_json::from_str::<serde_json::Value>(&text).map_err(|error| error.to_string()).and_then(|workspaces| {
                    self.engine_commands
                        .dispatch(&self.queue_mgr, "workspace.save", serde_json::json!({ "workspaces": workspaces }))
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
            });
            if let Err(error) = result {
                self.conversion_status = Some(format!("Could not save workspaces: {error}"));
            } else {
                self.last_saved_workspaces = Some(self.workspaces.clone());
            }
        }
        if self.last_saved_shortcuts.as_ref() != Some(&self.workspace_shortcuts) {
            let result = serde_json::to_value(&self.workspace_shortcuts).map_err(|error| error.to_string()).and_then(|shortcuts| {
                self.workspace_shortcuts.validate()?;
                self.engine_commands
                    .dispatch(&self.queue_mgr, "keyboard_shortcuts.save", serde_json::json!({ "shortcuts": shortcuts }))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
            match result {
                Ok(()) => self.last_saved_shortcuts = Some(self.workspace_shortcuts.clone()),
                Err(error) => {
                    log::error!("could not save keyboard shortcuts: {error}");
                    self.conversion_status = Some(format!("Could not save keyboard shortcuts: {error}"));
                }
            }
        }
        if self.last_saved_prefs.as_ref() != Some(&self.prefs_state.prefs) {
            let result = serde_json::to_value(&self.prefs_state.prefs).map_err(|error| error.to_string()).and_then(|preferences| {
                self.engine_commands
                    .dispatch(&self.queue_mgr, "preferences.update", serde_json::json!({ "preferences": preferences }))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = result {
                self.conversion_status = Some(format!("Could not save preferences: {error}"));
            } else {
                self.last_saved_prefs = Some(self.prefs_state.prefs.clone());
            }
        }
        if self.last_saved_favorites.as_deref() != Some(self.media_state.model.favorites.as_slice()) {
            let result = self.engine_commands.dispatch(
                &self.queue_mgr,
                "media.set_favorites",
                serde_json::json!({ "favorites": self.media_state.model.favorites }),
            );
            if let Err(error) = result {
                self.conversion_status = Some(format!("Could not save favorite folders: {error}"));
            } else {
                self.last_saved_favorites = Some(self.media_state.model.favorites.clone());
            }
        }
        if self.last_saved_watch.as_deref() != Some(self.watch_state.configs.as_slice()) {
            let result =
                self.engine_commands.dispatch(&self.queue_mgr, "watch.set_folders", serde_json::json!({ "folders": self.watch_state.configs }));
            if let Err(error) = result {
                self.conversion_status = Some(format!("Could not save watch folders: {error}"));
            } else {
                self.last_saved_watch = Some(self.watch_state.configs.clone());
            }
        }
    }

    fn preview_section(&mut self, ui: &mut Ui, job: Option<&CraftJobSpec>, output_ready: bool) {
        let prev_job_id = self.preview_state.active_job_id.clone();
        let prev_in = self.preview_state.transport.in_point;
        let prev_out = self.preview_state.transport.out_point;

        render_preview_panel(ui, &mut self.preview_state, job, output_ready, &self.tokens);

        // Persist In/Out trim changes back to the selected job in QueueManager
        if let Some(spec) = job
            && prev_job_id.as_deref() == Some(spec.job_id.as_str())
            && (self.preview_state.transport.in_point != prev_in || self.preview_state.transport.out_point != prev_out)
        {
            self.dispatch_engine_command(
                "queue.set_trim_range",
                serde_json::json!({
                    "jobId": spec.job_id,
                    "inFrame": self.preview_state.transport.in_point,
                    "outFrame": self.preview_state.transport.out_point,
                    "fps": self.preview_state.transport.fps().round().max(1.0) as u64,
                }),
            );
        }
    }

    /// Renders the complete application frame inside an egui Ui.
    pub fn render(&mut self, ui: &mut Ui) {
        if !self.fonts_installed {
            crate::fonts::install(ui.ctx());
            crate::fonts::check_static_glyphs(ui.ctx());
            self.fonts_installed = true;
        }
        apply_theme(ui.ctx(), &self.tokens);

        let ctx = ui.ctx().clone();
        if self.prefs_state.shortcut_recording.is_none() {
            self.handle_workspace_shortcuts(&ctx);
            for cmd in self.workspace_shortcuts.triggered_commands(&ctx) {
                if cmd.requires_queue_focus() && self.focused_panel != crate::workspace::PanelId::Queue {
                    continue;
                }
                self.run_command(cmd);
            }
        }

        self.drive_native_menu(&ctx);
        if std::mem::take(&mut self.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.render_name_dialog(&ctx);

        // 1. Top Application Menu Bar
        let style = self.title_bar;
        let native = self.has_native_menu();
        let bar_frame = egui::Frame::new().fill(self.tokens.bg_panel).inner_margin(egui::Margin::symmetric(4, 0));
        let bar = egui::Panel::top("top_menu_bar").frame(bar_frame).exact_size(crate::titlebar::BAR_HEIGHT).show(ui, |ui| {
            let bar_rect = ui.max_rect().expand2(egui::vec2(4.0, 0.0));
            if style.drags_window() {
                crate::titlebar::drag_area(ui, bar_rect);
            }
            if native {
                // The menus are in the system menu bar; the strip only names the window.
                let title = format!("encoderCraft — {}", self.workspaces.active);
                ui.centered_and_justified(|ui| ui.label(RichText::new(title).small().color(self.tokens.text_dim)));
                return bar_rect;
            }
            ui.horizontal_centered(|ui| {
                ui.add_space(style.leading_inset());
                let state = self.command_state();
                let mut chosen = None;
                for menu in crate::commands::Menu::ALL {
                    ui.menu_button(menu.label(), |ui| {
                        for info in crate::commands::menu_commands(menu) {
                            if info.separator_before {
                                ui.separator();
                            }
                            let mut button = egui::Button::new(info.label);
                            let shortcut_text = self.workspace_shortcuts.command_display(info.command);
                            if let Some(shortcut) = shortcut_text {
                                button = button.shortcut_text(shortcut);
                            }
                            if ui.add_enabled(info.command.enabled(&state), button).clicked() {
                                chosen = Some(info.command);
                                ui.close();
                            }
                        }
                        if menu == crate::commands::Menu::Window {
                            self.window_menu(ui);
                        }
                    });
                }
                if let Some(cmd) = chosen {
                    self.run_command(cmd);
                }
            });
            bar_rect
        });
        if style.is_custom() {
            crate::titlebar::caption_buttons(ui, &self.tokens, bar.inner);
            crate::titlebar::resize_zones(ui);
        }

        // 2. Bottom Status Bar
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let jobs = self.queue_mgr.list_jobs();
                let completed_count = jobs.iter().filter(|j| j.status == encodercraft_core::JobStatus::Completed).count();
                let watch_count = self.watch_state.configs.iter().filter(|c| c.enabled).count();

                ui.label(
                    RichText::new(format!("Queue: {} jobs ({} finished) · Watch Folders: {} active", jobs.len(), completed_count, watch_count))
                        .small()
                        .color(self.tokens.text_dim),
                );
                if let Some(message) = &self.conversion_status {
                    ui.separator();
                    ui.label(RichText::new(message).small().color(self.tokens.status_failed));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Pure Rust · Memory Safe · Parity Ready").small().color(self.tokens.text_dim));
                });
            });
        });

        // 3. Central Workspace with Side Panels
        let mut inspect_job = None;

        // Drag-and-drop detection for external media files (AME Dual-Zone Parity)
        let dropped_files = ui.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            let pointer_pos = ui.input(|i| i.pointer.hover_pos());
            let is_stitch_zone = pointer_pos.is_some_and(|pos| pos.x > 450.0);

            let paths: Vec<std::path::PathBuf> =
                dropped_files.into_iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect();

            if paths.len() > 1 && is_stitch_zone {
                if let Some(first) = paths.first() {
                    let out = first.with_file_name("stitched_sequence.mp4");
                    match QueueManager::build_stitched_job_spec(paths, out, "Match Source - High Bitrate".into()) {
                        Ok(spec) => self.submit_job_for_review(spec),
                        Err(error) => self.conversion_status = Some(format!("Could not prepare stitched job: {error}")),
                    }
                }
            } else {
                for p in paths {
                    self.add_file_job(p);
                }
            }
        }

        // Periodic Watch Folder Ingest Polling
        let now = std::time::Instant::now();
        let should_poll_watch = self.watch_state.auto_encode_enabled
            && !self.watch_state.configs.is_empty()
            && self.last_watch_tick.is_none_or(|last| now.duration_since(last) > std::time::Duration::from_secs(2));

        if should_poll_watch {
            self.last_watch_tick = Some(now);
            trigger_watch_scan(&mut self.watch_state, &self.queue_mgr, &self.preset_registry, &self.engine_commands);
        }

        let is_worker_active = self.queue_mgr.is_worker_running();
        if self.was_worker_running && !is_worker_active {
            // Queue finished!
            if self.prefs_state.prefs.play_completion_sound {
                self.completion_banner_timer = Some(std::time::Instant::now());
            }
        }
        self.was_worker_running = is_worker_active;
        self.queue_state.is_running = is_worker_active && !self.queue_mgr.is_paused();

        let is_hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
        if self.queue_state.is_running || is_worker_active || is_hovering || should_poll_watch {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }

        let selected_job_spec = self
            .queue_state
            .selected_job_id
            .as_ref()
            .and_then(|id| self.queue_mgr.list_jobs().into_iter().find(|j| &j.spec.job_id == id).map(|j| j.spec));

        let output_ready =
            self.queue_state.selected_job_id.as_ref().is_some_and(|id| {
                self.queue_mgr.list_jobs().iter().any(|j| &j.spec.job_id == id && j.status == encodercraft_core::JobStatus::Completed)
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.render_dock_area(ui, &mut inspect_job, selected_job_spec, output_ready, is_hovering);
        });
        for (job_id, spec) in std::mem::take(&mut self.preset_state.pending_updates) {
            self.update_job_for_review(&job_id, spec);
        }
        for (job_id, spec) in std::mem::take(&mut self.queue_state.pending_updates) {
            self.update_job_for_review(&job_id, spec);
        }
        for spec in std::mem::take(&mut self.queue_state.pending_specs) {
            self.submit_job_for_review(spec);
        }

        // 4. Handle Inspector Modal
        if let Some(spec) = inspect_job {
            self.inspector_state.job_spec = Some(spec);
            self.inspector_state.is_open = true;
        }

        let mut saved_spec = None;
        render_inspector_modal(&ctx, &mut self.inspector_state, &self.tokens, &mut saved_spec);
        self.render_conversion_approval(&ctx);

        if let Some(new_spec) = saved_spec {
            let job_id = new_spec.job_id.clone();
            self.update_job_for_review(&job_id, new_spec);
        }

        // 5. Log Dialog
        render_log_dialog(&ctx, &mut self.log_state, &self.queue_mgr, &self.engine_commands, &self.tokens);

        // 6. Preferences Dialog
        let mut theme_change = None;
        render_prefs_dialog(&ctx, &mut self.prefs_state, &mut self.workspace_shortcuts, &self.tokens, &mut theme_change);
        if self.queue_mgr.parallelism() != self.prefs_state.prefs.max_parallel_jobs {
            self.dispatch_engine_command("queue.set_parallelism", serde_json::json!({ "workers": self.prefs_state.prefs.max_parallel_jobs }));
        }
        if let Some(theme) = theme_change {
            self.set_brightness(Self::brightness_from_theme(&theme));
        }

        self.save_settings_if_changed();
        self.drive_auto_shutdown(&ctx);

        // 7. About Dialog
        if self.show_about_dialog {
            egui::Window::new("About encoderCraft").collapsible(false).resizable(false).open(&mut self.show_about_dialog).show(&ctx, |ui| {
                ui.heading(format!("encoderCraft {}", env!("CARGO_PKG_VERSION")));
                ui.label("A pure-Rust, universal media transcoding & batch queue system.");
                ui.label("Designed for seamless ArtCraft upstream integration.");
                ui.add_space(4.0);
                ui.label(RichText::new("Clean-Room Adobe Media Encoder Parity").strong());
            });
        }
    }

    fn render_dock_area(
        &mut self,
        ui: &mut Ui,
        inspect_job: &mut Option<CraftJobSpec>,
        selected_job: Option<CraftJobSpec>,
        output_ready: bool,
        is_hovering: bool,
    ) {
        if let Some(timer) = self.completion_banner_timer {
            if timer.elapsed().as_secs() < 5 {
                egui::Frame::new().fill(self.tokens.status_completed.gamma_multiply(0.2)).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icons::icon_label(ui, icons::Icon::Check, 15.0, self.tokens.status_completed);
                        ui.label(
                            RichText::new("All batch transcode queue jobs completed successfully!").color(self.tokens.status_completed).strong(),
                        );
                    });
                });
            } else {
                self.completion_banner_timer = None;
            }
        }
        if is_hovering {
            ui.label(RichText::new("Drop media onto the workspace to add it to the queue.").small().color(self.tokens.accent_primary));
        }

        let Some(workspace) = self.workspaces.current() else {
            ui.label(RichText::new("No workspace is available.").color(self.tokens.status_failed));
            return;
        };
        let layout = workspace.dock.layout.clone();
        let visible_panels = layout.panels().into_iter().copied().collect();
        let mut style = DockStyle::from_ui(ui);
        style.tab_height = 24.0;
        style.gap = 2.0;
        style.min_pane = 120.0;
        style.background = self.tokens.bg_panel;
        style.tab_background = self.tokens.bg_panel_darker;
        style.active_background = self.tokens.bg_surface_active;
        style.text = self.tokens.text_primary;
        style.inactive_text = self.tokens.text_secondary;
        style.border = egui::Stroke::new(1.0, self.tokens.border_subtle);
        style.accent = self.tokens.accent_primary;
        style.font = egui::FontId::proportional(11.0);

        let mut content = EncoderDockContent { app: self, inspect_job, selected_job, output_ready, visible_panels, visibility_changes: Vec::new() };
        let output =
            DockArea::new(egui::Id::unique("encodercraft-dock-area")).float_bounds(ui.max_rect()).show_content(ui, &layout, &style, &mut content);
        let visibility_changes = std::mem::take(&mut content.visibility_changes);
        drop(content);

        if let Some(error) = output.error {
            self.conversion_status = Some(format!("Workspace layout could not be rendered: {error}"));
        }
        let mut focused_panel_closed = false;
        for action in &output.actions {
            match action {
                DockAction::Activate { panel } => self.focused_panel = *panel,
                DockAction::Close { panel } if *panel == self.focused_panel => focused_panel_closed = true,
                _ => {}
            }
        }
        if !output.actions.is_empty()
            && let Some(workspace) = self.workspaces.current_mut()
            && let Err(error) = workspace.apply_actions(output.actions)
        {
            self.conversion_status = Some(format!("Workspace change was rejected: {error}"));
        }
        for (panel, visible) in visibility_changes {
            if let Some(workspace) = self.workspaces.current_mut()
                && let Err(error) = workspace.set_visible(panel, visible)
            {
                self.conversion_status = Some(format!("Could not change panel visibility: {error}"));
            } else if visible {
                self.focused_panel = panel;
            } else if panel == self.focused_panel {
                focused_panel_closed = true;
            }
        }
        if focused_panel_closed {
            self.reconcile_focused_panel();
        }
        for window in output.window_files {
            for dropped in window.dropped {
                let path = dropped.path().to_path_buf();
                if !path.as_os_str().is_empty() {
                    self.add_file_job(path);
                }
            }
        }
    }

    /// Runs the Auto Computer Shutdown countdown once the whole queue has finished.
    fn drive_auto_shutdown(&mut self, ctx: &egui::Context) {
        use crate::queue_footer::{ShutdownAction, queue_finished, shut_down_computer};
        let finished = queue_finished(&self.queue_mgr.list_jobs());
        match self.shutdown_watch.update(self.prefs_state.prefs.auto_shutdown, finished, std::time::Instant::now()) {
            ShutdownAction::Idle => {}
            ShutdownAction::Countdown { remaining_secs } => {
                let mut cancel = false;
                egui::Window::new("Auto Computer Shutdown").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
                    ctx,
                    |ui| {
                        ui.label(format!("The queue has finished. This computer shuts down in {remaining_secs} s."));
                        if ui.button("Cancel Shutdown").clicked() {
                            cancel = true;
                        }
                    },
                );
                if cancel {
                    self.shutdown_watch.cancel();
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            ShutdownAction::ShutDownNow => {
                if let Err(e) = shut_down_computer() {
                    log::error!("automatic shutdown failed: {e}");
                    self.shutdown_error = Some(e);
                }
            }
        }
        if let Some(err) = self.shutdown_error.clone() {
            let mut open = true;
            egui::Window::new("Shutdown failed").open(&mut open).show(ctx, |ui| {
                ui.label(err);
            });
            if !open {
                self.shutdown_error = None;
            }
        }
    }

    /// Builds a local-file job through the shared engine command and submits it for review.
    pub fn add_file_job(&mut self, path: std::path::PathBuf) {
        if let Some(spec) = self.dispatch_prepared_app_job_spec(
            crate::commands::AppCommand::AddMedia,
            serde_json::json!({ "path": path, "clientApp": "encoderCraft GUI" }),
        ) {
            self.submit_job_for_review(spec);
        }
    }

    fn submit_job_for_review(&mut self, spec: CraftJobSpec) {
        match self.queue_mgr.conversion_review(&spec) {
            Ok(conversions) if conversions.is_empty() => match serde_json::to_value(spec) {
                Ok(params) => self.dispatch_engine_command("queue.add", params),
                Err(error) => self.conversion_status = Some(format!("Could not serialize job for queueing: {error}")),
            },
            Ok(conversions) => {
                self.pending_conversion_approvals.push_back(PendingConversionApproval {
                    existing_job_id: None,
                    spec,
                    conversions,
                    remember_choice: false,
                });
            }
            Err(error) => {
                self.conversion_status = Some(format!("Could not queue job: {error}"));
            }
        }
    }

    fn update_job_for_review(&mut self, job_id: &str, spec: CraftJobSpec) {
        match self.queue_mgr.conversion_review(&spec) {
            Ok(conversions) if conversions.is_empty() => match serde_json::to_value(spec) {
                Ok(spec) => {
                    self.dispatch_engine_command("queue.update_with_approvals", serde_json::json!({ "jobId": job_id, "spec": spec, "approvals": [] }))
                }
                Err(error) => self.conversion_status = Some(format!("Could not serialize updated job: {error}")),
            },
            Ok(conversions) => {
                self.pending_conversion_approvals.push_back(PendingConversionApproval {
                    existing_job_id: Some(job_id.into()),
                    spec,
                    conversions,
                    remember_choice: false,
                });
            }
            Err(error) => {
                self.conversion_status = Some(format!("Could not update job: {error}"));
            }
        }
    }

    fn render_conversion_approval(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_conversion_approvals.front_mut() else { return };
        let mut open = true;
        let mut action = None;
        egui::Window::new("Approve Lossy Conversion")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("This output requires irreversible video changes:");
                ui.add_space(4.0);
                for conversion in &pending.conversions {
                    ui.label(RichText::new(conversion.description()).small().color(self.tokens.text_secondary));
                }
                ui.add_space(8.0);
                ui.checkbox(&mut pending.remember_choice, "Don't ask again for these exact format pairs");
                ui.horizontal(|ui| {
                    if ui.button("Refuse").clicked() {
                        action = Some(false);
                    }
                    if ui.button("Allow").clicked() {
                        action = Some(true);
                    }
                });
            });
        if !open {
            action = Some(false);
        }
        if let Some(allowed) = action {
            self.resolve_conversion_approval(allowed);
        }
    }

    fn resolve_conversion_approval(&mut self, allowed: bool) {
        let Some(pending) = self.pending_conversion_approvals.front().cloned() else { return };
        if pending.remember_choice {
            for conversion in &pending.conversions {
                let result = serde_json::to_value(&conversion.key).map_err(|error| error.to_string()).and_then(|key| {
                    self.engine_commands
                        .dispatch(&self.queue_mgr, "queue.save_conversion_decision", serde_json::json!({ "key": key, "allowed": allowed }))
                        .map_err(|error| error.to_string())
                });
                if let Err(error) = result {
                    self.conversion_status = Some(format!("Could not save conversion choice: {error}"));
                    return;
                }
            }
        }

        if allowed {
            let approved: Vec<LossyConversionKey> = pending.conversions.iter().map(|conversion| conversion.key.clone()).collect();
            let command_id = if pending.existing_job_id.is_some() { "queue.update_with_approvals" } else { "queue.add_with_approvals" };
            let spec = match serde_json::to_value(&pending.spec) {
                Ok(spec) => spec,
                Err(error) => {
                    self.conversion_status = Some(format!("Could not serialize approved job: {error}"));
                    return;
                }
            };
            let approvals = match serde_json::to_value(&approved) {
                Ok(approvals) => approvals,
                Err(error) => {
                    self.conversion_status = Some(format!("Could not serialize conversion approvals: {error}"));
                    return;
                }
            };
            let params = match &pending.existing_job_id {
                Some(job_id) => serde_json::json!({ "jobId": job_id, "spec": spec, "approvals": approvals }),
                None => serde_json::json!({ "spec": spec, "approvals": approvals }),
            };
            if self.dispatch_engine_command_value(command_id, params).is_some() {
                self.conversion_status = Some(if pending.existing_job_id.is_some() {
                    "Lossy conversion approved; job settings updated.".into()
                } else {
                    "Lossy conversion approved and queued.".into()
                });
            }
        } else {
            self.conversion_status = Some(if pending.remember_choice {
                "Conversion refused; choice saved for these format pairs.".into()
            } else {
                "Conversion refused; job was not queued.".into()
            });
        }
        self.pending_conversion_approvals.pop_front();
    }
}

struct EncoderDockContent<'a, 'b> {
    app: &'a mut EncoderCraftApp,
    inspect_job: &'b mut Option<CraftJobSpec>,
    selected_job: Option<CraftJobSpec>,
    output_ready: bool,
    visible_panels: std::collections::HashSet<crate::workspace::PanelId>,
    visibility_changes: Vec<(crate::workspace::PanelId, bool)>,
}

impl DockContent<crate::workspace::PanelId> for EncoderDockContent<'_, '_> {
    fn body(&mut self, ui: &mut Ui, panel: &crate::workspace::PanelId) {
        use crate::workspace::PanelId;

        egui::Frame::new().fill(self.app.tokens.bg_panel).inner_margin(self.app.tokens.panel_margin()).show(ui, |ui| match panel {
            PanelId::MediaBrowser => {
                for path in crate::media_browser::render_media_browser(
                    ui,
                    &mut self.app.media_state,
                    &self.app.tokens,
                    &self.app.engine_commands,
                    &self.app.queue_mgr,
                ) {
                    self.app.pending_media_item = Some(path);
                    self.app.run_command(crate::commands::AppCommand::AddMediaItem);
                }
            }

            PanelId::Presets => {
                if let Some(command) = render_preset_browser(
                    ui,
                    &mut self.app.preset_state,
                    &self.app.queue_mgr,
                    &self.app.engine_commands,
                    &self.app.queue_state.selected_ids_list(),
                    &self.app.tokens,
                ) {
                    self.app.run_command(command);
                }
            }
            PanelId::Queue => {
                let mut add_media = false;
                render_queue_panel(
                    ui,
                    &mut self.app.queue_state,
                    crate::queue_panel::QueuePanelContext {
                        queue_mgr: &self.app.queue_mgr,
                        commands: &self.app.engine_commands,
                        tokens: &self.app.tokens,
                        on_inspect_job: self.inspect_job,
                        on_open_log: &mut self.app.log_state.is_open,
                        auto_shutdown: &mut self.app.prefs_state.prefs.auto_shutdown,
                        on_add_media: &mut add_media,
                        selected_renderer: &mut self.app.prefs_state.prefs.selected_renderer,
                    },
                );
                if add_media {
                    self.app.pick_media();
                }
            }
            PanelId::WatchFolders => {
                render_watch_panel(
                    ui,
                    &mut self.app.watch_state,
                    &self.app.queue_mgr,
                    &self.app.preset_registry,
                    &self.app.engine_commands,
                    &self.app.tokens,
                );
            }
            PanelId::Preview => self.app.preview_section(ui, self.selected_job.as_ref(), self.output_ready),
            PanelId::Encoding => {
                if let Some(job_id) =
                    crate::encoding_panel::render_encoding_panel(ui, &mut self.app.encoding_state, &self.app.queue_mgr, &self.app.tokens)
                {
                    self.app.dispatch_engine_command("queue.stop", serde_json::json!({ "jobId": job_id }));
                }
            }
        });

        if self.app.focused_panel == *panel {
            ui.painter().rect_stroke(
                ui.max_rect().shrink(1.0),
                4.0,
                egui::Stroke::new(1.5, self.app.tokens.accent_primary),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn logical_label(&self, panel: &crate::workspace::PanelId) -> String {
        panel.label().into()
    }

    fn paint_icon(&mut self, painter: &egui::Painter, rect: egui::Rect, panel: &crate::workspace::PanelId, color: egui::Color32) -> bool {
        let icon = match panel {
            crate::workspace::PanelId::MediaBrowser | crate::workspace::PanelId::WatchFolders => icons::Icon::Folder,
            crate::workspace::PanelId::Presets => icons::Icon::Preset,
            crate::workspace::PanelId::Queue => icons::Icon::ListView,
            crate::workspace::PanelId::Preview => icons::Icon::Info,
            crate::workspace::PanelId::Encoding => icons::Icon::Sliders,
        };
        icons::paint(painter, rect, icon, color);
        true
    }

    fn group_header_width(
        &self,
        _panels: &[crate::workspace::PanelId],
        _active: &crate::workspace::PanelId,
        _floating: bool,
        _strip_width: f32,
    ) -> f32 {
        28.0
    }

    fn group_header(&mut self, ui: &mut Ui, _panels: &[crate::workspace::PanelId], _active: &crate::workspace::PanelId, _floating: bool) {
        ui.menu_button("☰", |ui| {
            for panel in crate::workspace::PanelId::ALL {
                let mut visible = self.visible_panels.contains(&panel);
                if ui.checkbox(&mut visible, panel.label()).changed() {
                    self.visibility_changes.push((panel, visible));
                    if visible {
                        self.visible_panels.insert(panel);
                    } else {
                        self.visible_panels.remove(&panel);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut_input(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers });
        input
    }

    fn run_shortcut_frame(app: &mut EncoderCraftApp, input: egui::RawInput) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(input, |ui| app.handle_workspace_shortcuts(ui.ctx()));
        output.textures_delta.clear();
    }

    #[test]
    fn workspace_commands_cycle_focus_and_toggle_the_focused_panel() {
        let queue_mgr = Arc::new(QueueManager::new_in_memory());
        let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));
        let mut app = EncoderCraftApp::new(queue_mgr, preset_registry);
        app.focused_panel = crate::workspace::PanelId::MediaBrowser;

        app.run_workspace_action(crate::settings::WorkspaceShortcutAction::FocusNext);
        assert_eq!(app.focused_panel, crate::workspace::PanelId::Presets);

        app.run_workspace_action(crate::settings::WorkspaceShortcutAction::ToggleFocusedPanel);
        assert!(!app.workspaces.current().is_some_and(|workspace| workspace.is_visible(crate::workspace::PanelId::Presets)));
        assert_eq!(app.focused_panel, crate::workspace::PanelId::MediaBrowser);

        app.run_workspace_action(crate::settings::WorkspaceShortcutAction::FocusPrevious);
        assert_eq!(app.focused_panel, crate::workspace::PanelId::Encoding);
    }

    #[test]
    fn configured_workspace_shortcuts_dispatch_focus_and_visibility_commands() {
        let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));

        run_shortcut_frame(&mut app, shortcut_input(egui::Key::F6, egui::Modifiers::default()));
        assert_eq!(app.focused_panel, crate::workspace::PanelId::WatchFolders);

        run_shortcut_frame(&mut app, shortcut_input(egui::Key::F6, egui::Modifiers { shift: true, ..Default::default() }));
        assert_eq!(app.focused_panel, crate::workspace::PanelId::Queue);

        run_shortcut_frame(&mut app, shortcut_input(egui::Key::P, egui::Modifiers { command: true, shift: true, ..Default::default() }));
        assert!(!app.workspaces.current().is_some_and(|workspace| workspace.is_visible(crate::workspace::PanelId::Queue)));
    }

    #[test]
    fn workspace_menu_commands_persist_shared_engine_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
            .with_config_dir(dir.path().to_path_buf());

        app.workspace_name_input = "Studio".into();
        app.run_command(crate::commands::AppCommand::SaveWorkspaceAs);
        assert_eq!(app.workspaces.active, "Studio");

        app.workspace_name_input = "Edit Suite".into();
        app.run_command(crate::commands::AppCommand::RenameWorkspace);
        assert_eq!(app.workspaces.active, "Edit Suite");

        app.run_command(crate::commands::AppCommand::DeleteWorkspace);
        assert_eq!(app.workspaces.active, crate::workspace::DEFAULT_WORKSPACE);

        let persisted = app.engine_commands.dispatch(&app.queue_mgr, "workspace.get", serde_json::json!({})).unwrap();
        assert_eq!(persisted["active"], crate::workspace::DEFAULT_WORKSPACE);
        assert_eq!(persisted["workspaces"].as_array().unwrap().len(), 3);
    }
}
