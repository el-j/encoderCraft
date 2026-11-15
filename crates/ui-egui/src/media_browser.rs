//! Media Browser panel: directory tree sidebar, flat-column table view, and bottom zoom/status toolbar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{RichText, Ui, Vec2};

use crate::icons::{self, Icon};
use crate::media_model::{BoundedCache, BrowserModel, Entry, EntryKind, SortBy, THUMB_CACHE_CAPACITY, standard_locations};
use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::theme::Tokens;

const MAX_THUMB_DECODES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserView {
    #[default]
    List,
    Thumbnails,
}

/// Collapsible tree expansion states for Media Browser sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaBrowserTreeState {
    pub favorites_expanded: bool,
    pub user_folders_expanded: bool,
    pub drives_expanded: bool,
}

impl Default for MediaBrowserTreeState {
    fn default() -> Self {
        Self { favorites_expanded: true, user_folders_expanded: true, drives_expanded: true }
    }
}

pub const MEDIA_COLUMNS: [(&str, &str); 6] =
    [("name", "Name"), ("kind", "Type"), ("size", "Size"), ("modified", "Date Modified"), ("created", "Date Created"), ("format", "Format")];

#[derive(Debug, Clone, PartialEq)]
pub struct MediaBrowserColumnSettings {
    pub name: f32,
    pub kind: f32,
    pub size: f32,
    pub modified: f32,
    pub created: f32,
    pub format: f32,
    pub visible: Vec<String>,
}

impl Default for MediaBrowserColumnSettings {
    fn default() -> Self {
        Self {
            name: 240.0,
            kind: 80.0,
            size: 80.0,
            modified: 130.0,
            created: 130.0,
            format: 80.0,
            visible: vec!["name".into(), "kind".into(), "size".into(), "modified".into()],
        }
    }
}

/// Fixed column widths for pixel-aligned table layout in List mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaBrowserColumnWidths {
    pub name: f32,
    pub kind: f32,
    pub size: f32,
    pub date: f32,
    pub created: f32,
    pub format: f32,
}

impl MediaBrowserColumnWidths {
    pub fn compute(total_width: f32) -> Self {
        let kind = 70.0;
        let size = 65.0;
        let date = 115.0;
        let padding = 24.0;
        let name = (total_width - kind - size - date - padding).max(120.0);
        Self { name, kind, size, date, created: 0.0, format: 0.0 }
    }

    pub fn compute_with_settings(total_width: f32, settings: &MediaBrowserColumnSettings) -> Self {
        let is_vis = |id: &str| settings.visible.iter().any(|v| v == id);
        let kind = if is_vis("kind") { settings.kind } else { 0.0 };
        let size = if is_vis("size") { settings.size } else { 0.0 };
        let date = if is_vis("modified") { settings.modified } else { 0.0 };
        let created = if is_vis("created") { settings.created } else { 0.0 };
        let format = if is_vis("format") { settings.format } else { 0.0 };
        let fixed = kind + size + date + created + format;
        let name = (total_width - fixed - 24.0).max(120.0);
        Self { name, kind, size, date, created, format }
    }
}

pub struct MediaBrowserState {
    pub model: BrowserModel,
    pub view: BrowserView,
    pub thumb_size: f32,
    pub selected: Option<PathBuf>,
    pub selected_paths: std::collections::HashSet<PathBuf>,
    pub last_clicked_index: Option<usize>,
    pub inspect_properties: Option<PathBuf>,
    pub tree_state: MediaBrowserTreeState,
    pub columns: MediaBrowserColumnSettings,
    pub rename_target: Option<(PathBuf, String)>,
    pub creating_folder: bool,
    pub new_folder_input: String,
    locations: Vec<(String, PathBuf)>,
    path_input: String,
    thumbs: BoundedCache<egui::TextureHandle>,
    failed: BoundedCache<()>,
    decoding: HashMap<PathBuf, PreviewDecoder>,
}

impl Default for MediaBrowserState {
    fn default() -> Self {
        let locations = standard_locations();
        let start = locations.first().map(|(_, p)| p.clone()).unwrap_or_else(|| PathBuf::from("."));
        Self::at(start, locations)
    }
}

impl MediaBrowserState {
    pub fn at(start: PathBuf, locations: Vec<(String, PathBuf)>) -> Self {
        Self {
            path_input: start.display().to_string(),
            model: BrowserModel::new(start),
            view: BrowserView::List,
            thumb_size: 96.0,
            selected: None,
            selected_paths: std::collections::HashSet::new(),
            last_clicked_index: None,
            inspect_properties: None,
            tree_state: MediaBrowserTreeState::default(),
            columns: MediaBrowserColumnSettings::default(),
            rename_target: None,
            creating_folder: false,
            new_folder_input: "New Folder".to_string(),
            locations,
            thumbs: BoundedCache::new(THUMB_CACHE_CAPACITY),
            failed: BoundedCache::new(THUMB_CACHE_CAPACITY),
            decoding: HashMap::new(),
        }
    }

    fn go(&mut self, path: PathBuf) {
        self.model.navigate(path);
        self.path_input = self.model.location().display().to_string();
        self.selected = None;
        self.selected_paths.clear();
        self.last_clicked_index = None;
    }

