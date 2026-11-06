//! Per-user settings files: preferences, watch folders and workspaces live in one config directory.

use craft_keymap::{Chord, Key, Mods, Platform};
use encodercraft_watch::WatchFolderConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const WATCH_SCHEMA_VERSION: u32 = 1;
const MAX_SHORTCUTS_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceShortcutAction {
    FocusNext,
    FocusPrevious,
    ToggleFocusedPanel,
}

impl WorkspaceShortcutAction {
    pub(crate) const ALL: [Self; 3] = [Self::FocusNext, Self::FocusPrevious, Self::ToggleFocusedPanel];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::FocusNext => "Focus Next Panel",
            Self::FocusPrevious => "Focus Previous Panel",
            Self::ToggleFocusedPanel => "Toggle Focused Panel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceShortcuts {
    pub focus_next: String,
    pub focus_previous: String,
    pub toggle_focused_panel: String,
    #[serde(default)]
    app_commands: HashMap<String, String>,
}

impl Default for WorkspaceShortcuts {
    fn default() -> Self {
        Self { focus_next: "F6".into(), focus_previous: "Shift+F6".into(), toggle_focused_panel: "Cmd+Shift+P".into(), app_commands: HashMap::new() }
    }
}

impl WorkspaceShortcuts {
    fn action_for_command(command: crate::commands::AppCommand) -> Option<WorkspaceShortcutAction> {
        match command {
            crate::commands::AppCommand::FocusNextPanel => Some(WorkspaceShortcutAction::FocusNext),
            crate::commands::AppCommand::FocusPreviousPanel => Some(WorkspaceShortcutAction::FocusPrevious),
            crate::commands::AppCommand::ToggleFocusedPanel => Some(WorkspaceShortcutAction::ToggleFocusedPanel),
            _ => None,
        }
    }

    fn default_command_chord(command: crate::commands::AppCommand) -> Option<Chord> {
        let info = command.info()?;
        if !info.shortcut_configurable {
            return None;
        }
        let shortcut = info.shortcut?;
        let key = Key::from_egui(shortcut.key)?;
        Some(Chord::new(Mods { cmd: shortcut.primary, shift: shortcut.shift, alt: shortcut.alt, ..Mods::NONE }, key))
    }

    pub fn command_chord(&self, command: crate::commands::AppCommand) -> Option<Chord> {
        if let Some(action) = Self::action_for_command(command) {
            return Chord::parse(self.binding(action)).ok();
        }
        let id = command.info()?.id;
        if !command.info()?.shortcut_configurable {
            return None;
        }
        self.app_commands.get(id).map_or_else(|| Self::default_command_chord(command), |binding| Chord::parse(binding).ok())
    }

    pub fn command_display(&self, command: crate::commands::AppCommand) -> Option<String> {
        self.command_chord(command).map(|chord| chord.display(Platform::current()))
    }

    pub fn binding(&self, action: WorkspaceShortcutAction) -> &str {
        match action {
            WorkspaceShortcutAction::FocusNext => &self.focus_next,
            WorkspaceShortcutAction::FocusPrevious => &self.focus_previous,
            WorkspaceShortcutAction::ToggleFocusedPanel => &self.toggle_focused_panel,
        }
    }

    pub fn set_binding(&mut self, action: WorkspaceShortcutAction, binding: String) -> Result<(), String> {
        let chord = Chord::parse(&binding).map_err(|error| error.to_string())?;
        for other in WorkspaceShortcutAction::ALL {
            if other != action
                && Chord::parse(self.binding(other))
                    .is_ok_and(|existing| existing.effective(Platform::current()) == chord.effective(Platform::current()))
            {
                return Err(format!("{} already uses {}", other.label(), chord.display(Platform::current())));
            }
        }
        for info in crate::commands::COMMANDS {
            if Self::action_for_command(info.command).is_some() {
                continue;
            }
            if self
                .command_chord(info.command)
                .is_some_and(|existing| existing.effective(Platform::current()) == chord.effective(Platform::current()))
            {
                return Err(format!("{} already uses {}", info.label, chord.display(Platform::current())));
            }
        }
        match action {
            WorkspaceShortcutAction::FocusNext => self.focus_next = binding,
            WorkspaceShortcutAction::FocusPrevious => self.focus_previous = binding,
            WorkspaceShortcutAction::ToggleFocusedPanel => self.toggle_focused_panel = binding,
        }
        Ok(())
    }

