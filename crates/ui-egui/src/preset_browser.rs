//! Preset browser: searchable, categorized tree view of factory and user presets.

use std::sync::{Arc, Mutex, PoisonError};

use egui::{RichText, Ui};
use encodercraft_engine::QueueManager;
use encodercraft_presets::{EncodingPreset, PresetCategory, PresetRegistry};

use crate::icons;
use crate::theme::Tokens;

/// State for the preset browser panel.
#[derive(Default)]
pub struct PresetBrowserState {
    pub search_query: String,
    pub selected_category: Option<PresetCategory>,
    pub selected_preset: Option<EncodingPreset>,
    pub editor: crate::preset_editor::PresetEditorState,
}

/// Renders the preset browser pane.
pub fn render_preset_browser(
    ui: &mut Ui,
    state: &mut PresetBrowserState,
    registry: &Arc<Mutex<PresetRegistry>>,
    queue_mgr: &Arc<QueueManager>,
    selected_job_id: Option<&str>,
    tokens: &Tokens,
) {
    ui.vertical(|ui| {
        ui.heading(RichText::new("Preset Browser").size(15.0));
        ui.add_space(4.0);

        // Search input & New Preset action
        ui.horizontal(|ui| {
            ui.label(icons::SEARCH);
            ui.add(egui::TextEdit::singleline(&mut state.search_query).hint_text("Search presets..."));
            if ui.button(RichText::new("+ New").strong().color(tokens.accent_primary)).clicked() {
                state.editor.open_new();
            }
        });

        ui.add_space(4.0);

        // Category filter chips
        ui.horizontal_wrapped(|ui| {
            let all_sel = state.selected_category.is_none();
            if ui.selectable_label(all_sel, "All").clicked() {
                state.selected_category = None;
            }

            let categories = [
                (PresetCategory::Web, "Web"),
                (PresetCategory::Broadcast, "Broadcast"),
                (PresetCategory::SocialMedia, "Social"),
                (PresetCategory::Audio, "Audio"),
                (PresetCategory::Spatial3D, "3D"),
            ];

            for (cat, label) in categories {
                let is_sel = state.selected_category == Some(cat);
                if ui.selectable_label(is_sel, label).clicked() {
                    state.selected_category = Some(cat);
                }
            }
        });

        ui.separator();

        // Preset items list
        let reg = registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut matching_presets: Vec<EncodingPreset> = reg.list().to_vec();
        drop(reg);

        let query = state.search_query.to_lowercase();
        matching_presets.retain(|p| {
            let matches_cat = match state.selected_category {
                Some(cat) => p.category == cat,
                None => true,
            };
            let matches_query = query.is_empty() || p.name.to_lowercase().contains(&query) || p.description.to_lowercase().contains(&query);
            matches_cat && matches_query
        });

        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for preset in matching_presets {
                let is_sel = state.selected_preset.as_ref().map(|p| &p.name) == Some(&preset.name);
                ui.horizontal(|ui| {
                    ui.label(icons::PRESET);
                    let label = ui.selectable_label(is_sel, &preset.name);
                    if label.clicked() {
                        state.selected_preset = Some(preset.clone());
                    }
                });
            }
        });

        // Detail card & Apply button for selected preset
        if let Some(ref preset) = state.selected_preset {
            ui.separator();
            egui::Frame::new()
                .fill(tokens.bg_panel_darker)
                .stroke(egui::Stroke::new(1.0, tokens.border_subtle))
                .corner_radius(4)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(&preset.name).strong().color(tokens.accent_primary));
                    ui.label(RichText::new(&preset.description).small().color(tokens.text_dim));

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Container:").small().color(tokens.text_dim));
                        ui.label(RichText::new(format!("{:?}", preset.container)).small());
                    });

                    if let Some(ref v) = preset.video {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Video:").small().color(tokens.text_dim));
                            ui.label(RichText::new(format!("{:?}", v.codec)).small());
                        });
                    }

                    if let Some(ref a) = preset.audio {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Audio:").small().color(tokens.text_dim));
                            ui.label(RichText::new(format!("{:?}", a.codec)).small());
                        });
                    }

                    ui.add_space(4.0);

                    // Apply to selected job button
                    let has_job = selected_job_id.is_some();
                    let apply_btn = ui.add_enabled(has_job, egui::Button::new(format!("{} Apply to Selected Job", icons::STATUS_OK)));

                    if apply_btn.clicked()
                        && let Some(job_id) = selected_job_id
                    {
                        apply_preset_to_job(job_id, preset, queue_mgr);
                    }
                });
        }
    });

    // Render preset editor modal dialog if active
    crate::preset_editor::render_preset_editor(ui.ctx(), &mut state.editor, registry, tokens);
}

fn apply_preset_to_job(job_id: &str, preset: &EncodingPreset, queue_mgr: &Arc<QueueManager>) {
    let jobs = queue_mgr.list_jobs();
    if let Some(job) = jobs.iter().find(|j| j.spec.job_id == job_id) {
        let mut new_spec = job.spec.clone();
        if let Some(target) = new_spec.targets.first_mut() {
            target.preset_name = preset.name.clone();
            target.container = preset.container;
        }
        // Update job
        let _ = queue_mgr.remove_job(job_id);
        let _ = queue_mgr.submit_job(new_spec);
    }
}
