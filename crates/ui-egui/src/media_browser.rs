//! Media Browser panel: sidebar of locations and favourites, list and thumbnail views.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use egui::{RichText, Ui, Vec2};

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

pub struct MediaBrowserState {
    pub model: BrowserModel,
    pub view: BrowserView,
    pub thumb_size: f32,
    pub selected: Option<PathBuf>,
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

pub fn format_age(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(secs) = modified.and_then(|m| now.duration_since(m).ok()).map(|d| d.as_secs()) else { return "-".into() };
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
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

    ui.horizontal(|ui| {
        if ui.add_enabled(state.model.can_back(), egui::Button::new("<")).on_hover_text("Back").clicked() {
            state.model.back();
            state.path_input = state.model.location().display().to_string();
        }
        if ui.add_enabled(state.model.can_forward(), egui::Button::new(">")).on_hover_text("Forward").clicked() {
            state.model.forward();
            state.path_input = state.model.location().display().to_string();
        }
        if ui.button("Up").on_hover_text("Parent folder").clicked() {
            state.model.up();
            state.path_input = state.model.location().display().to_string();
        }
        let loc = ui.add(egui::TextEdit::singleline(&mut state.path_input).desired_width(260.0).hint_text("Location"));
        if loc.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let p = PathBuf::from(state.path_input.trim());
            state.go(p);
        }
        if ui.button("Refresh").clicked() {
            state.model.refresh();
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.model.search).hint_text("Search").desired_width(140.0));
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
        ui.separator();
        ui.selectable_value(&mut state.view, BrowserView::List, "List");
        ui.selectable_value(&mut state.view, BrowserView::Thumbnails, "Thumbnails");
        if state.view == BrowserView::Thumbnails {
            ui.add(egui::Slider::new(&mut state.thumb_size, 48.0..=200.0).show_value(false));
        }
    });
    ui.separator();

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(130.0);
            ui.label(RichText::new("Favorites").small().color(tokens.text_dim));
            let favs = state.model.favorites.clone();
            if favs.is_empty() {
                ui.label(RichText::new("None yet").small().color(tokens.text_dim));
            }
            for f in favs {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
                if ui.link(name).on_hover_text(f.display().to_string()).clicked() {
                    state.go(f);
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Locations").small().color(tokens.text_dim));
            for (label, p) in state.locations.clone() {
                if ui.link(label).on_hover_text(p.display().to_string()).clicked() {
                    state.go(p);
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            if let Some(err) = &state.model.error {
                ui.label(RichText::new(err).color(tokens.status_failed));
            }
            if state.model.truncated() {
                ui.label(RichText::new("Showing the first 20000 items of this folder").small().color(tokens.text_dim));
            }
            let entries: Vec<Entry> = state.model.visible().into_iter().cloned().collect();
            match state.view {
                BrowserView::List => list_view(ui, state, &entries, tokens, &mut to_add, commands, queue_mgr),
                BrowserView::Thumbnails => thumbnail_view(ui, state, &entries, tokens, &mut to_add, commands, queue_mgr),
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
            crate::commands::AppCommand::AddMediaItem => e.kind.is_media(),
            crate::commands::AppCommand::OpenMediaFolder | crate::commands::AppCommand::ToggleMediaFavorite => e.kind == EntryKind::Folder,
            _ => false,
        };
        if ui.add_enabled(enabled, egui::Button::new(info.context_label(crate::commands::CommandContextMenu::MediaItem))).clicked() {
            match info.command {
                crate::commands::AppCommand::AddMediaItem => to_add.push(e.path.clone()),
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
    ui.horizontal(|ui| {
        for (label, by) in [("Name", SortBy::Name), ("Type", SortBy::Kind), ("Size", SortBy::Size), ("Modified", SortBy::Modified)] {
            let arrow = if state.model.sort == by { if state.model.descending { " v" } else { " ^" } } else { "" };
            if ui.button(RichText::new(format!("{label}{arrow}")).small().strong()).clicked() {
                state.model.descending = state.model.sort == by && !state.model.descending;
                state.model.sort = by;
            }
        }
    });
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, entries.len(), |ui, range| {
        for e in entries.iter().skip(range.start).take(range.len()) {
            let selected = state.selected.as_ref() == Some(&e.path);
            ui.horizontal(|ui| {
                let resp = ui.selectable_label(selected, &e.name);
                ui.label(RichText::new(e.kind.label()).small().color(tokens.text_dim));
                if e.kind != EntryKind::Folder {
                    ui.label(RichText::new(format_size(e.size)).small().color(tokens.text_dim));
                }
                ui.label(RichText::new(format_age(e.modified, now)).small().color(tokens.text_dim));
                if resp.clicked() {
                    state.selected = Some(e.path.clone());
                }
                if resp.double_clicked() {
                    activate(state, e, to_add);
                }
                resp.context_menu(|ui| entry_menu(ui, state, e, to_add, commands, queue_mgr));
            });
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Enter))
        && !ui.ctx().egui_wants_keyboard_input()
        && let Some(sel) = entries.iter().find(|e| state.selected.as_ref() == Some(&e.path))
    {
        activate(state, sel, to_add);
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
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell.y + 8.0, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for e in entries.iter().skip(row * cols).take(cols) {
                    state.want_thumb(e);
                    let selected = state.selected.as_ref() == Some(&e.path);
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
                        painter.image(tex.id(), fitted, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)), egui::Color32::WHITE);
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
                        state.selected = Some(e.path.clone());
                    }
                    if resp.double_clicked() {
                        activate(state, e, to_add);
                    }
                    resp.on_hover_text(&e.name).context_menu(|ui| entry_menu(ui, state, e, to_add, commands, queue_mgr));
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
        assert_eq!(format_age(Some(now - Duration::from_secs(120)), now), "2 min ago");
        assert_eq!(format_age(Some(now + Duration::from_secs(120)), now), "-");
        assert_eq!(format_age(Some(now - Duration::from_secs(3 * 86_400)), now), "3 d ago");
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
}