    pub fn select_single(&mut self, path: PathBuf, index: usize) {
        self.selected_paths.clear();
        self.selected_paths.insert(path.clone());
        self.selected = Some(path);
        self.last_clicked_index = Some(index);
    }

    pub fn toggle_select(&mut self, path: PathBuf, index: usize) {
        if self.selected_paths.contains(&path) {
            self.selected_paths.remove(&path);
            if self.selected.as_ref() == Some(&path) {
                self.selected = self.selected_paths.iter().next().cloned();
            }
        } else {
            self.selected_paths.insert(path.clone());
            self.selected = Some(path);
        }
        self.last_clicked_index = Some(index);
    }

    pub fn range_select(&mut self, entries: &[Entry], target_index: usize) {
        let start = self.last_clicked_index.unwrap_or(0).min(target_index);
        let end = self.last_clicked_index.unwrap_or(0).max(target_index);
        for i in start..=end {
            if let Some(e) = entries.get(i) {
                self.selected_paths.insert(e.path.clone());
            }
        }
        if let Some(e) = entries.get(target_index) {
            self.selected = Some(e.path.clone());
        }
        self.last_clicked_index = Some(target_index);
    }

    pub fn select_all(&mut self, entries: &[Entry]) {
        self.selected_paths.clear();
        for e in entries {
            self.selected_paths.insert(e.path.clone());
        }
        self.selected = self.selected_paths.iter().next().cloned();
    }

    /// Advances thumbnail decoding. Returns true while work is outstanding.
    fn pump_thumbs(&mut self, ctx: &egui::Context) -> bool {
        let mut done = Vec::new();
        for (path, dec) in &self.decoding {
            while let Some(ev) = dec.poll() {
                match ev {
                    PreviewEvent::Opened { frame_count, .. } => {
                        if frame_count == 0 {
                            self.failed.insert(path.clone(), ());
                            done.push(path.clone());
                        } else {
                            dec.request_frame(0);
                        }
                    }
                    PreviewEvent::Frame { width, height, rgba, .. } => {
                        if width > 0 && height > 0 {
                            let img = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                            let tex = ctx.load_texture(format!("thumb-{}", path.display()), img, egui::TextureOptions::LINEAR);
                            self.thumbs.insert(path.clone(), tex);
                        } else {
                            self.failed.insert(path.clone(), ());
                        }
                        done.push(path.clone());
                    }
                    PreviewEvent::Failed(_) => {
                        self.failed.insert(path.clone(), ());
                        done.push(path.clone());
                    }
                }
            }
        }
        for p in done {
            self.decoding.remove(&p);
        }
        !self.decoding.is_empty()
    }

    fn want_thumb(&mut self, entry: &Entry) {
        if matches!(entry.kind, EntryKind::Video | EntryKind::Image)
            && !self.thumbs.contains(&entry.path)
            && !self.failed.contains(&entry.path)
            && !self.decoding.contains_key(&entry.path)
            && self.decoding.len() < MAX_THUMB_DECODES
        {
            self.decoding.insert(entry.path.clone(), PreviewDecoder::open(entry.path.clone()));
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS.get(unit).copied().unwrap_or("TB"))
}

pub fn format_age(modified: Option<SystemTime>, _now: SystemTime) -> String {
    let Some(m) = modified else { return "-".into() };
    let dt: chrono::DateTime<chrono::Local> = m.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn is_drive_or_volume(label: &str, path: &Path) -> bool {
    label.starts_with("Drive ")
        || path.starts_with("/Volumes")
        || path.starts_with("/media")
        || path.starts_with("/mnt")
        || path == Path::new("/")
        || path.parent().is_none()
}

fn collapsible_tree_header(ui: &mut Ui, label: &str, expanded: &mut bool, tokens: &Tokens) {
    let icon = if *expanded { Icon::ChevronDown } else { Icon::ChevronRight };
    let resp = ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::click());
        icons::paint(ui.painter(), rect, icon, tokens.text_dim);
        let text_resp = ui.add(egui::Label::new(RichText::new(label).small().strong().color(tokens.text_dim)).sense(egui::Sense::click()));
        resp.union(text_resp)
    });
    if resp.inner.clicked() {
        *expanded = !*expanded;
    }
}

fn tree_item(ui: &mut Ui, label: &str, path: &Path, is_active: bool, tokens: &Tokens) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::hover());
        icons::paint(ui.painter(), rect, Icon::Folder, if is_active { tokens.accent_primary } else { tokens.text_dim });
        let text_color = if is_active { tokens.accent_primary } else { tokens.text_primary };
        let resp = ui.add(egui::Label::new(RichText::new(label).small().color(text_color)).sense(egui::Sense::click()));
        if resp.on_hover_text(path.display().to_string()).clicked() {
            clicked = true;
        }
    });
    clicked
}

