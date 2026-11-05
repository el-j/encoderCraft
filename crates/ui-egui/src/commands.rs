//! The GUI command table: every menu item and keyboard shortcut is one `AppCommand`.
//!
//! Domain actions map to engine `CommandSpec` ids so the GUI, CLI, IPC and MCP share behavior.

use egui::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Menu {
    File,
    Edit,
    Queue,
    Preset,
    Window,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandContextMenu {
    QueueRow,
    Preset,
    MediaItem,
}

impl Menu {
    pub const ALL: [Menu; 6] = [Menu::File, Menu::Edit, Menu::Queue, Menu::Preset, Menu::Window, Menu::Help];

    pub fn label(self) -> &'static str {
        match self {
            Menu::File => "File",
            Menu::Edit => "Edit",
            Menu::Queue => "Queue",
            Menu::Preset => "Preset",
            Menu::Window => "Window",
            Menu::Help => "Help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppCommand {
    AddMedia,
    AddMediaItem,
    OpenMediaFolder,
    ToggleMediaFavorite,
    RevealOutput,
    CopyOutputPath,
    RevealSource,
    CopySourcePath,
    ExportSettings,
    AddStitched,
    AddWatchFolder,
    ShowLog,
    Preferences,
    ThemeDark,
    ThemeLight,
    ThemeHighContrast,
    StartQueue,
    PauseQueue,
    StopQueue,
    ResetSelected,
    DuplicateSelected,
    RemoveSelected,
    MoveSelectedUp,
    MoveSelectedDown,
    SelectAll,
    StitchSelected,
    SetDestination,
    AddOutputTarget,
    InspectSettings,
    ClearFinished,
    CreatePreset,
    ImportPresets,
    ExportPresets,
    ApplyPreset,
    AddPresetTarget,
    DeletePreset,
    FocusNextPanel,
    FocusPreviousPanel,
    ToggleFocusedPanel,
    ActivateWorkspace,
    SaveWorkspaceAs,
    RenameWorkspace,
    ResetWorkspace,
    DeleteWorkspace,
    SetPanelVisibility,
    About,
}

/// Primary-modifier shortcut (Cmd on macOS, Ctrl elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Shortcut {
    const fn cmd(key: Key) -> Self {
        Self { primary: true, shift: false, alt: false, key }
    }

    const fn plain(key: Key) -> Self {
        Self { primary: false, shift: false, alt: false, key }
    }

    const fn alt(key: Key) -> Self {
        Self { primary: false, shift: false, alt: true, key }
    }

    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.primary {
            out.push_str(if cfg!(target_os = "macos") { "Cmd+" } else { "Ctrl+" });
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        out.push_str(&match self.key {
            Key::Comma => ",".to_string(),
            Key::Period => ".".to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Delete => "Del".to_string(),
            Key::Escape => "Esc".to_string(),
            other => format!("{other:?}"),
        });
        out
    }

    /// True when this shortcut was pressed with exactly its modifiers.
    pub fn pressed(&self, input: &egui::InputState) -> bool {
        let m: Modifiers = input.modifiers;
        m.command == self.primary && m.shift == self.shift && m.alt == self.alt && input.key_pressed(self.key)
    }
}

/// Facts about the app that decide which commands are available.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandState {
    pub selected: usize,
    pub jobs: usize,
    pub finished: usize,
    pub worker_running: bool,
    pub worker_active: bool,
    pub preset_selected: bool,
    pub preset_deletable: bool,
    pub has_source: bool,
    pub has_output: bool,
}

pub struct CommandInfo {
    pub command: AppCommand,
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Menu,
    pub shortcut: Option<Shortcut>,
    /// Matching engine command id when the action is also available over CLI/IPC/MCP.
    pub engine_id: Option<&'static str>,
    pub menu_visible: bool,
    pub shortcut_configurable: bool,
    /// Draw a separator above this entry.
    pub separator_before: bool,
}

