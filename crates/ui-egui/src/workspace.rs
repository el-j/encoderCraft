//! Named workspaces: which panels are shown in the right dock, their order and the dock width.
//!
//! Layouts are plain data so they can be saved, validated on load and restored without panics.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_DOCK_WIDTH: f32 = 260.0;
pub const MAX_DOCK_WIDTH: f32 = 720.0;
pub const DEFAULT_WORKSPACE: &str = "Default";
pub const QUEUE_WORKSPACE: &str = "Queue Focused";
pub const PREVIEW_WORKSPACE: &str = "Preview Focused";

/// A panel that can be shown in the right dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    Preview,
    Encoding,
    Presets,
}

impl PanelId {
    pub const ALL: [PanelId; 3] = [PanelId::Preview, PanelId::Encoding, PanelId::Presets];

    pub fn label(self) -> &'static str {
        match self {
            PanelId::Preview => "Preview Monitor",
            PanelId::Encoding => "Encoding",
            PanelId::Presets => "Preset Browser",
        }
    }
}

/// One saved arrangement of the right dock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub dock_width: f32,
    /// Panels shown in the dock, top to bottom.
    pub panels: Vec<PanelId>,
}

impl Workspace {
    pub fn is_visible(&self, panel: PanelId) -> bool {
        self.panels.contains(&panel)
    }

    pub fn toggle(&mut self, panel: PanelId) {
        if let Some(i) = self.panels.iter().position(|p| *p == panel) {
            self.panels.remove(i);
        } else {
            self.panels.push(panel);
        }
    }

    /// Moves a visible panel one slot up (`-1`) or down (`1`).
    pub fn shift(&mut self, panel: PanelId, delta: isize) {
        let Some(i) = self.panels.iter().position(|p| *p == panel) else { return };
        let Some(j) = i.checked_add_signed(delta).filter(|j| *j < self.panels.len()) else { return };
        self.panels.swap(i, j);
    }

    /// Repairs a layout that came from disk: removes duplicates and bad widths.
    fn sanitize(&mut self) {
        let mut seen = Vec::new();
        self.panels.retain(|p| {
            let fresh = !seen.contains(p);
            seen.push(*p);
            fresh
        });
        self.dock_width = if self.dock_width.is_finite() { self.dock_width.clamp(MIN_DOCK_WIDTH, MAX_DOCK_WIDTH) } else { 360.0 };
        if self.name.trim().is_empty() {
            self.name = "Workspace".into();
        }
    }
}

fn builtins() -> Vec<Workspace> {
    vec![
        Workspace { name: DEFAULT_WORKSPACE.into(), dock_width: 360.0, panels: vec![PanelId::Preview, PanelId::Encoding, PanelId::Presets] },
        Workspace { name: QUEUE_WORKSPACE.into(), dock_width: 280.0, panels: vec![PanelId::Encoding, PanelId::Presets] },
        Workspace { name: PREVIEW_WORKSPACE.into(), dock_width: 560.0, panels: vec![PanelId::Preview, PanelId::Encoding] },
    ]
}

fn is_builtin(name: &str) -> bool {
    [DEFAULT_WORKSPACE, QUEUE_WORKSPACE, PREVIEW_WORKSPACE].contains(&name)
}

/// All workspaces plus the active one. Edits to the active workspace are live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSet {
    pub active: String,
    pub workspaces: Vec<Workspace>,
}

impl Default for WorkspaceSet {
    fn default() -> Self {
        Self { active: DEFAULT_WORKSPACE.into(), workspaces: builtins() }
    }
}