fn render_sidebar(ui: &mut Ui, state: &mut MediaBrowserState, tokens: &Tokens) {
    ui.push_id("media_browser_sidebar_inner", |ui| {
        egui::ScrollArea::vertical().id_salt("media_browser_sidebar_scroll").auto_shrink([false, false]).show(ui, |ui| {
            // 1. Favorites Group
            collapsible_tree_header(ui, "Favorites", &mut state.tree_state.favorites_expanded, tokens);
            if state.tree_state.favorites_expanded {
                let favs = state.model.favorites.clone();
                if favs.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new("None yet").small().color(tokens.text_dim));
                    });
                } else {
                    for f in favs {
                        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
                        let is_active = state.model.location() == f;
                        let mut navigate_to = None;
                        let mut remove_fav = false;
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::hover());
                            icons::paint(ui.painter(), rect, Icon::Folder, if is_active { tokens.accent_primary } else { tokens.text_dim });
                            let resp = ui
                                .add(
                                    egui::Label::new(RichText::new(&name).small().color(if is_active {
                                        tokens.accent_primary
                                    } else {
                                        tokens.text_primary
                                    }))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text(f.display().to_string());
                            if resp.clicked() {
                                navigate_to = Some(f.clone());
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Remove from Favorites").clicked() {
                                    remove_fav = true;
                                    ui.close();
                                }
                            });
                        });
                        if remove_fav {
                            state.model.favorites.retain(|p| p != &f);
                        }
                        if let Some(target) = navigate_to {
                            state.go(target);
                        }
                    }
                }
            }

            ui.add_space(6.0);

            // 2. Local Storage / User Folders Group
            collapsible_tree_header(ui, "Local Storage", &mut state.tree_state.user_folders_expanded, tokens);
            if state.tree_state.user_folders_expanded {
                let mut navigate_to = None;
                for (label, p) in state.locations.clone() {
                    if !is_drive_or_volume(&label, &p) {
                        let is_active = state.model.location() == p;
                        if tree_item(ui, &label, &p, is_active, tokens) {
                            navigate_to = Some(p);
                        }
                    }
                }
                if let Some(target) = navigate_to {
                    state.go(target);
                }
            }

            ui.add_space(6.0);

            // 3. Drives & Volumes Group
            collapsible_tree_header(ui, "Drives & Volumes", &mut state.tree_state.drives_expanded, tokens);
            if state.tree_state.drives_expanded {
                let mut navigate_to = None;
                for (label, p) in state.locations.clone() {
                    if is_drive_or_volume(&label, &p) {
                        let is_active = state.model.location() == p;
                        if tree_item(ui, &label, &p, is_active, tokens) {
                            navigate_to = Some(p);
                        }
                    }
                }
                if let Some(target) = navigate_to {
                    state.go(target);
                }
            }
        });
    });
}

