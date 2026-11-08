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

/// Fixed column widths for pixel-aligned table layout in List mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaBrowserColumnWidths {
    pub name: f32,
    pub kind: f32,
    pub size: f32,
    pub date: f32,
}

impl MediaBrowserColumnWidths {
    pub fn compute(total_width: f32) -> Self {
        let kind = 70.0;
        let size = 65.0;
        let date = 115.0;
        let padding = 24.0;
        let name = (total_width - kind - size - date - padding).max(120.0);
        Self { name, kind, size, date }
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
                for k in [EntryKind::Video, EntryKind::Audio, EntryKind::Image] {
                    ui.selectable_value(&mut state.model.kind_filter, Some(k), k.label());
                }
            },
        );
        ui.checkbox(&mut state.model.show_hidden, "Hidden");
    });

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
}

fn activate(state: &mut MediaBrowserState, e: &Entry, to_add: &mut Vec<PathBuf>) {
    if e.kind == EntryKind::Folder {
        state.go(e.path.clone());
    } else if e.kind.is_media() {
        to_add.push(e.path.clone());
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
    let cols = MediaBrowserColumnWidths::compute(total_w);

    let current_sort = state.model.sort;
    let is_desc = state.model.descending;
    let sort_arrow = move |by: SortBy| {
        if current_sort == by { if is_desc { " \u{25be}" } else { " \u{25b4}" } } else { "" }
    };

    // Header bar
    ui.horizontal(|ui| {
        let name_resp = ui.add_sized(
            [cols.name, 18.0],
            egui::Button::new(RichText::new(format!("Name{}", sort_arrow(SortBy::Name))).small().strong()).frame(false),
        );
        if name_resp.clicked() {
            state.model.descending = state.model.sort == SortBy::Name && !state.model.descending;
            state.model.sort = SortBy::Name;
        }

        let type_resp = ui.add_sized(
            [cols.kind, 18.0],
            egui::Button::new(RichText::new(format!("Type{}", sort_arrow(SortBy::Kind))).small().strong()).frame(false),
        );
        if type_resp.clicked() {
            state.model.descending = state.model.sort == SortBy::Kind && !state.model.descending;
            state.model.sort = SortBy::Kind;
        }

        let size_resp = ui.add_sized(
            [cols.size, 18.0],
            egui::Button::new(RichText::new(format!("Size{}", sort_arrow(SortBy::Size))).small().strong()).frame(false),
        );
        if size_resp.clicked() {
            state.model.descending = state.model.sort == SortBy::Size && !state.model.descending;
            state.model.sort = SortBy::Size;
        }

        let date_resp = ui.add_sized(
            [cols.date, 18.0],
            egui::Button::new(RichText::new(format!("Modified{}", sort_arrow(SortBy::Modified))).small().strong()).frame(false),
        );
        if date_resp.clicked() {
            state.model.descending = state.model.sort == SortBy::Modified && !state.model.descending;
            state.model.sort = SortBy::Modified;
        }
    });

    // Cmd/Ctrl+A shortcut to select all files
    if ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::A)) {
        state.select_all(entries);
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

                    // Icon + Name
                    let icon = if e.kind == EntryKind::Folder { Icon::Folder } else { Icon::Preset };
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 14.0), egui::Sense::hover());
                    icons::paint(ui.painter(), icon_rect, icon, if e.kind == EntryKind::Folder { tokens.text_dim } else { tokens.accent_primary });

                    let name_label = RichText::new(&e.name).small().color(tokens.text_primary);
                    let resp = ui.add_sized([cols.name - 18.0, 18.0], egui::Label::new(name_label).sense(egui::Sense::click()).truncate());

                    // Type
                    ui.add_sized([cols.kind, 18.0], egui::Label::new(RichText::new(e.kind.label()).small().color(tokens.text_dim)).truncate());

                    // Size
                    let size_str = if e.kind == EntryKind::Folder { "--".to_string() } else { format_size(e.size) };
                    ui.add_sized([cols.size, 18.0], egui::Label::new(RichText::new(size_str).small().color(tokens.text_dim)).truncate());

                    // Modified
                    let date_str = format_age(e.modified, now);
                    ui.add_sized([cols.date, 18.0], egui::Label::new(RichText::new(date_str).small().color(tokens.text_dim)).truncate());

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
                        painter.rect_filled(img_rect.shrink(2.0), 2.0, tokens.bg_panel_darker);
                        if let Some(tex) = state.thumbs.get(&e.path) {
                            let size = tex.size_vec2();
                            let fitted = crate::monitor_geom::fit_rect(img_rect.shrink(2.0), size.x / size.y.max(1.0));
                            painter.image(
                                tex.id(),
                                fitted,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                        } else {
                            painter.text(
                                img_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                e.kind.label(),
                                egui::FontId::proportional(11.0),
                                tokens.text_dim,
                            );
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
}