const fn info(
    command: AppCommand,
    id: &'static str,
    label: &'static str,
    menu: Menu,
    shortcut: Option<Shortcut>,
    engine_id: Option<&'static str>,
    separator_before: bool,
) -> CommandInfo {
    CommandInfo { command, id, label, menu, shortcut, engine_id, menu_visible: true, shortcut_configurable: true, separator_before }
}

const fn context_info(command: AppCommand, id: &'static str, label: &'static str, menu: Menu, engine_id: Option<&'static str>) -> CommandInfo {
    CommandInfo { command, id, label, menu, shortcut: None, engine_id, menu_visible: false, shortcut_configurable: false, separator_before: false }
}

pub const COMMANDS: &[CommandInfo] = &[
    info(
        AppCommand::AddMedia,
        "file.add_media",
        "Add Media Files...",
        Menu::File,
        Some(Shortcut::cmd(Key::I)),
        Some("queue.prepare_add_file"),
        false,
    ),
    context_info(AppCommand::AddMediaItem, "media.add_item", "Add Media to Queue", Menu::File, Some("queue.prepare_add_file")),
    context_info(AppCommand::OpenMediaFolder, "media.open_folder", "Open Folder", Menu::File, Some("media.open_folder")),
    context_info(AppCommand::ToggleMediaFavorite, "media.toggle_favorite", "Toggle Favorite", Menu::File, Some("media.toggle_favorite")),
    context_info(AppCommand::RevealOutput, "queue.reveal_output", "Reveal Output in File Manager", Menu::Queue, Some("queue.reveal_output")),
    context_info(AppCommand::CopyOutputPath, "queue.copy_output_path", "Copy Output Path", Menu::Queue, Some("queue.copy_output_path")),
    context_info(AppCommand::RevealSource, "queue.reveal_source", "Reveal Source in File Manager", Menu::Queue, Some("queue.reveal_source")),
    context_info(AppCommand::CopySourcePath, "queue.copy_source_path", "Copy Source Path", Menu::Queue, Some("queue.copy_source_path")),
    context_info(AppCommand::ExportSettings, "queue.export_settings", "Export Settings...", Menu::Queue, Some("queue.export_settings")),
    info(AppCommand::AddStitched, "file.add_stitched", "Add Stitched Sequence...", Menu::File, None, Some("queue.prepare_stitch_sources"), false),
    info(AppCommand::AddWatchFolder, "file.add_watch_folder", "Add Watch Folder...", Menu::File, None, Some("watch.add_folder"), false),
    info(AppCommand::ShowLog, "file.show_log", "Show Encoding Log...", Menu::File, Some(Shortcut::cmd(Key::L)), Some("queue.log"), true),
    info(AppCommand::Preferences, "edit.preferences", "Preferences...", Menu::Edit, Some(Shortcut::cmd(Key::Comma)), Some("preferences.get"), false),
    info(AppCommand::ThemeDark, "edit.theme_dark", "Theme: Dark", Menu::Edit, None, Some("preferences.set_theme"), true),
    info(AppCommand::ThemeLight, "edit.theme_light", "Theme: Light", Menu::Edit, None, Some("preferences.set_theme"), false),
    info(AppCommand::ThemeHighContrast, "edit.theme_high_contrast", "Theme: High Contrast", Menu::Edit, None, Some("preferences.set_theme"), false),
    info(AppCommand::StartQueue, "queue.start", "Start Queue", Menu::Queue, Some(Shortcut::plain(Key::Enter)), Some("queue.start"), false),
    info(AppCommand::PauseQueue, "queue.pause", "Pause Queue", Menu::Queue, Some(Shortcut::cmd(Key::Period)), Some("queue.pause_worker"), false),
    info(AppCommand::StopQueue, "queue.stop", "Stop Queue", Menu::Queue, Some(Shortcut::plain(Key::Escape)), Some("queue.stop_worker"), false),
    info(AppCommand::ResetSelected, "queue.reset", "Reset Selected", Menu::Queue, None, Some("queue.reset"), true),
    info(
        AppCommand::DuplicateSelected,
        "queue.duplicate",
        "Duplicate Selected",
        Menu::Queue,
        Some(Shortcut::cmd(Key::D)),
        Some("queue.duplicate_spec"),
        false,
    ),
    info(AppCommand::RemoveSelected, "queue.remove", "Remove Selected", Menu::Queue, Some(Shortcut::plain(Key::Delete)), Some("queue.remove"), false),
    info(
        AppCommand::MoveSelectedUp,
        "queue.move_up",
        "Move Selected Up",
        Menu::Queue,
        Some(Shortcut::alt(Key::ArrowUp)),
        Some("queue.move_up"),
        false,
    ),
    info(
        AppCommand::MoveSelectedDown,
        "queue.move_down",
        "Move Selected Down",
        Menu::Queue,
        Some(Shortcut::alt(Key::ArrowDown)),
        Some("queue.move_down"),
        false,
    ),
    info(AppCommand::SelectAll, "queue.select_all", "Select All Jobs", Menu::Edit, Some(Shortcut::cmd(Key::A)), Some("queue.select_all"), false),
    info(AppCommand::StitchSelected, "queue.stitch_selected", "Stitch Selected Clips...", Menu::Queue, None, Some("queue.prepare_stitch"), true),
    info(AppCommand::SetDestination, "queue.set_destination", "Set Destination Directory...", Menu::Queue, None, Some("queue.set_output_dir"), false),
    context_info(AppCommand::AddOutputTarget, "queue.add_output_target", "Add Output Target", Menu::Queue, Some("queue.prepare_add_output_target")),
    context_info(AppCommand::InspectSettings, "queue.inspect_settings", "Inspect Export Settings", Menu::Queue, Some("queue.export_settings")),
    info(AppCommand::ClearFinished, "queue.clear_finished", "Clear Finished Jobs", Menu::Queue, None, Some("queue.clear_finished"), true),
    info(AppCommand::CreatePreset, "preset.create", "Create Encoding Preset...", Menu::Preset, None, Some("preset.create"), false),
    context_info(AppCommand::ImportPresets, "preset.import", "Import Presets", Menu::Preset, Some("preset.import")),
    context_info(AppCommand::ExportPresets, "preset.export", "Export Presets", Menu::Preset, Some("preset.export")),
    context_info(AppCommand::ApplyPreset, "preset.apply_selected", "Apply Preset to Selected Jobs", Menu::Preset, Some("queue.prepare_apply_preset")),
    context_info(AppCommand::AddPresetTarget, "preset.add_output", "Add Preset as Output", Menu::Preset, Some("queue.prepare_add_preset_target")),
    context_info(AppCommand::DeletePreset, "preset.delete", "Delete Preset", Menu::Preset, Some("preset.delete")),
    info(AppCommand::FocusNextPanel, "window.focus_next_panel", "Focus Next Panel", Menu::Window, None, Some("workspace.focus_next"), false),
    info(
        AppCommand::FocusPreviousPanel,
        "window.focus_previous_panel",
        "Focus Previous Panel",
        Menu::Window,
        None,
        Some("workspace.focus_previous"),
        false,
    ),
    info(
        AppCommand::ToggleFocusedPanel,
        "window.toggle_focused_panel",
        "Toggle Focused Panel",
        Menu::Window,
        None,
        Some("workspace.toggle_panel"),
        false,
    ),
    context_info(AppCommand::ActivateWorkspace, "workspace.activate", "Activate Workspace", Menu::Window, Some("workspace.activate")),
    context_info(AppCommand::SaveWorkspaceAs, "workspace.save_as", "Save Workspace As", Menu::Window, Some("workspace.save_as")),
    context_info(AppCommand::RenameWorkspace, "workspace.rename", "Rename Workspace", Menu::Window, Some("workspace.rename")),
    context_info(AppCommand::ResetWorkspace, "workspace.reset", "Reset Workspace", Menu::Window, Some("workspace.reset")),
    context_info(AppCommand::DeleteWorkspace, "workspace.delete", "Delete Workspace", Menu::Window, Some("workspace.delete")),
    context_info(AppCommand::SetPanelVisibility, "workspace.set_visibility", "Set Panel Visibility", Menu::Window, Some("workspace.set_visibility")),
    info(AppCommand::About, "help.about", "About encoderCraft", Menu::Help, None, Some("app.about"), false),
];