/// Renders the browser. Returns media files the user asked to add to the queue.
pub fn render_media_browser(
    ui: &mut Ui,
    state: &mut MediaBrowserState,
    tokens: &Tokens,
    commands: &encodercraft_engine::cmd::CommandRegistry,
    queue_mgr: &std::sync::Arc<encodercraft_engine::QueueManager>,
) -> Vec<PathBuf> {
    let mut to_add = Vec::new();
    if state.model.poll() {
        ui.ctx().request_repaint();
    }
    if state.model.loading || state.pump_thumbs(ui.ctx()) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }

    // 1. Top Navigation & Filter Bar
    ui.horizontal(|ui| {
        ui.add_enabled_ui(state.model.can_back(), |ui| {
            if ui.button("<").on_hover_text("Back").clicked() {
                state.model.back();
                state.path_input = state.model.location().display().to_string();
            }
        });
        ui.add_enabled_ui(state.model.can_forward(), |ui| {
            if ui.button(">").on_hover_text("Forward").clicked() {
                state.model.forward();
                state.path_input = state.model.location().display().to_string();
            }
        });
        if ui.button("^").on_hover_text("Parent folder").clicked() {
            state.model.up();
            state.path_input = state.model.location().display().to_string();
        }

        let loc = ui.add(egui::TextEdit::singleline(&mut state.path_input).desired_width(180.0).hint_text("Location"));
        if loc.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let p = PathBuf::from(state.path_input.trim());
            state.go(p);
        }
        if icons::small_button(ui, Icon::Reset, tokens, "Refresh").clicked() {
            state.model.refresh();
        }

        ui.separator();

        ui.add(egui::TextEdit::singleline(&mut state.model.search).hint_text("Search").desired_width(110.0));
        egui::ComboBox::from_id_salt("media-kind").selected_text(state.model.kind_filter.map(EntryKind::label).unwrap_or("All types")).show_ui(
            ui,
            |ui| {
                ui.selectable_value(&mut state.model.kind_filter, None, "All types");
                for k in [EntryKind::Video, EntryKind::Audio, EntryKind::Image, EntryKind::Spatial3D, EntryKind::Vector, EntryKind::Document] {
                    ui.selectable_value(&mut state.model.kind_filter, Some(k), k.label());
                }
            },
        );
        ui.checkbox(&mut state.model.show_hidden, "Hidden");

        if ui.button("+ New Folder").on_hover_text("Create a new folder in this directory").clicked() {
            state.creating_folder = true;
            state.new_folder_input = "New Folder".to_string();
        }
    });

    if state.creating_folder {
        egui::Window::new("Create New Folder").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ui.ctx(),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Folder Name:");
                    ui.text_edit_singleline(&mut state.new_folder_input);
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let name = state.new_folder_input.trim();
                        if !name.is_empty() {
                            let new_dir = state.model.location().join(name);
                            match std::fs::create_dir(&new_dir) {
                                Ok(()) => {
                                    state.creating_folder = false;
                                    state.model.refresh();
                                }
                                Err(e) => {
                                    state.model.error = Some(format!("Failed to create folder: {e}"));
                                    state.creating_folder = false;
                                }
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        state.creating_folder = false;
                    }
                });
            },
        );
    }

    if let Some((target_path, mut target_name)) = state.rename_target.clone() {
        let mut close = false;
        let mut commit = false;
        egui::Window::new("Rename Item").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("New Name:");
                ui.text_edit_singleline(&mut target_name);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Rename").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        if commit {
            let trimmed = target_name.trim();
            if !trimmed.is_empty() {
                let new_path = target_path.with_file_name(trimmed);
                if new_path != target_path {
                    match std::fs::rename(&target_path, &new_path) {
                        Ok(()) => {
                            state.rename_target = None;
                            state.model.refresh();
                        }
                        Err(e) => {
                            state.model.error = Some(format!("Rename failed: {e}"));
                            state.rename_target = None;
                        }
                    }
                } else {
                    state.rename_target = None;
                }
            }
        } else if close {
            state.rename_target = None;
        } else {
            state.rename_target = Some((target_path, target_name));
        }
    }

    ui.separator();

    let entries: Vec<Entry> = state.model.visible().into_iter().cloned().collect();

    // 2. Middle Content Area (Sidebar + File List/Grid)
    let footer_h = 28.0;
    let available_h = (ui.available_height() - footer_h).max(60.0);

    ui.horizontal(|ui| {
        // Left sidebar
        let sidebar_w = 160.0;
        ui.push_id("media_sidebar_wrap", |ui| {
            ui.allocate_ui_with_layout(Vec2::new(sidebar_w, available_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                render_sidebar(ui, state, tokens);
            });
        });

        ui.separator();

        // Right content pane
        ui.push_id("media_content_wrap", |ui| {
            ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), available_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                if let Some(err) = &state.model.error {
                    ui.label(RichText::new(err).color(tokens.status_failed));
                }
                if state.model.truncated() {
                    ui.label(RichText::new("Showing the first 20000 items of this folder").small().color(tokens.text_dim));
                }

                match state.view {
                    BrowserView::List => list_view(ui, state, &entries, tokens, &mut to_add, commands, queue_mgr),
                    BrowserView::Thumbnails => thumbnail_view(ui, state, &entries, tokens, &mut to_add, commands, queue_mgr),
                }
            });
        });
    });

    // 3. Bottom Status & View Toolbar
    ui.separator();
    ui.horizontal(|ui| {
        let count_text = if entries.is_empty() {
            "0 items".to_string()
        } else if !state.selected_paths.is_empty() {
            format!("{} item{} ({} selected)", entries.len(), if entries.len() == 1 { "" } else { "s" }, state.selected_paths.len())
        } else if state.selected.is_some() {
            format!("{} item{} (1 selected)", entries.len(), if entries.len() == 1 { "" } else { "s" })
        } else {
            format!("{} item{}", entries.len(), if entries.len() == 1 { "" } else { "s" })
        };
        ui.label(RichText::new(count_text).small().color(tokens.text_dim));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.view == BrowserView::Thumbnails {
                ui.add(egui::Slider::new(&mut state.thumb_size, 48.0..=220.0).show_value(false));
                ui.label(RichText::new("Zoom").small().color(tokens.text_dim));
            }

            ui.separator();

            let grid_active = state.view == BrowserView::Thumbnails;
            if icons::button(ui, Icon::GridView, 14.0, grid_active, tokens, "Thumbnail View").clicked() {
                state.view = BrowserView::Thumbnails;
            }

            let list_active = state.view == BrowserView::List;
            if icons::button(ui, Icon::ListView, 14.0, list_active, tokens, "List View").clicked() {
                state.view = BrowserView::List;
            }
        });
    });

    to_add
}