impl WorkspaceSet {
    pub fn current(&self) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.name == self.active)
    }

    pub fn current_mut(&mut self) -> Option<&mut Workspace> {
        let active = self.active.clone();
        self.workspaces.iter_mut().find(|w| w.name == active)
    }

    pub fn names(&self) -> Vec<String> {
        self.workspaces.iter().map(|w| w.name.clone()).collect()
    }

    pub fn activate(&mut self, name: &str) -> bool {
        let found = self.workspaces.iter().any(|w| w.name == name);
        if found {
            self.active = name.to_string();
        }
        found
    }

    /// Saves the active layout under `name` (overwriting a custom one) and activates it.
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Workspace name cannot be empty".into());
        }
        if is_builtin(name) {
            return Err(format!("'{name}' is a built-in workspace; choose another name"));
        }
        let mut layout = self.current().cloned().unwrap_or_else(|| builtins().remove(0));
        layout.name = name.to_string();
        match self.workspaces.iter_mut().find(|w| w.name == name) {
            Some(slot) => *slot = layout,
            None => self.workspaces.push(layout),
        }
        self.active = name.to_string();
        Ok(())
    }

    pub fn rename_active(&mut self, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if is_builtin(&self.active) {
            return Err("Built-in workspaces cannot be renamed".into());
        }
        if new_name.is_empty() || is_builtin(new_name) || self.workspaces.iter().any(|w| w.name == new_name) {
            return Err(format!("'{new_name}' is not available as a workspace name"));
        }
        let active = self.active.clone();
        if let Some(w) = self.workspaces.iter_mut().find(|w| w.name == active) {
            w.name = new_name.to_string();
            self.active = new_name.to_string();
        }
        Ok(())
    }

    /// Deletes the active custom workspace and falls back to Default.
    pub fn delete_active(&mut self) -> Result<(), String> {
        if is_builtin(&self.active) {
            return Err("Built-in workspaces cannot be deleted".into());
        }
        let active = self.active.clone();
        self.workspaces.retain(|w| w.name != active);
        self.active = DEFAULT_WORKSPACE.into();
        Ok(())
    }

    /// Restores the active workspace to its built-in layout, or Default for custom ones.
    pub fn reset_active(&mut self) {
        let defaults = builtins();
        let source = defaults.iter().find(|w| w.name == self.active).or_else(|| defaults.first()).cloned();
        if let (Some(mut fresh), Some(slot)) = (source, self.current_mut()) {
            fresh.name = slot.name.clone();
            *slot = fresh;
        }
    }

    /// Makes any deserialised set safe to use: built-ins present, layouts repaired, active valid.
    pub fn sanitize(&mut self) {
        for w in &mut self.workspaces {
            w.sanitize();
        }
        let mut seen: Vec<String> = Vec::new();
        self.workspaces.retain(|w| {
            let fresh = !seen.contains(&w.name);
            seen.push(w.name.clone());
            fresh
        });
        for builtin in builtins() {
            if !self.workspaces.iter().any(|w| w.name == builtin.name) {
                self.workspaces.push(builtin);
            }
        }
        if !self.workspaces.iter().any(|w| w.name == self.active) {
            self.active = DEFAULT_WORKSPACE.into();
        }
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    /// Parses saved JSON. Corrupt data yields the defaults instead of an error.
    pub fn from_json(text: &str) -> Self {
        match serde_json::from_str::<WorkspaceSet>(text) {
            Ok(mut set) => {
                set.sanitize();
                set
            }
            Err(e) => {
                log::warn!("ignoring unreadable workspace layout: {e}");
                Self::default()
            }
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(_) => Self::default(),
        }
    }

    /// Writes atomically so a crash never leaves a half-written layout.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

/// Per-user location of the saved layouts, if a home or config directory is known.
pub fn default_layout_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("encoderCraft").join("workspaces.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_preserves_custom_layouts() {
        let mut set = WorkspaceSet::default();
        set.activate(QUEUE_WORKSPACE);
        set.save_as("Mine").unwrap();
        if let Some(w) = set.current_mut() {
            w.toggle(PanelId::Preview);
            w.dock_width = 400.0;
        }
        let back = WorkspaceSet::from_json(&set.to_json().unwrap());
        assert_eq!(back, set);
        assert!(back.current().unwrap().is_visible(PanelId::Preview));
    }

    #[test]
    fn corrupt_or_hostile_layouts_recover() {
        assert_eq!(WorkspaceSet::from_json("{not json"), WorkspaceSet::default());
        let hostile = r#"{"active":"ghost","workspaces":[{"name":"","dock_width":1e30,"panels":["Preview","Preview"]}]}"#;
        let set = WorkspaceSet::from_json(hostile);
        assert_eq!(set.active, DEFAULT_WORKSPACE);
        assert!(set.current().is_some());
        let w = set.workspaces.iter().find(|w| w.name == "Workspace").unwrap();
        assert_eq!(w.panels, vec![PanelId::Preview]);
        assert_eq!(w.dock_width, MAX_DOCK_WIDTH);
        assert!(set.workspaces.len() >= 4);
    }

    #[test]
    fn builtins_are_protected_and_custom_ones_managed() {
        let mut set = WorkspaceSet::default();
        assert!(set.save_as(DEFAULT_WORKSPACE).is_err());
        assert!(set.rename_active("X").is_err());
        assert!(set.delete_active().is_err());
        set.save_as("A").unwrap();
        set.rename_active("B").unwrap();
        assert_eq!(set.active, "B");
        set.delete_active().unwrap();
        assert_eq!(set.active, DEFAULT_WORKSPACE);
        assert!(!set.names().contains(&"B".to_string()));
    }

    #[test]
    fn reset_restores_builtin_layout() {
        let mut set = WorkspaceSet::default();
        if let Some(w) = set.current_mut() {
            w.panels.clear();
            w.dock_width = 500.0;
        }
        set.reset_active();
        assert_eq!(set.current().unwrap().panels.len(), 3);
        assert_eq!(set.current().unwrap().dock_width, 360.0);
    }

    #[test]
    fn panels_toggle_and_reorder() {
        let mut w = builtins().remove(0);
        w.shift(PanelId::Preview, 1);
        assert_eq!(w.panels[1], PanelId::Preview);
        w.shift(PanelId::Preview, 5);
        w.shift(PanelId::Presets, -9);
        assert_eq!(w.panels.len(), 3);
        w.toggle(PanelId::Encoding);
        assert!(!w.is_visible(PanelId::Encoding));
        w.toggle(PanelId::Encoding);
        assert!(w.is_visible(PanelId::Encoding));
    }

    #[test]
    fn save_and_load_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/workspaces.json");
        let mut set = WorkspaceSet::default();
        set.save_as("Disk").unwrap();
        set.save(&path).unwrap();
        assert_eq!(WorkspaceSet::load(&path), set);
        assert_eq!(WorkspaceSet::load(&dir.path().join("missing.json")), WorkspaceSet::default());
    }
}
