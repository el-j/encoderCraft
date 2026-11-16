//! Named workspaces backed by craft-ui's validated, dockable panel layout.

use craft_ui::docking::{Action, DockError, Layout, Node, Permissions, Workspace as DockWorkspace};
use craft_ui::layout::{SplitAxis, SplitSize};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_WORKSPACE: &str = "Default";
pub const QUEUE_WORKSPACE: &str = "Queue Focused";
pub const PREVIEW_WORKSPACE: &str = "Preview Focused";
const MAX_WORKSPACE_BYTES: u64 = 4 * 1024 * 1024;

/// A panel rendered by the encoderCraft application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    MediaBrowser,
    Presets,
    Queue,
    WatchFolders,
    Preview,
    Encoding,
}

impl PanelId {
    pub const ALL: [PanelId; 6] =
        [PanelId::MediaBrowser, PanelId::Presets, PanelId::Queue, PanelId::WatchFolders, PanelId::Preview, PanelId::Encoding];

    pub fn label(self) -> &'static str {
        match self {
            PanelId::MediaBrowser => "Media Browser",
            PanelId::Presets => "Preset Browser",
            PanelId::Queue => "Queue",
            PanelId::WatchFolders => "Watch Folders",
            PanelId::Preview => "Preview Monitor",
            PanelId::Encoding => "Encoding",
        }
    }
}

/// A user-named workspace and its validated dock layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub dock: DockWorkspace<PanelId>,
}

impl Workspace {
    pub fn is_visible(&self, panel: PanelId) -> bool {
        self.dock.layout.contains(&panel)
    }

    pub fn focus_neighbor(&mut self, current: PanelId, forward: bool) -> Result<Option<PanelId>, DockError> {
        let visible: Vec<_> = PanelId::ALL.into_iter().filter(|panel| self.is_visible(*panel)).collect();
        if visible.is_empty() {
            return Ok(None);
        }
        let index = visible.iter().position(|panel| *panel == current);
        let next = match (index, forward) {
            (Some(index), true) => (index + 1) % visible.len(),
            (Some(index), false) => (index + visible.len() - 1) % visible.len(),
            (None, true) => 0,
            (None, false) => visible.len() - 1,
        };
        let panel = visible[next];
        self.set_visible(panel, true)?;
        Ok(Some(panel))
    }

    pub fn set_visible(&mut self, panel: PanelId, visible: bool) -> Result<(), DockError> {
        let fallback = match panel {
            PanelId::Preview => Some(PanelId::Queue),
            _ => Some(PanelId::Preview),
        };
        self.dock.set_visible(panel, visible, fallback, |_| Permissions::default())
    }

    pub fn apply_actions(&mut self, actions: impl IntoIterator<Item = Action<PanelId>>) -> Result<(), DockError> {
        self.dock.apply_all(actions, |_| Permissions::default()).map(drop)
    }
}

fn tabs(panel: PanelId) -> Node<PanelId> {
    Node::Tabs { panels: vec![panel], active: 0 }
}

fn split(axis: SplitAxis, ratio: f32, first: Node<PanelId>, second: Node<PanelId>) -> Node<PanelId> {
    Node::Split { axis, size: SplitSize::Ratio(ratio), first: Box::new(first), second: Box::new(second) }
}

fn default_layout(name: &str) -> Workspace {
    let left = split(SplitAxis::Vertical, 0.57, tabs(PanelId::MediaBrowser), tabs(PanelId::Presets));
    let queue_encoding =
        split(SplitAxis::Vertical, 0.67, Node::Tabs { panels: vec![PanelId::Queue, PanelId::WatchFolders], active: 0 }, tabs(PanelId::Encoding));
    let layout = match name {
        DEFAULT_WORKSPACE => split(SplitAxis::Horizontal, 0.44, left, queue_encoding),
        QUEUE_WORKSPACE => split(
            SplitAxis::Horizontal,
            0.24,
            split(SplitAxis::Vertical, 0.5, tabs(PanelId::MediaBrowser), tabs(PanelId::Presets)),
            split(
                SplitAxis::Horizontal,
                0.52,
                Node::Tabs { panels: vec![PanelId::Queue, PanelId::WatchFolders], active: 0 },
                tabs(PanelId::Encoding),
            ),
        ),
        PREVIEW_WORKSPACE => split(
            SplitAxis::Horizontal,
            0.2,
            split(SplitAxis::Vertical, 0.5, tabs(PanelId::MediaBrowser), tabs(PanelId::Presets)),
            split(SplitAxis::Horizontal, 0.78, tabs(PanelId::Preview), tabs(PanelId::Encoding)),
        ),
        _ => split(SplitAxis::Horizontal, 0.44, left, queue_encoding),
    };
    Workspace { name: name.into(), dock: DockWorkspace::new(Layout { root: Some(layout), floating: Vec::new() }) }
}

/// The active workspace and the built-in/user-defined layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSet {
    pub active: String,
    pub workspaces: Vec<Workspace>,
}