fn entry_menu(
    ui: &mut Ui,
    state: &mut MediaBrowserState,
    e: &Entry,
    to_add: &mut Vec<PathBuf>,
    commands: &encodercraft_engine::cmd::CommandRegistry,
    queue_mgr: &std::sync::Arc<encodercraft_engine::QueueManager>,
) {
    for info in crate::commands::context_menu_commands(crate::commands::CommandContextMenu::MediaItem) {
        let enabled = match info.command {
            crate::commands::AppCommand::AddMediaItem => true,
            crate::commands::AppCommand::SourceProperties => e.kind.is_media(),
            crate::commands::AppCommand::OpenMediaFolder | crate::commands::AppCommand::ToggleMediaFavorite => e.kind == EntryKind::Folder,
            _ => false,
        };
        if ui.add_enabled(enabled, egui::Button::new(info.context_label(crate::commands::CommandContextMenu::MediaItem))).clicked() {
            match info.command {
                crate::commands::AppCommand::AddMediaItem => {
                    if state.selected_paths.is_empty() || !state.selected_paths.contains(&e.path) {
                        to_add.push(e.path.clone());
                    } else {
                        for p in &state.selected_paths {
                            to_add.push(p.clone());
                        }
                    }
                }
                crate::commands::AppCommand::SourceProperties => state.inspect_properties = Some(e.path.clone()),
                crate::commands::AppCommand::OpenMediaFolder => {
                    match commands.dispatch(queue_mgr, "media.open_folder", serde_json::json!({ "path": e.path.clone() })) {
                        Ok(value) => match value.get("path").and_then(serde_json::Value::as_str) {
                            Some(path) => state.go(PathBuf::from(path)),
                            None => state.model.error = Some("Open Folder returned no folder path.".into()),
                        },
                        Err(error) => state.model.error = Some(format!("Open Folder failed: {error}")),
                    }
                }
                crate::commands::AppCommand::ToggleMediaFavorite => {
                    match commands.dispatch(queue_mgr, "media.toggle_favorite", serde_json::json!({ "path": e.path.clone() })) {
                        Ok(value) => match serde_json::from_value(value) {
                            Ok(favorites) => state.model.favorites = favorites,
                            Err(error) => state.model.error = Some(format!("Favorite Folder returned invalid data: {error}")),
                        },
                        Err(error) => state.model.error = Some(format!("Favorite Folder failed: {error}")),
                    }
                }
                _ => {}
            }
            ui.close();
        }
    }

    ui.separator();
    if ui.button("Rename... (F2)").clicked() {
        state.rename_target = Some((e.path.clone(), e.name.clone()));
        ui.close();
    }
    if let Some(parent) = state.model.location().parent() {
        let dest = parent.join(&e.name);
        if ui.button("Move to Parent Folder").clicked() {
            match std::fs::rename(&e.path, &dest) {
                Ok(()) => state.model.refresh(),
                Err(err) => state.model.error = Some(format!("Move failed: {err}")),
            }
            ui.close();
        }
    }
    if ui.button("New Folder...").clicked() {
        state.creating_folder = true;
        state.new_folder_input = "New Folder".to_string();
        ui.close();
    }
}

fn activate(state: &mut MediaBrowserState, e: &Entry, to_add: &mut Vec<PathBuf>) {
    if e.kind == EntryKind::Folder {
        state.go(e.path.clone());
    } else if e.kind.is_media() {
        to_add.push(e.path.clone());
    }
}

fn media_column_picker(ui: &mut Ui, settings: &mut MediaBrowserColumnSettings) {
    ui.label(RichText::new("Media Browser Columns").strong());
    ui.separator();
    for (id, label) in MEDIA_COLUMNS {
        let is_name = id == "name";
        let mut shown = settings.visible.iter().any(|v| v == id);
        let resp = ui.add_enabled(!is_name, egui::Checkbox::new(&mut shown, label));
        if resp.changed() {
            if shown {
                if !settings.visible.iter().any(|v| v == id) {
                    settings.visible.push(id.to_string());
                }
            } else if !is_name {
                settings.visible.retain(|v| v != id);
            }
        }
    }
    ui.separator();
    if ui.button("Reset to Default").clicked() {
        *settings = MediaBrowserColumnSettings::default();
        ui.close();
    }
}

fn media_column_header(ui: &mut Ui, label: &str, sort_by: SortBy, model: &mut BrowserModel, width: &mut f32, tokens: &Tokens) {
    let current_sort = model.sort;
    let is_desc = model.descending;
    let arrow = if current_sort == sort_by { if is_desc { " \u{25be}" } else { " \u{25b4}" } } else { "" };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(*width, 20.0), egui::Sense::click());
    let mut header_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    header_ui.set_clip_rect(rect);
    header_ui
        .add(egui::Label::new(RichText::new(format!("{label}{arrow}")).small().strong().color(tokens.text_primary)).selectable(false).truncate());
    if resp.clicked() {
        model.descending = model.sort == sort_by && !model.descending;
        model.sort = sort_by;
    }
    // Resize divider handle on right edge
    let handle_rect = egui::Rect::from_min_max(egui::pos2(rect.max.x - 3.0, rect.min.y), egui::pos2(rect.max.x + 3.0, rect.max.y));
    let handle_resp = ui.allocate_rect(handle_rect, egui::Sense::drag());
    if handle_resp.dragged() {
        *width = (*width + handle_resp.drag_delta().x).clamp(40.0, 600.0);
    }
    if handle_resp.hovered() || handle_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
    }
}

