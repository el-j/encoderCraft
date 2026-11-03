//! The GUI command table: every menu item and keyboard shortcut is one `AppCommand`.
//!
//! Queue-level actions map to the engine `CommandSpec` ids so the GUI, CLI, IPC and MCP
//! run the same operations; dialogs and view state are GUI-only commands.

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
    AddStitched,
    AddWatchFolder,
    AddDemoClip,
    ShowLog,
    Preferences,
    ThemeDark,
    ThemeHighContrast,
    StartQueue,
    PauseQueue,
    StopQueue,
    ResetSelected,
    DuplicateSelected,
    RemoveSelected,
    StitchSelected,
    SetDestination,
    ClearFinished,
    CreatePreset,
    About,
}

/// Primary-modifier shortcut (Cmd on macOS, Ctrl elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub primary: bool,
    pub shift: bool,
    pub key: Key,
}

impl Shortcut {
    const fn cmd(key: Key) -> Self {
        Self { primary: true, shift: false, key }
    }

    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.primary {
            out.push_str(if cfg!(target_os = "macos") { "Cmd+" } else { "Ctrl+" });
        }
        if self.shift {
            out.push_str("Shift+");
        }
        out.push_str(&match self.key {
            Key::Comma => ",".to_string(),
            Key::Period => ".".to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Delete => "Del".to_string(),
            other => format!("{other:?}"),
        });
        out
    }

    /// True when this shortcut was pressed with exactly its modifiers.
    pub fn pressed(&self, input: &egui::InputState) -> bool {
        let m: Modifiers = input.modifiers;
        m.command == self.primary && m.shift == self.shift && !m.alt && input.key_pressed(self.key)
    }
}

/// Facts about the app that decide which commands are available.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandState {
    pub selected: usize,
    pub jobs: usize,
    pub finished: usize,
    pub worker_running: bool,
}

pub struct CommandInfo {
    pub command: AppCommand,
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Menu,
    pub shortcut: Option<Shortcut>,
    /// Matching engine command id when the action is also available over CLI/IPC/MCP.
    pub engine_id: Option<&'static str>,
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
    CommandInfo { command, id, label, menu, shortcut, engine_id, separator_before }
}

pub const COMMANDS: &[CommandInfo] = &[
    info(AppCommand::AddMedia, "file.add_media", "Add Media Files...", Menu::File, Some(Shortcut::cmd(Key::I)), Some("queue.add"), false),
    info(AppCommand::AddStitched, "file.add_stitched", "Add Stitched Sequence...", Menu::File, None, Some("queue.stitch"), false),
    info(AppCommand::AddWatchFolder, "file.add_watch_folder", "Add Watch Folder...", Menu::File, None, None, false),
    info(AppCommand::ShowLog, "file.show_log", "Show Encoding Log...", Menu::File, Some(Shortcut::cmd(Key::L)), Some("queue.log"), true),
    info(AppCommand::AddDemoClip, "file.demo_clip", "Quick Add Demo Clip", Menu::File, None, Some("queue.add"), true),
    info(AppCommand::Preferences, "edit.preferences", "Preferences...", Menu::Edit, Some(Shortcut::cmd(Key::Comma)), None, false),
    info(AppCommand::ThemeDark, "edit.theme_dark", "Theme: Dark", Menu::Edit, None, None, true),
    info(AppCommand::ThemeHighContrast, "edit.theme_high_contrast", "Theme: High Contrast", Menu::Edit, None, None, false),
    info(AppCommand::StartQueue, "queue.start", "Start Queue", Menu::Queue, Some(Shortcut::cmd(Key::Enter)), Some("queue.start"), false),
    info(AppCommand::PauseQueue, "queue.pause", "Pause Queue", Menu::Queue, Some(Shortcut::cmd(Key::Period)), Some("queue.pause"), false),
    info(AppCommand::StopQueue, "queue.stop", "Stop Queue", Menu::Queue, None, Some("queue.stop_worker"), false),
    info(AppCommand::ResetSelected, "queue.reset", "Reset Selected", Menu::Queue, None, Some("queue.reset"), true),
    info(
        AppCommand::DuplicateSelected,
        "queue.duplicate",
        "Duplicate Selected",
        Menu::Queue,
        Some(Shortcut::cmd(Key::D)),
        Some("queue.duplicate"),
        false,
    ),
    info(AppCommand::RemoveSelected, "queue.remove", "Remove Selected", Menu::Queue, Some(Shortcut::cmd(Key::Delete)), Some("queue.remove"), false),
    info(AppCommand::StitchSelected, "queue.stitch_selected", "Stitch Selected Clips...", Menu::Queue, None, Some("queue.stitch"), true),
    info(AppCommand::SetDestination, "queue.set_destination", "Set Destination Directory...", Menu::Queue, None, None, false),
    info(AppCommand::ClearFinished, "queue.clear_finished", "Clear Finished Jobs", Menu::Queue, None, Some("queue.clear_finished"), true),
    info(AppCommand::CreatePreset, "preset.create", "Create Encoding Preset...", Menu::Preset, None, None, false),
    info(AppCommand::About, "help.about", "About encoderCraft", Menu::Help, None, None, false),
];