impl Default for WorkspaceSet {
    fn default() -> Self {
        Self {
            active: DEFAULT_WORKSPACE.into(),
            workspaces: vec![default_layout(DEFAULT_WORKSPACE), default_layout(QUEUE_WORKSPACE), default_layout(PREVIEW_WORKSPACE)],
        }
    }
}

impl WorkspaceSet {
    pub fn current(&self) -> Option<&Workspace> {
        self.workspaces.iter().find(|workspace| workspace.name == self.active)
    }

    pub fn current_mut(&mut self) -> Option<&mut Workspace> {
        let active = self.active.clone();
        self.workspaces.iter_mut().find(|workspace| workspace.name == active)
    }

    pub fn names(&self) -> Vec<String> {
        self.workspaces.iter().map(|workspace| workspace.name.clone()).collect()
    }

    pub fn activate(&mut self, name: &str) -> bool {
        if self.workspaces.iter().any(|workspace| workspace.name == name) {
            self.active = name.into();
            true
        } else {
            false
        }
    }

    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || is_builtin(name) {
            return Err(format!("'{name}' is not available as a workspace name"));
        }
        let mut workspace = self.current().cloned().ok_or_else(|| "no active workspace to copy".to_string())?;
        workspace.name = name.into();
        match self.workspaces.iter_mut().find(|workspace| workspace.name == name) {
            Some(existing) => *existing = workspace,
            None => self.workspaces.push(workspace),
        }
        self.active = name.into();
        Ok(())
    }

    pub fn rename_active(&mut self, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if is_builtin(&self.active)
            || new_name.is_empty()
            || is_builtin(new_name)
            || self.workspaces.iter().any(|workspace| workspace.name == new_name)
        {
            return Err(format!("'{new_name}' is not available as a workspace name"));
        }
        let active = self.active.clone();
        let workspace = self.workspaces.iter_mut().find(|workspace| workspace.name == active).ok_or("active workspace is missing")?;
        workspace.name = new_name.into();
        self.active = new_name.into();
        Ok(())
    }

    pub fn delete_active(&mut self) -> Result<(), String> {
        if is_builtin(&self.active) {
            return Err("Built-in workspaces cannot be deleted".into());
        }
        self.workspaces.retain(|workspace| workspace.name != self.active);
        self.active = DEFAULT_WORKSPACE.into();
        Ok(())
    }

    pub fn reset_active(&mut self) {
        let name = if is_builtin(&self.active) { self.active.clone() } else { DEFAULT_WORKSPACE.into() };
        if let Some(workspace) = self.workspaces.iter_mut().find(|workspace| workspace.name == self.active) {
            *workspace = default_layout(&name);
            self.active = name;
        }
    }

    /// Validates loaded dock trees and restores built-in defaults for invalid entries.
    pub fn sanitize(&mut self) {
        let mut seen = Vec::new();
        self.workspaces.retain(|workspace| {
            let fresh = !workspace.name.trim().is_empty() && !seen.contains(&workspace.name);
            seen.push(workspace.name.clone());
            fresh
        });
        for workspace in &mut self.workspaces {
            if workspace.dock.validate().is_err() {
                log::warn!("invalid docking layout for workspace '{}'; restoring its default", workspace.name);
                *workspace = default_layout(&workspace.name);
            }
        }
        for name in [DEFAULT_WORKSPACE, QUEUE_WORKSPACE, PREVIEW_WORKSPACE] {
            if !self.workspaces.iter().any(|workspace| workspace.name == name) {
                self.workspaces.push(default_layout(name));
            }
        }
        if !self.workspaces.iter().any(|workspace| workspace.name == self.active) {
            self.active = DEFAULT_WORKSPACE.into();
        }
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }

    /// Parses saved layouts after bounding input size; malformed data falls back to defaults.
    pub fn from_json(text: &str) -> Self {
        if text.len() as u64 > MAX_WORKSPACE_BYTES {
            log::warn!("workspace file exceeds the {MAX_WORKSPACE_BYTES}-byte limit; using defaults");
            return Self::default();
        }
        match serde_json::from_str::<Self>(text) {
            Ok(mut workspaces) => {
                workspaces.sanitize();
                workspaces
            }
            Err(error) => {
                log::warn!("ignoring unreadable workspace layout: {error}");
                Self::default()
            }
        }
    }

    pub fn load(path: &Path) -> Self {
        let result = std::fs::metadata(path).and_then(|metadata| {
            if metadata.len() > MAX_WORKSPACE_BYTES {
                return Err(std::io::Error::other("workspace file exceeds the size limit"));
            }
            std::fs::read_to_string(path)
        });
        match result {
            Ok(text) => Self::from_json(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                log::warn!("could not load workspaces from {}: {error}", path.display());
                Self::default()
            }
        }
    }

    /// Writes atomically so a crash never leaves a half-written layout.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json()?;
        if json.len() as u64 > MAX_WORKSPACE_BYTES {
            return Err(format!("workspace file exceeds the {MAX_WORKSPACE_BYTES}-byte limit"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, path).map_err(|error| error.to_string())
    }
}