fn list_view(
    ui: &mut Ui,
    state: &mut MediaBrowserState,
    entries: &[Entry],
    tokens: &Tokens,
    to_add: &mut Vec<PathBuf>,
    commands: &encodercraft_engine::cmd::CommandRegistry,
    queue_mgr: &std::sync::Arc<encodercraft_engine::QueueManager>,
) {
    let now = SystemTime::now();
    let total_w = ui.available_width();
    let cols = MediaBrowserColumnWidths::compute_with_settings(total_w, &state.columns);

    // Header bar with resizable column dividers and right-click column show/hide
    let header_resp = ui.horizontal(|ui| {
        media_column_header(ui, "Name", SortBy::Name, &mut state.model, &mut state.columns.name, tokens);
        if state.columns.visible.iter().any(|v| v == "kind") {
            media_column_header(ui, "Type", SortBy::Kind, &mut state.model, &mut state.columns.kind, tokens);
        }
        if state.columns.visible.iter().any(|v| v == "size") {
            media_column_header(ui, "Size", SortBy::Size, &mut state.model, &mut state.columns.size, tokens);
        }
        if state.columns.visible.iter().any(|v| v == "modified") {
            media_column_header(ui, "Modified", SortBy::Modified, &mut state.model, &mut state.columns.modified, tokens);
        }
        if state.columns.visible.iter().any(|v| v == "created") {
            media_column_header(ui, "Created", SortBy::Created, &mut state.model, &mut state.columns.created, tokens);
        }
        if state.columns.visible.iter().any(|v| v == "format") {
            media_column_header(ui, "Format", SortBy::Extension, &mut state.model, &mut state.columns.format, tokens);
        }
    });
    header_resp.response.interact(egui::Sense::click()).context_menu(|ui| {
        media_column_picker(ui, &mut state.columns);
    });

    // Cmd/Ctrl+A shortcut to select all files
    if ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::A)) {
        state.select_all(entries);
    }
    // F2 shortcut to rename selected item
    if ui.input(|i| i.key_pressed(egui::Key::F2))
        && let Some(ref sel_path) = state.selected
        && let Some(sel_entry) = entries.iter().find(|e| &e.path == sel_path)
    {
        state.rename_target = Some((sel_entry.path.clone(), sel_entry.name.clone()));
    }

    let row_h = 20.0;
    egui::ScrollArea::vertical().id_salt("media_browser_list_scroll").auto_shrink([false, false]).show_rows(ui, row_h, entries.len(), |ui, range| {
        for (idx, e) in entries.iter().enumerate().skip(range.start).take(range.len()) {
            ui.push_id(&e.path, |ui| {
                let selected = state.selected_paths.contains(&e.path) || state.selected.as_ref() == Some(&e.path);
                let row_start_y = ui.cursor().min.y;

                ui.horizontal(|ui| {
                    if selected {
                        let hl_rect = egui::Rect::from_min_size(egui::pos2(ui.cursor().min.x, row_start_y), Vec2::new(total_w, row_h));
                        ui.painter().rect_filled(hl_rect, 2.0, tokens.accent_primary.gamma_multiply(0.20));
                    }

                    // Icon + Name (strictly left-aligned)
                    let (name_rect, resp) = ui.allocate_exact_size(Vec2::new(cols.name, 18.0), egui::Sense::click());
                    let mut name_ui =
                        ui.new_child(egui::UiBuilder::new().max_rect(name_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    name_ui.set_clip_rect(name_rect);

                    let icon = match e.kind {
                        EntryKind::Folder => Icon::Folder,
                        EntryKind::Audio => Icon::AudioWaveform,
                        EntryKind::Video => Icon::Film,
                        EntryKind::Image => Icon::Image,
                        _ => Icon::Preset,
                    };
                    let (icon_rect, _) = name_ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                    icons::paint(
                        name_ui.painter(),
                        icon_rect,
                        icon,
                        if e.kind == EntryKind::Folder { tokens.text_dim } else { tokens.accent_primary },
                    );
                    name_ui.add_space(4.0);

                    let name_label = RichText::new(&e.name).small().color(tokens.text_primary);
                    name_ui.add(egui::Label::new(name_label).selectable(false).truncate());

                    // Type
                    if cols.kind > 0.0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(cols.kind, 18.0), egui::Sense::hover());
                        let mut cell_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        cell_ui.set_clip_rect(rect);
                        cell_ui.add(egui::Label::new(RichText::new(e.kind.label()).small().color(tokens.text_dim)).truncate());
                    }

                    // Size
                    if cols.size > 0.0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(cols.size, 18.0), egui::Sense::hover());
                        let mut cell_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        cell_ui.set_clip_rect(rect);
                        let size_str = if e.kind == EntryKind::Folder { "--".to_string() } else { format_size(e.size) };
                        cell_ui.add(egui::Label::new(RichText::new(size_str).small().color(tokens.text_dim)).truncate());
                    }

                    // Modified
                    if cols.date > 0.0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(cols.date, 18.0), egui::Sense::hover());
                        let mut cell_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        cell_ui.set_clip_rect(rect);
                        let date_str = format_age(e.modified, now);
                        cell_ui.add(egui::Label::new(RichText::new(date_str).small().color(tokens.text_dim)).truncate());
                    }

                    // Created
                    if cols.created > 0.0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(cols.created, 18.0), egui::Sense::hover());
                        let mut cell_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        cell_ui.set_clip_rect(rect);
                        let date_str = format_age(e.created, now);
                        cell_ui.add(egui::Label::new(RichText::new(date_str).small().color(tokens.text_dim)).truncate());
                    }

                    // Format
                    if cols.format > 0.0 {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(cols.format, 18.0), egui::Sense::hover());
                        let mut cell_ui =
                            ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        cell_ui.set_clip_rect(rect);
                        cell_ui.add(egui::Label::new(RichText::new(&e.extension).small().color(tokens.text_dim)).truncate());
                    }

                    if resp.clicked() {
                        let cmd_held = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                        let shift_held = ui.input(|i| i.modifiers.shift);
                        if shift_held {
                            state.range_select(entries, idx);
                        } else if cmd_held {
                            state.toggle_select(e.path.clone(), idx);
                        } else {
                            state.select_single(e.path.clone(), idx);
                        }
                    }
                    if resp.double_clicked() {
                        if state.selected_paths.len() > 1 && state.selected_paths.contains(&e.path) {
                            let selected: Vec<_> = state.selected_paths.iter().cloned().collect();
                            for p in &selected {
                                if let Some(entry) = entries.iter().find(|it| &it.path == p) {
                                    activate(state, entry, to_add);
                                }
                            }
                        } else {
                            activate(state, e, to_add);
                        }
                    }
                    resp.context_menu(|ui| entry_menu(ui, state, e, to_add, commands, queue_mgr));
                });
            });
        }
    });

    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ui.ctx().egui_wants_keyboard_input() {
        if state.selected_paths.len() > 1 {
            let selected: Vec<_> = state.selected_paths.iter().cloned().collect();
            for p in &selected {
                if let Some(entry) = entries.iter().find(|it| &it.path == p) {
                    activate(state, entry, to_add);
                }
            }
        } else if let Some(sel) = entries.iter().find(|e| state.selected.as_ref() == Some(&e.path)) {
            activate(state, sel, to_add);
        }
    }
}