impl AppCommand {
    pub fn info(self) -> Option<&'static CommandInfo> {
        COMMANDS.iter().find(|c| c.command == self)
    }

    /// Whether the command can run right now.
    pub fn enabled(self, s: &CommandState) -> bool {
        match self {
            AppCommand::StartQueue => s.jobs > 0 && (!s.worker_running || !s.worker_active),
            AppCommand::PauseQueue => s.worker_active,
            AppCommand::StopQueue => s.worker_running,
            AppCommand::ResetSelected
            | AppCommand::DuplicateSelected
            | AppCommand::RemoveSelected
            | AppCommand::MoveSelectedUp
            | AppCommand::MoveSelectedDown
            | AppCommand::SetDestination
            | AppCommand::AddOutputTarget
            | AppCommand::InspectSettings => s.selected > 0,
            AppCommand::ApplyPreset | AppCommand::AddPresetTarget => s.selected > 0 && s.preset_selected,
            AppCommand::DeletePreset => s.preset_deletable,
            AppCommand::RevealOutput | AppCommand::CopyOutputPath => s.has_output,
            AppCommand::RevealSource | AppCommand::CopySourcePath => s.has_source,
            AppCommand::ExportSettings => s.selected > 0,
            AppCommand::StitchSelected => s.selected > 1,
            AppCommand::ClearFinished => s.finished > 0,
            AppCommand::SelectAll => s.jobs > 0,
            _ => true,
        }
    }

    pub fn requires_queue_focus(self) -> bool {
        matches!(
            self,
            AppCommand::StartQueue
                | AppCommand::PauseQueue
                | AppCommand::StopQueue
                | AppCommand::ResetSelected
                | AppCommand::DuplicateSelected
                | AppCommand::RemoveSelected
                | AppCommand::MoveSelectedUp
                | AppCommand::MoveSelectedDown
                | AppCommand::SelectAll
                | AppCommand::StitchSelected
                | AppCommand::SetDestination
                | AppCommand::ClearFinished
        )
    }
}