    pub fn set_command_binding(&mut self, command: crate::commands::AppCommand, binding: String) -> Result<(), String> {
        let chord = Chord::parse(&binding).map_err(|error| error.to_string())?;
        if let Some(action) = Self::action_for_command(command) {
            return self.set_binding(action, binding);
        }
        let info = command.info().ok_or_else(|| "Unknown application command.".to_owned())?;
        if !info.shortcut_configurable {
            return Err(format!("{} is only available from its context menu.", info.label));
        }
        for action in WorkspaceShortcutAction::ALL {
            if Chord::parse(self.binding(action))
                .is_ok_and(|existing| existing.effective(Platform::current()) == chord.effective(Platform::current()))
            {
                return Err(format!("{} already uses {}", action.label(), chord.display(Platform::current())));
            }
        }
        for other in crate::commands::COMMANDS {
            if other.command != command
                && Self::action_for_command(other.command).is_none()
                && self
                    .command_chord(other.command)
                    .is_some_and(|existing| existing.effective(Platform::current()) == chord.effective(Platform::current()))
            {
                return Err(format!("{} already uses {}", other.label, chord.display(Platform::current())));
            }
        }
        self.app_commands.insert(info.id.to_owned(), binding);
        Ok(())
    }

    pub fn triggered_commands(&self, ctx: &egui::Context) -> Vec<crate::commands::AppCommand> {
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input(|input| {
            crate::commands::COMMANDS
                .iter()
                .filter_map(|info| {
                    if Self::action_for_command(info.command).is_some() {
                        return None;
                    }
                    if !info.shortcut_configurable {
                        return None;
                    }
                    let chord = self.command_chord(info.command)?;
                    if typing && !chord.mods.cmd && !chord.mods.ctrl {
                        return None;
                    }
                    input
                        .events
                        .iter()
                        .any(|event| {
                            let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event else { return false };
                            craft_keymap::egui::matches(&chord, *key, modifiers, Platform::current())
                        })
                        .then_some(info.command)
                })
                .collect()
        })
    }

    pub fn matches(&self, action: WorkspaceShortcutAction, key: egui::Key, modifiers: &egui::Modifiers) -> bool {
        Chord::parse(self.binding(action)).is_ok_and(|chord| craft_keymap::egui::matches(&chord, key, modifiers, Platform::current()))
    }

    pub fn display(&self, action: WorkspaceShortcutAction) -> String {
        Chord::parse(self.binding(action)).map_or_else(|_| "Invalid shortcut".into(), |chord| chord.display(Platform::current()))
    }

    pub fn capture(event: &egui::Event) -> Option<String> {
        let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event else { return None };
        let key = Key::from_egui(*key)?;
        let mods = Mods::from_egui(modifiers, Platform::current());
        Some(Chord::new(mods, key).portable())
    }

    pub fn validate(&self) -> Result<(), String> {
        for action in WorkspaceShortcutAction::ALL {
            let mut validated = self.clone();
            validated.set_binding(action, self.binding(action).to_owned())?;
        }
        for (id, binding) in &self.app_commands {
            let Some(info) = crate::commands::COMMANDS.iter().find(|info| info.id == id.as_str()) else {
                return Err(format!("Unknown application command shortcut `{id}`."));
            };
            if Self::action_for_command(info.command).is_some() {
                return Err(format!("Workspace command shortcut `{id}` must use its workspace binding."));
            }
            if !info.shortcut_configurable {
                return Err(format!("Command shortcut `{id}` is only available from its context menu."));
            }
            let mut validated = self.clone();
            validated.set_command_binding(info.command, binding.clone())?;
        }
        Ok(())
    }