fn thumbnail_view(
    ui: &mut Ui,
    state: &mut MediaBrowserState,
    entries: &[Entry],
    tokens: &Tokens,
    to_add: &mut Vec<PathBuf>,
    commands: &encodercraft_engine::cmd::CommandRegistry,
    queue_mgr: &std::sync::Arc<encodercraft_engine::QueueManager>,
) {
    let cell = Vec2::new(state.thumb_size, state.thumb_size * 0.65 + 28.0);
    let cols = ((ui.available_width() / (cell.x + 8.0)).floor() as usize).max(1);
    let rows = entries.len().div_ceil(cols);
    egui::ScrollArea::vertical().id_salt("media_browser_thumb_scroll").auto_shrink([false, false]).show_rows(ui, cell.y + 8.0, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for (col_idx, e) in entries.iter().skip(row * cols).take(cols).enumerate() {
                    let entry_idx = row * cols + col_idx;
                    ui.push_id(&e.path, |ui| {
                        state.want_thumb(e);
                        let selected = state.selected_paths.contains(&e.path) || state.selected.as_ref() == Some(&e.path);
                        let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                        let painter = ui.painter_at(rect);
                        if selected {
                            painter.rect_filled(rect, 3.0, tokens.accent_primary.gamma_multiply(0.25));
                        }
                        let img_rect = egui::Rect::from_min_size(rect.min, Vec2::new(cell.x, cell.y - 28.0));
                        painter.rect_filled(img_rect.shrink(2.0), 3.0, tokens.bg_panel_darker);

                        if e.kind == EntryKind::Folder {
                            // Folders shown as folder cards
                            let icon_size = Vec2::new(32.0, 32.0);
                            let icon_rect = egui::Rect::from_center_size(img_rect.center() - Vec2::new(0.0, 4.0), icon_size);
                            icons::paint(&painter, icon_rect, Icon::Folder, tokens.text_dim);
                            painter.text(
                                egui::pos2(img_rect.center().x, img_rect.max.y - 10.0),
                                egui::Align2::CENTER_CENTER,
                                "Folder",
                                egui::FontId::proportional(10.0),
                                tokens.text_dim,
                            );
                        } else if e.kind == EntryKind::Audio {
                            // Audio files shown with audio symbol and format
                            let icon_size = Vec2::new(28.0, 28.0);
                            let icon_rect = egui::Rect::from_center_size(img_rect.center() - Vec2::new(0.0, 6.0), icon_size);
                            icons::paint(&painter, icon_rect, Icon::AudioWaveform, tokens.accent_primary);
                            let fmt_text = if !e.extension.is_empty() { &e.extension } else { "AUDIO" };
                            painter.text(
                                egui::pos2(img_rect.center().x, img_rect.max.y - 10.0),
                                egui::Align2::CENTER_CENTER,
                                fmt_text,
                                egui::FontId::proportional(10.0),
                                tokens.text_dim,
                            );
                        } else if let Some(tex) = state.thumbs.get(&e.path) {
                            // Videos and images shown with preview
                            let size = tex.size_vec2();
                            let fitted = crate::monitor_geom::fit_rect(img_rect.shrink(2.0), size.x / size.y.max(1.0));
                            painter.image(
                                tex.id(),
                                fitted,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            let icon = match e.kind {
                                EntryKind::Video => Icon::Film,
                                EntryKind::Image => Icon::Image,
                                _ => Icon::Preset,
                            };
                            let icon_size = Vec2::new(26.0, 26.0);
                            let icon_rect = egui::Rect::from_center_size(img_rect.center() - Vec2::new(0.0, 6.0), icon_size);
                            icons::paint(&painter, icon_rect, icon, tokens.text_dim);
                            painter.text(
                                egui::pos2(img_rect.center().x, img_rect.max.y - 10.0),
                                egui::Align2::CENTER_CENTER,
                                e.kind.label(),
                                egui::FontId::proportional(10.0),
                                tokens.text_dim,
                            );
                        }

                        // Little "format" name in right corner in thumbnail view
                        if e.kind != EntryKind::Folder && !e.extension.is_empty() {
                            let badge_text = &e.extension;
                            let font = egui::FontId::proportional(9.0);
                            let text_galley = painter.layout_no_wrap(badge_text.to_string(), font.clone(), egui::Color32::WHITE);
                            let badge_pad = Vec2::new(3.0, 1.0);
                            let badge_size = text_galley.size() + badge_pad * 2.0;
                            let badge_min = egui::pos2(img_rect.max.x - badge_size.x - 4.0, img_rect.max.y - badge_size.y - 4.0);
                            let badge_rect = egui::Rect::from_min_size(badge_min, badge_size);
                            painter.rect_filled(badge_rect, 2.0, egui::Color32::from_black_alpha(200));
                            painter.text(badge_rect.center(), egui::Align2::CENTER_CENTER, badge_text, font, egui::Color32::WHITE);
                        }

                        let mut name = e.name.clone();
                        if name.chars().count() > 18 {
                            name = format!("{}...", name.chars().take(15).collect::<String>());
                        }
                        painter.text(
                            egui::Pos2::new(rect.center().x, rect.max.y - 14.0),
                            egui::Align2::CENTER_CENTER,
                            name,
                            egui::FontId::proportional(11.0),
                            tokens.text_primary,
                        );
                        if resp.clicked() {
                            let cmd_held = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                            let shift_held = ui.input(|i| i.modifiers.shift);
                            if shift_held {
                                state.range_select(entries, entry_idx);
                            } else if cmd_held {
                                state.toggle_select(e.path.clone(), entry_idx);
                            } else {
                                state.select_single(e.path.clone(), entry_idx);
                            }
                        }
                        if resp.double_clicked() {
                            if state.selected_paths.len() > 1 && state.selected_paths.contains(&e.path) {
                                let selected: Vec<_> = state.selected_paths.iter().cloned().collect();
                                for p in &selected {
                                    if let Some(entry) = entries.iter().find(|it| &it.path == p) {
                                        activate(state, entry, to_add);
                                    }
                                }
                            } else {
                                activate(state, e, to_add);
                            }
                        }
                        resp.on_hover_text(&e.name).context_menu(|ui| entry_menu(ui, state, e, to_add, commands, queue_mgr));
                    });
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn size_and_age_formatting() {
        assert_eq!(format_size(10), "10 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        let now = SystemTime::now();
        assert_eq!(format_age(None, now), "-");
        let formatted = format_age(Some(now), now);
        assert_eq!(formatted.len(), 16);
        assert!(formatted.contains('-'));
        assert!(formatted.contains(':'));
    }

    #[test]
    fn panel_lists_files_and_adds_via_context_logic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mov"), b"x").unwrap();
        let mut state = MediaBrowserState::at(dir.path().to_path_buf(), Vec::new());
        let tokens = Tokens::dark();
        let queue_mgr = std::sync::Arc::new(encodercraft_engine::QueueManager::new_in_memory());
        let commands = encodercraft_engine::cmd::CommandRegistry::new();
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while state.model.visible().is_empty() && std::time::Instant::now() < end {
            egui::__run_test_ui(|ui| {
                render_media_browser(ui, &mut state, &tokens, &commands, &queue_mgr);
            });
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.model.visible().len(), 1);
        state.view = BrowserView::Thumbnails;
        egui::__run_test_ui(|ui| {
            render_media_browser(ui, &mut state, &tokens, &commands, &queue_mgr);
        });
        let entry = state.model.visible().into_iter().cloned().collect::<Vec<_>>();
        let mut added = Vec::new();
        activate(&mut state, &entry[0], &mut added);
        assert_eq!(added, vec![dir.path().join("a.mov")]);
    }

    #[test]
    fn column_widths_computation() {
        let cols = MediaBrowserColumnWidths::compute(600.0);
        assert_eq!(cols.kind, 70.0);
        assert_eq!(cols.size, 65.0);
        assert_eq!(cols.date, 115.0);
        assert!(cols.name >= 300.0);
    }

    #[test]
    fn tree_state_defaults() {
        let ts = MediaBrowserTreeState::default();
        assert!(ts.favorites_expanded);
        assert!(ts.user_folders_expanded);
        assert!(ts.drives_expanded);
    }

    #[test]
    fn column_settings_and_visibility() {
        let mut settings = MediaBrowserColumnSettings::default();
        let cols = MediaBrowserColumnWidths::compute_with_settings(800.0, &settings);
        assert_eq!(cols.kind, 80.0);
        assert_eq!(cols.created, 0.0);

        settings.visible.push("created".into());
        let cols_with_created = MediaBrowserColumnWidths::compute_with_settings(800.0, &settings);
        assert_eq!(cols_with_created.created, 130.0);
    }

    #[test]
    fn media_file_operations() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("original.mp4");
        std::fs::write(&file_path, b"test").unwrap();

        let renamed_path = dir.path().join("renamed.mp4");
        std::fs::rename(&file_path, &renamed_path).unwrap();
        assert!(!file_path.exists());
        assert!(renamed_path.exists());

        let subfolder = dir.path().join("subfolder");
        std::fs::create_dir(&subfolder).unwrap();
        assert!(subfolder.is_dir());

        let moved_path = subfolder.join("renamed.mp4");
        std::fs::rename(&renamed_path, &moved_path).unwrap();
        assert!(!renamed_path.exists());
        assert!(moved_path.exists());
    }
}