fn is_builtin(name: &str) -> bool {
    [DEFAULT_WORKSPACE, QUEUE_WORKSPACE, PREVIEW_WORKSPACE].contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_layouts_validate_and_expose_their_expected_panels() {
        let workspace = WorkspaceSet::default();
        let current = workspace.current().expect("default workspace exists");
        assert!(current.dock.validate().is_ok());
        for panel in [PanelId::MediaBrowser, PanelId::Presets, PanelId::Queue, PanelId::WatchFolders, PanelId::Encoding] {
            assert!(current.is_visible(panel), "{panel:?} should be visible in the default workspace");
        }
        assert!(!current.is_visible(PanelId::Preview));
        let preview = workspace.workspaces.iter().find(|item| item.name == PREVIEW_WORKSPACE).expect("preview workspace exists");
        assert!(preview.dock.validate().is_ok());
        assert!(preview.is_visible(PanelId::Preview));
    }

    #[test]
    fn default_layout_matches_the_target_panel_arrangement_and_proportions() {
        fn split(node: &Node<PanelId>) -> Option<(SplitAxis, f32, &Node<PanelId>, &Node<PanelId>)> {
            match node {
                Node::Split { axis, size: SplitSize::Ratio(ratio), first, second } => Some((*axis, *ratio, first, second)),
                _ => None,
            }
        }

        fn only_panel(node: &Node<PanelId>, expected: PanelId) -> bool {
            matches!(node, Node::Tabs { panels, active: 0 } if panels.as_slice() == [expected])
        }

        let workspace = WorkspaceSet::default();
        let root = workspace.current().and_then(|current| current.dock.layout.root.as_ref()).expect("default layout has a root");
        let (axis, root_ratio, left, right) = split(root).expect("default layout root is split");
        assert_eq!(axis, SplitAxis::Horizontal);
        assert!((root_ratio - 0.44).abs() < f32::EPSILON);
        let (axis, left_ratio, media, presets) = split(left).expect("left column is split");
        assert_eq!(axis, SplitAxis::Vertical);
        assert!((left_ratio - 0.57).abs() < f32::EPSILON);
        assert!(only_panel(media, PanelId::MediaBrowser));
        assert!(only_panel(presets, PanelId::Presets));
        let (axis, queue_ratio, queue_watch, encoding) = split(right).expect("queue and encoding panels are split");
        assert_eq!(axis, SplitAxis::Vertical);
        assert!((queue_ratio - 0.67).abs() < f32::EPSILON);
        assert!(matches!(queue_watch, Node::Tabs { panels, active: 0 } if panels.as_slice() == [PanelId::Queue, PanelId::WatchFolders]));
        assert!(only_panel(encoding, PanelId::Encoding));
    }

    #[test]
    fn json_round_trip_preserves_user_docking_actions() {
        let mut set = WorkspaceSet::default();
        set.save_as("Mine").unwrap();
        let workspace = set.current_mut().unwrap();
        workspace.set_visible(PanelId::Preview, false).unwrap();
        workspace.set_visible(PanelId::Preview, true).unwrap();
        let back = WorkspaceSet::from_json(&set.to_json().unwrap());
        assert_eq!(back, set);
    }

    #[test]
    fn corrupt_or_hostile_layouts_recover_to_defaults() {
        assert_eq!(WorkspaceSet::from_json("{not json"), WorkspaceSet::default());
        let too_large = " ".repeat(MAX_WORKSPACE_BYTES as usize + 1);
        assert_eq!(WorkspaceSet::from_json(&too_large), WorkspaceSet::default());
        let mut set = WorkspaceSet::default();
        let workspace = set.current_mut().unwrap();
        workspace.dock.layout.root = Some(Node::Tabs { panels: vec![PanelId::Preview, PanelId::Preview], active: 0 });
        set.sanitize();
        assert!(set.current().unwrap().dock.validate().is_ok());
        assert!(set.current().unwrap().is_visible(PanelId::Queue));
    }

    #[test]
    fn builtins_are_protected_and_custom_workspaces_are_managed() {
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
    fn reset_restores_the_builtin_layout() {
        let mut set = WorkspaceSet::default();
        set.current_mut().unwrap().set_visible(PanelId::Queue, false).unwrap();
        set.reset_active();
        assert!(set.current().unwrap().is_visible(PanelId::Queue));
        assert!(!set.current().unwrap().is_visible(PanelId::Preview));
    }

    #[test]
    fn focus_cycles_through_visible_panels_and_skips_hidden_panels() {
        let mut workspace = default_layout(DEFAULT_WORKSPACE);
        workspace.set_visible(PanelId::Presets, false).unwrap();
        assert_eq!(workspace.focus_neighbor(PanelId::MediaBrowser, true).unwrap(), Some(PanelId::Queue));
        assert_eq!(workspace.focus_neighbor(PanelId::MediaBrowser, false).unwrap(), Some(PanelId::Encoding));
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