    pub fn from_json(text: &str) -> Self {
        if text.len() as u64 > MAX_SHORTCUTS_BYTES {
            log::warn!("keyboard shortcuts file exceeds the {MAX_SHORTCUTS_BYTES}-byte limit; using defaults");
            return Self::default();
        }
        match serde_json::from_str::<Self>(text) {
            Ok(shortcuts) if shortcuts.validate().is_ok() => shortcuts,
            Ok(_) => {
                log::warn!("ignoring invalid or conflicting keyboard shortcuts; using defaults");
                Self::default()
            }
            Err(error) => {
                log::warn!("ignoring unreadable keyboard shortcuts: {error}");
                Self::default()
            }
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.len() > MAX_SHORTCUTS_BYTES => {
                log::warn!("keyboard shortcuts file exceeds the {MAX_SHORTCUTS_BYTES}-byte limit; using defaults");
                Self::default()
            }
            Ok(_) => match std::fs::read_to_string(path) {
                Ok(text) => Self::from_json(&text),
                Err(error) => {
                    log::warn!("could not read keyboard shortcuts from {}: {error}", path.display());
                    Self::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                log::warn!("could not inspect keyboard shortcuts at {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        if json.len() as u64 > MAX_SHORTCUTS_BYTES {
            return Err(format!("keyboard shortcuts file exceeds the {MAX_SHORTCUTS_BYTES}-byte limit"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, path).map_err(|error| error.to_string())
    }
}

#[derive(Serialize, Deserialize)]
struct WatchFile {
    version: u32,
    folders: Vec<WatchFolderConfig>,
}

/// The per-user configuration directory, if the platform exposes one.
pub fn default_config_dir() -> Option<PathBuf> {
    encodercraft_engine::default_config_dir()
}

pub fn preferences_path(dir: &Path) -> PathBuf {
    dir.join("preferences.json")
}

pub fn workspaces_path(dir: &Path) -> PathBuf {
    dir.join("workspaces.json")
}

pub fn shortcuts_path(dir: &Path) -> PathBuf {
    dir.join("keyboard_shortcuts.json")
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

    #[test]
    fn workspace_shortcuts_are_customizable_and_invalid_files_restore_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = shortcuts_path(dir.path());
        let mut shortcuts = WorkspaceShortcuts::default();
        shortcuts.set_binding(WorkspaceShortcutAction::FocusNext, "Alt+F8".into()).unwrap();
        shortcuts.save(&path).unwrap();
        assert_eq!(WorkspaceShortcuts::load(&path), shortcuts);
        let captured = WorkspaceShortcuts::capture(&egui::Event::Key {
            key: egui::Key::F8,
            physical_key: Some(egui::Key::F8),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { alt: true, ..Default::default() },
        });
        assert_eq!(captured.as_deref(), Some("Alt+F8"));
        assert!(shortcuts.matches(WorkspaceShortcutAction::FocusNext, egui::Key::F8, &egui::Modifiers { alt: true, ..Default::default() }));
        assert!(shortcuts.set_binding(WorkspaceShortcutAction::ToggleFocusedPanel, "Alt+F8".into()).is_err());
        assert!(shortcuts.set_binding(WorkspaceShortcutAction::ToggleFocusedPanel, "Cmd+I".into()).is_err());
        std::fs::write(&path, r#"{"focus_next":"Alt+F8","focus_previous":"Alt+F8","toggle_focused_panel":"Cmd+Shift+P"}"#).unwrap();
        assert_eq!(WorkspaceShortcuts::load(&path), WorkspaceShortcuts::default());
        assert_eq!(WorkspaceShortcuts::from_json("{bad"), WorkspaceShortcuts::default());
    }

    #[test]
    fn app_command_shortcuts_persist_and_dispatch_with_conflict_checks() {
        let dir = tempfile::tempdir().unwrap();
        let path = shortcuts_path(dir.path());
        let mut shortcuts = WorkspaceShortcuts::default();
        shortcuts.set_command_binding(crate::commands::AppCommand::AddMedia, "Cmd+M".into()).unwrap();

        assert_eq!(
            shortcuts.command_display(crate::commands::AppCommand::AddMedia).as_deref(),
            Some(Chord::parse("Cmd+M").unwrap().display(Platform::current()).as_str())
        );
        assert!(shortcuts.set_command_binding(crate::commands::AppCommand::DuplicateSelected, "Cmd+M".into()).is_err());
        assert!(shortcuts.set_binding(WorkspaceShortcutAction::FocusNext, "Cmd+M".into()).is_err());
        assert!(shortcuts.set_command_binding(crate::commands::AppCommand::AddMediaItem, "Cmd+Y".into()).is_err());
        shortcuts.save(&path).unwrap();
        assert_eq!(WorkspaceShortcuts::load(&path), shortcuts);

        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::M,
            physical_key: Some(egui::Key::M),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        let ctx = egui::Context::default();
        let mut triggered = Vec::new();
        let mut output = ctx.run_ui(input, |ui| triggered = shortcuts.triggered_commands(ui.ctx()));
        output.textures_delta.clear();
        assert_eq!(triggered, vec![crate::commands::AppCommand::AddMedia]);
    }

    #[test]
    fn old_shortcut_files_load_and_context_only_bindings_are_rejected() {
        let old_format = r#"{"focus_next":"F6","focus_previous":"Shift+F6","toggle_focused_panel":"Cmd+Shift+P"}"#;
        assert_eq!(WorkspaceShortcuts::from_json(old_format), WorkspaceShortcuts::default());

        let invalid_command_binding =
            r#"{"focus_next":"F6","focus_previous":"Shift+F6","toggle_focused_panel":"Cmd+Shift+P","app_commands":{"media.add_item":"Alt+X"}}"#;
        assert_eq!(WorkspaceShortcuts::from_json(invalid_command_binding), WorkspaceShortcuts::default());
    }
}