impl AppCommand {
    pub fn info(self) -> Option<&'static CommandInfo> {
        COMMANDS.iter().find(|c| c.command == self)
    }

    /// Whether the command can run right now.
    pub fn enabled(self, s: &CommandState) -> bool {
        match self {
            AppCommand::StartQueue => s.jobs > 0,
            AppCommand::PauseQueue | AppCommand::StopQueue => s.worker_running,
            AppCommand::ResetSelected | AppCommand::DuplicateSelected | AppCommand::RemoveSelected | AppCommand::SetDestination => s.selected > 0,
            AppCommand::StitchSelected => s.selected > 1,
            AppCommand::ClearFinished => s.finished > 0,
            _ => true,
        }
    }
}

pub fn menu_commands(menu: Menu) -> impl Iterator<Item = &'static CommandInfo> {
    COMMANDS.iter().filter(move |c| c.menu == menu)
}

/// Commands whose shortcut was pressed this frame. Plain-key shortcuts are skipped while a text field has focus.
pub fn triggered(ctx: &egui::Context) -> Vec<AppCommand> {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input(|i| COMMANDS.iter().filter(|c| c.shortcut.is_some_and(|s| (s.primary || !typing) && s.pressed(i))).map(|c| c.command).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [AppCommand; 19] = [
        AppCommand::AddMedia,
        AppCommand::AddStitched,
        AppCommand::AddWatchFolder,
        AppCommand::AddDemoClip,
        AppCommand::ShowLog,
        AppCommand::Preferences,
        AppCommand::ThemeDark,
        AppCommand::ThemeHighContrast,
        AppCommand::StartQueue,
        AppCommand::PauseQueue,
        AppCommand::StopQueue,
        AppCommand::ResetSelected,
        AppCommand::DuplicateSelected,
        AppCommand::RemoveSelected,
        AppCommand::StitchSelected,
        AppCommand::SetDestination,
        AppCommand::ClearFinished,
        AppCommand::CreatePreset,
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
        let keys: Vec<_> = COMMANDS.iter().filter_map(|c| c.shortcut).map(|s| (s.primary, s.shift, s.key)).collect();
        let unique: HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "shortcut conflict");
    }

    #[test]
    fn engine_ids_exist_in_the_engine_registry() {
        let known: HashSet<_> = encodercraft_engine::cmd::CommandRegistry::new().list().into_iter().map(|(id, _, _)| id).collect();
        for c in COMMANDS {
            if let Some(e) = c.engine_id {
                assert!(known.contains(e), "{} maps to unknown engine command {e}", c.id);
            }
        }
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
    fn availability_follows_queue_state() {
        let empty = CommandState::default();
        assert!(!AppCommand::StartQueue.enabled(&empty));
        assert!(!AppCommand::StopQueue.enabled(&empty));
        assert!(!AppCommand::StitchSelected.enabled(&CommandState { selected: 1, ..empty }));
        assert!(AppCommand::StitchSelected.enabled(&CommandState { selected: 2, ..empty }));
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
