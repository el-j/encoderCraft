//! Watch Folders Panel: automated directory monitoring and ingest management (AME Parity).

use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use encodercraft_watch::{PostProcessAction, WatchFolderConfig, WatchFolderService};

use crate::icons;
use crate::theme::Tokens;

/// State for the watch folders panel.
pub struct WatchPanelState {
    pub configs: Vec<WatchFolderConfig>,
    pub selected_config_idx: Option<usize>,
    pub auto_encode_enabled: bool,
    pub last_scan_time: Option<std::time::Instant>,
    pub last_detected_count: usize,
    pub status_message: Option<String>,
}

impl Default for WatchPanelState {
    fn default() -> Self {
        Self {
            configs: Vec::new(),
            selected_config_idx: None,
            auto_encode_enabled: true,
            last_scan_time: None,
            last_detected_count: 0,
            status_message: None,
        }
    }
}

/// Renders the Watch Folders panel with management toolbar and configured folders table.
pub fn render_watch_panel(
    ui: &mut Ui,
    state: &mut WatchPanelState,
    queue_mgr: &Arc<QueueManager>,
    preset_registry: &Arc<Mutex<PresetRegistry>>,
    tokens: &Tokens,
) {
    ui.vertical(|ui| {
        // --- 1. Watch Folder Toolbar ---
        ui.horizontal(|ui| {
            if icons::button_with_label(ui, icons::Icon::Plus, "Add Watch Folder...", false, tokens, "Select an incoming directory to monitor")
                .clicked()
                && let Some(watch_dir) = rfd::FileDialog::new().set_title("Select Watch Folder to Monitor").pick_folder()
            {
                let out_dir = watch_dir.join("_encoded");
                let id = format!("watch-{}", state.configs.len() + 1);
                let cfg = WatchFolderConfig::new(id, watch_dir, out_dir, "Match Source - High Bitrate");
                state.configs.push(cfg);
                state.selected_config_idx = Some(state.configs.len() - 1);
                state.status_message = Some("Watch folder added. Files dropped here will be automatically encoded.".into());
            }

            let can_remove = state.selected_config_idx.is_some_and(|idx| idx < state.configs.len());
            ui.add_enabled_ui(can_remove, |ui| {
                if icons::button_with_label(ui, icons::Icon::Minus, "Remove", false, tokens, "Remove selected watch folder").clicked()
                    && let Some(idx) = state.selected_config_idx
                    && idx < state.configs.len()
                {
                    state.configs.remove(idx);
                    state.selected_config_idx = None;
                    state.status_message = Some("Watch folder removed.".into());
                }
            });

            ui.separator();

            let auto_icon = if state.auto_encode_enabled { icons::Icon::Check } else { icons::Icon::Close };
            let auto_label = if state.auto_encode_enabled { "Auto-Encode: Active" } else { "Auto-Encode: Paused" };
            if icons::button_with_label(ui, auto_icon, auto_label, state.auto_encode_enabled, tokens, "Toggle background auto-encode").clicked() {
                state.auto_encode_enabled = !state.auto_encode_enabled;
            }

            if ui.button("Scan Now").on_hover_text("Immediately scan watch folders for settled media files").clicked() {
                trigger_watch_scan(state, queue_mgr, preset_registry);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(ref msg) = state.status_message {
                    ui.label(RichText::new(msg).small().color(tokens.accent_primary));
                }
            });
        });

        ui.add_space(4.0);
        ui.separator();

        // --- 2. Watch Folders Table ---
        if state.configs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                icons::icon_label(ui, icons::Icon::Folder, 32.0, tokens.text_dim);
                ui.add_space(8.0);
                ui.heading(RichText::new("No Watch Folders Configured").color(tokens.text_dim));
                ui.label(
                    RichText::new("Watch folders automatically detect, settle, and encode files dropped into monitored directories.")
                        .color(tokens.text_dim),
                );
                ui.add_space(12.0);
                if ui.button("Add Your First Watch Folder...").clicked()
                    && let Some(watch_dir) = rfd::FileDialog::new().set_title("Select Watch Folder to Monitor").pick_folder()
                {
                    let out_dir = watch_dir.join("_encoded");
                    let id = format!("watch-{}", state.configs.len() + 1);
                    let cfg = WatchFolderConfig::new(id, watch_dir, out_dir, "Match Source - High Bitrate");
                    state.configs.push(cfg);
                    state.selected_config_idx = Some(0);
                }
            });
            return;
        }

        // Table Header
        egui::Frame::new().fill(tokens.bg_surface_active).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Active").strong().color(tokens.text_primary));
                ui.separator();
                ui.label(RichText::new("Watched Folder").strong().color(tokens.text_primary));
                ui.separator();
                ui.label(RichText::new("Output Destination").strong().color(tokens.text_primary));
                ui.separator();
                ui.label(RichText::new("Assigned Preset").strong().color(tokens.text_primary));
                ui.separator();
                ui.label(RichText::new("Post-Action").strong().color(tokens.text_primary));
            });
        });

        ui.add_space(2.0);

        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            let mut remove_idx = None;
            for (idx, cfg) in state.configs.iter_mut().enumerate() {
                let is_selected = state.selected_config_idx == Some(idx);
                let row_bg = if is_selected { tokens.bg_surface_active } else { tokens.bg_surface };

                egui::Frame::new().fill(row_bg).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Enabled checkbox
                        ui.checkbox(&mut cfg.enabled, "");

                        // Watch Directory Display
                        let watch_name = cfg.watch_dir.display().to_string();
                        let select_label = ui.selectable_label(is_selected, RichText::new(watch_name).strong());
                        if select_label.clicked() {
                            state.selected_config_idx = Some(idx);
                        }

                        ui.label(RichText::new("➔").color(tokens.text_dim));

                        // Output Directory Display
                        let out_name = cfg.output_dir.display().to_string();
                        ui.label(RichText::new(out_name).color(tokens.text_secondary));

                        // Preset Badge
                        ui.label(RichText::new(format!("[{}]", cfg.preset_name)).color(tokens.accent_primary));

                        // Post-process action label
                        let post_label = match &cfg.post_action {
                            PostProcessAction::LeaveInPlace => "Leave in place",
                            PostProcessAction::MoveToProcessed(_) => "Move to _processed",
                            PostProcessAction::DeleteSource => "Delete source",
                            PostProcessAction::MoveToErrors(_) => "Move to _errors",
                        };
                        ui.label(RichText::new(post_label).small().color(tokens.text_dim));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::button(ui, icons::Icon::Trash, 16.0, false, tokens, "Delete watch folder").clicked() {
                                remove_idx = Some(idx);
                            }
                            if icons::button_with_label(ui, icons::Icon::Folder, "Change Output...", false, tokens, "Select Output Directory")
                                .clicked()
                                && let Some(new_out) = rfd::FileDialog::new().set_title("Select Output Directory").pick_folder()
                            {
                                cfg.output_dir = new_out;
                            }
                        });
                    });
                });
                ui.add_space(2.0);
            }

            if let Some(r_idx) = remove_idx
                && r_idx < state.configs.len()
            {
                state.configs.remove(r_idx);
                state.selected_config_idx = None;
            }
        });
    });
}

/// Triggers a scan pass across all active watch folders and automatically queues settled jobs.
pub fn trigger_watch_scan(state: &mut WatchPanelState, queue_mgr: &Arc<QueueManager>, preset_registry: &Arc<Mutex<PresetRegistry>>) {
    if state.configs.is_empty() {
        return;
    }

    let registry = preset_registry.lock().map(|g| g.clone()).unwrap_or_default();
    let mut service = WatchFolderService::new(registry);
    for cfg in &state.configs {
        service.add_config(cfg.clone());
    }

    match service.scan_once() {
        Ok(jobs) => {
            let count = jobs.len();
            state.last_detected_count = count;
            state.last_scan_time = Some(std::time::Instant::now());

            for (_folder_id, spec) in jobs {
                let _ = queue_mgr.submit_job(spec);
            }

            if count > 0 {
                state.status_message = Some(format!("Found & queued {count} settled media files."));
            } else {
                state.status_message = Some("Watch scan complete: no new settled files.".into());
            }
        }
        Err(err) => {
            state.status_message = Some(format!("Scan error: {err}"));
        }
    }
}