impl CommandInfo {
    pub fn context_label(&self, context: CommandContextMenu) -> &'static str {
        match (self.command, context) {
            (AppCommand::AddMediaItem, CommandContextMenu::MediaItem) => "Add to Queue",
            (AppCommand::OpenMediaFolder, CommandContextMenu::MediaItem) => "Open Folder",
            (AppCommand::ApplyPreset, CommandContextMenu::Preset) => "Apply to Selected Jobs",
            (AppCommand::AddPresetTarget, CommandContextMenu::Preset) => "Add as Output to Selected Jobs",
            _ => self.label,
        }
    }
}

pub fn menu_commands(menu: Menu) -> impl Iterator<Item = &'static CommandInfo> {
    COMMANDS.iter().filter(move |c| c.menu == menu && c.menu_visible)
}

pub fn context_menu_commands(context: CommandContextMenu) -> impl Iterator<Item = &'static CommandInfo> {
    COMMANDS.iter().filter(move |info| match context {
        CommandContextMenu::QueueRow => matches!(
            info.command,
            AppCommand::RevealOutput
                | AppCommand::CopyOutputPath
                | AppCommand::RevealSource
                | AppCommand::CopySourcePath
                | AppCommand::ExportSettings
                | AppCommand::ResetSelected
                | AppCommand::DuplicateSelected
                | AppCommand::RemoveSelected
                | AppCommand::MoveSelectedUp
                | AppCommand::MoveSelectedDown
                | AppCommand::StitchSelected
                | AppCommand::SetDestination
        ),
        CommandContextMenu::Preset => matches!(info.command, AppCommand::ApplyPreset | AppCommand::AddPresetTarget | AppCommand::DeletePreset),
        CommandContextMenu::MediaItem => {
            matches!(info.command, AppCommand::AddMediaItem | AppCommand::OpenMediaFolder | AppCommand::ToggleMediaFavorite)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [AppCommand; 46] = [
        AppCommand::AddMedia,
        AppCommand::AddMediaItem,
        AppCommand::OpenMediaFolder,
        AppCommand::ToggleMediaFavorite,
        AppCommand::RevealOutput,
        AppCommand::CopyOutputPath,
        AppCommand::RevealSource,
        AppCommand::CopySourcePath,
        AppCommand::ExportSettings,
        AppCommand::AddStitched,
        AppCommand::AddWatchFolder,
        AppCommand::ShowLog,
        AppCommand::Preferences,
        AppCommand::ThemeDark,
        AppCommand::ThemeLight,
        AppCommand::ThemeHighContrast,
        AppCommand::StartQueue,
        AppCommand::PauseQueue,
        AppCommand::StopQueue,
        AppCommand::ResetSelected,
        AppCommand::DuplicateSelected,
        AppCommand::RemoveSelected,
        AppCommand::MoveSelectedUp,
        AppCommand::MoveSelectedDown,
        AppCommand::SelectAll,
        AppCommand::StitchSelected,
        AppCommand::SetDestination,
        AppCommand::AddOutputTarget,
        AppCommand::InspectSettings,
        AppCommand::ClearFinished,
        AppCommand::CreatePreset,
        AppCommand::ImportPresets,
        AppCommand::ExportPresets,
        AppCommand::ApplyPreset,
        AppCommand::AddPresetTarget,
        AppCommand::DeletePreset,
        AppCommand::FocusNextPanel,
        AppCommand::FocusPreviousPanel,
        AppCommand::ToggleFocusedPanel,
        AppCommand::ActivateWorkspace,
        AppCommand::SaveWorkspaceAs,
        AppCommand::RenameWorkspace,
        AppCommand::ResetWorkspace,
        AppCommand::DeleteWorkspace,
        AppCommand::SetPanelVisibility,
        AppCommand::About,
    ];

    #[test]
    fn every_command_is_in_the_table_exactly_once() {
        for c in ALL {
            assert_eq!(COMMANDS.iter().filter(|i| i.command == c).count(), 1, "{c:?}");
        }
        assert_eq!(COMMANDS.len(), ALL.len());
    }

    #[test]
    fn ids_and_shortcuts_are_unique() {
        let ids: HashSet<_> = COMMANDS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), COMMANDS.len());
        let keys: Vec<_> = COMMANDS.iter().filter_map(|c| c.shortcut).map(|s| (s.primary, s.shift, s.alt, s.key)).collect();
        let unique: HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "shortcut conflict");
    }

    #[test]
    fn engine_ids_exist_in_the_engine_registry() {
        let known: HashSet<_> = encodercraft_engine::cmd::CommandRegistry::new().list().into_iter().map(|(id, _, _)| id).collect();
        for c in COMMANDS {
            let e = c.engine_id.expect("every application command must have a shared engine command");
            assert!(known.contains(e), "{} maps to unknown engine command {e}", c.id);
        }
        assert_eq!(AppCommand::PauseQueue.info().and_then(|info| info.engine_id), Some("queue.pause_worker"));
    }

    #[test]
    fn queue_commands_are_scoped_to_the_queue_panel() {
        assert!(AppCommand::MoveSelectedUp.requires_queue_focus());
        assert!(AppCommand::StartQueue.requires_queue_focus());
        assert!(!AppCommand::AddMedia.requires_queue_focus());
    }

    #[test]
    fn every_menu_has_entries() {
        for m in Menu::ALL {
            if m != Menu::Window {
                assert!(menu_commands(m).next().is_some(), "{m:?} is empty");
            }
        }
    }

    #[test]
    fn context_menus_are_built_from_registered_commands() {
        let queue: Vec<_> = context_menu_commands(CommandContextMenu::QueueRow).map(|info| info.command).collect();
        assert_eq!(
            queue,
            vec![
                AppCommand::RevealOutput,
                AppCommand::CopyOutputPath,
                AppCommand::RevealSource,
                AppCommand::CopySourcePath,
                AppCommand::ExportSettings,
                AppCommand::ResetSelected,
                AppCommand::DuplicateSelected,
                AppCommand::RemoveSelected,
                AppCommand::MoveSelectedUp,
                AppCommand::MoveSelectedDown,
                AppCommand::StitchSelected,
                AppCommand::SetDestination,
            ]
        );
        assert_eq!(
            context_menu_commands(CommandContextMenu::Preset).map(|info| info.command).collect::<Vec<_>>(),
            vec![AppCommand::ApplyPreset, AppCommand::AddPresetTarget, AppCommand::DeletePreset]
        );
        assert_eq!(
            context_menu_commands(CommandContextMenu::MediaItem).map(|info| info.command).collect::<Vec<_>>(),
            vec![AppCommand::AddMediaItem, AppCommand::OpenMediaFolder, AppCommand::ToggleMediaFavorite]
        );
        assert!(!menu_commands(Menu::File).any(|info| info.command == AppCommand::AddMediaItem));
    }

    #[test]
    fn availability_follows_queue_state() {
        let empty = CommandState::default();
        assert!(!AppCommand::StartQueue.enabled(&empty));
        assert!(!AppCommand::StartQueue.enabled(&CommandState { jobs: 1, worker_running: true, worker_active: true, ..empty }));
        assert!(AppCommand::StartQueue.enabled(&CommandState { jobs: 1, worker_running: true, ..empty }));
        assert!(AppCommand::PauseQueue.enabled(&CommandState { worker_active: true, ..empty }));
        assert!(!AppCommand::PauseQueue.enabled(&CommandState { worker_running: true, ..empty }));
        assert!(!AppCommand::StopQueue.enabled(&empty));
        assert!(AppCommand::StopQueue.enabled(&CommandState { worker_running: true, ..empty }));
        assert!(!AppCommand::StitchSelected.enabled(&CommandState { selected: 1, ..empty }));
        assert!(AppCommand::StitchSelected.enabled(&CommandState { selected: 2, ..empty }));
        assert!(!AppCommand::ApplyPreset.enabled(&CommandState { selected: 1, ..empty }));
        assert!(AppCommand::ApplyPreset.enabled(&CommandState { selected: 1, preset_selected: true, ..empty }));
        assert!(AppCommand::ClearFinished.enabled(&CommandState { finished: 1, ..empty }));
        assert!(AppCommand::AddMedia.enabled(&empty));
    }

    #[test]
    fn labels_render_with_the_bundled_fonts() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let font = egui::FontId::proportional(14.0);
        for c in COMMANDS {
            let text = format!("{} {}", c.label, c.shortcut.map(|s| s.display()).unwrap_or_default());
            for ch in text.chars() {
                assert!(ctx.fonts_mut(|f| f.has_glyph(&font, ch)), "'{ch}' in '{}' has no glyph", c.label);
            }
        }
    }

    #[test]
    fn shortcut_text_is_platform_specific() {
        assert!(Shortcut::cmd(Key::I).display().ends_with("+I"));
    }
}
