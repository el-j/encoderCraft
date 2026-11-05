//! Preset browser: searchable, categorized tree view of factory and user presets.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::CraftJobSpec;
use encodercraft_engine::{QueueManager, cmd::CommandRegistry};
use encodercraft_presets::catalog::{self, SortColumn};
use encodercraft_presets::{EncodingPreset, PresetCategory};

use crate::icons;
use crate::theme::Tokens;

/// State for the preset browser panel.
#[derive(Default)]
pub struct PresetBrowserState {
    pub search_query: String,
    pub selected_category: Option<PresetCategory>,
    pub selected_preset: Option<EncodingPreset>,
    pub editor: crate::preset_editor::PresetEditorState,
    pub sort_column: Option<SortColumn>,
    pub sort_descending: bool,
    pub status: Option<String>,
    pub pending_updates: Vec<(String, CraftJobSpec)>,
}

/// Renders the preset browser pane.
pub fn render_preset_browser(
    ui: &mut Ui,
    state: &mut PresetBrowserState,
    queue_mgr: &Arc<QueueManager>,
    commands: &CommandRegistry,
    selected_job_ids: &[String],
    tokens: &Tokens,
) -> Option<crate::commands::AppCommand> {
    let mut chosen_command = None;
    ui.vertical(|ui| {
        ui.heading(RichText::new("Preset Browser").size(15.0));
        ui.add_space(4.0);

        // Search input & New Preset action
        ui.horizontal(|ui| {
            icons::icon_label(ui, icons::Icon::Search, 14.0, tokens.text_dim);
            ui.add(egui::TextEdit::singleline(&mut state.search_query).hint_text("Search presets..."));
            if icons::button_with_label(ui, icons::Icon::Plus, "New", false, tokens, "Create custom encoding preset").clicked() {
                chosen_command = Some(crate::commands::AppCommand::CreatePreset);
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
                (PresetCategory::StillImage, "Image"),
                (PresetCategory::Spatial3D, "3D"),
                (PresetCategory::Vector, "Vector"),
                (PresetCategory::Document, "Document"),
            ];

            for (cat, label) in categories {
                let is_sel = state.selected_category == Some(cat);
                if ui.selectable_label(is_sel, label).clicked() {
                    state.selected_category = Some(cat);
                }
            }
        });

        ui.separator();

        // Import / export / delete
        ui.horizontal(|ui| {
            if ui.button("Import...").on_hover_text("Import an encoderCraft preset bundle (.json)").clicked() {
                chosen_command = Some(crate::commands::AppCommand::ImportPresets);
            }
            if ui.button("Export...").on_hover_text("Export user presets as an encoderCraft preset bundle").clicked() {
                chosen_command = Some(crate::commands::AppCommand::ExportPresets);
            }
            let deletable = state.selected_preset.as_ref().is_some_and(|preset| preset.category == PresetCategory::Custom);
            let delete_label = crate::commands::AppCommand::DeletePreset.info().map_or("Delete Preset", |info| info.label);
            if ui.add_enabled(deletable, egui::Button::new(delete_label)).on_hover_text("Delete the selected user preset").clicked() {
                chosen_command = Some(crate::commands::AppCommand::DeletePreset);
            }
        });
        if let Some(msg) = &state.status {
            ui.label(RichText::new(msg).small().color(tokens.text_dim));
        }

        // Preset table
        let presets: Vec<EncodingPreset> = match commands.dispatch(queue_mgr, "preset.list", serde_json::json!({})) {
            Ok(value) => match serde_json::from_value(value) {
                Ok(presets) => presets,
                Err(error) => {
                    state.status = Some(format!("Preset list returned invalid data: {error}"));
                    Vec::new()
                }
            },
            Err(error) => {
                state.status = Some(format!("Could not list presets: {error}"));
                Vec::new()
            }
        };
        let mut table = catalog::filter_rows(catalog::rows(&presets), &state.search_query, state.selected_category);
        if let Some(col) = state.sort_column {
            catalog::sort_rows(&mut table, col, state.sort_descending);
        }

        egui::ScrollArea::both().max_height(240.0).show(ui, |ui| {
            egui::Grid::new("preset_table").striped(true).num_columns(5).show(ui, |ui| {
                for (label, col) in [
                    ("Preset Name", SortColumn::Name),
                    ("Format", SortColumn::Format),
                    ("Frame Size", SortColumn::FrameSize),
                    ("Frame Rate", SortColumn::FrameRate),
                    ("Target Rate", SortColumn::TargetRate),
                ] {
                    let arrow = match (state.sort_column == Some(col), state.sort_descending) {
                        (true, true) => " v",
                        (true, false) => " ^",
                        _ => "",
                    };
                    if ui.button(RichText::new(format!("{label}{arrow}")).small().strong()).clicked() {
                        state.sort_descending = state.sort_column == Some(col) && !state.sort_descending;
                        state.sort_column = Some(col);
                    }
                }
                ui.end_row();

                for row in &table {
                    let Some(preset) = presets.iter().find(|p| p.name == row.name) else { continue };
                    let is_sel = state.selected_preset.as_ref().map(|p| &p.name) == Some(&row.name);
                    let name = ui.selectable_label(is_sel, &row.name).on_hover_text(&row.comment);
                    if name.clicked() {
                        state.selected_preset = Some(preset.clone());
                    }
                    if name.double_clicked() {
                        state.selected_preset = Some(preset.clone());
                        chosen_command = Some(crate::commands::AppCommand::ApplyPreset);
                    }
                    name.context_menu(|ui| {
                        state.selected_preset = Some(preset.clone());
                        let preset_deletable = preset.category == PresetCategory::Custom;
                        let applicable = !selected_job_ids.is_empty() && !preset_incompatible_with_jobs(preset, selected_job_ids, queue_mgr);
                        for info in crate::commands::context_menu_commands(crate::commands::CommandContextMenu::Preset) {
                            let enabled = if info.command == crate::commands::AppCommand::DeletePreset { preset_deletable } else { applicable };
                            if ui.add_enabled(enabled, egui::Button::new(info.context_label(crate::commands::CommandContextMenu::Preset))).clicked() {
                                chosen_command = Some(info.command);
                                ui.close();
                            }
                        }
                    });
                    for cell in [&row.format, &row.frame_size, &row.frame_rate, &row.target_rate] {
                        ui.label(RichText::new(cell).small().color(tokens.text_secondary));
                    }
                    ui.end_row();
                }
            });
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

                    // AME Format Guardrail: Incompatible source-to-preset format guards
                    let has_jobs = !selected_job_ids.is_empty();
                    let incompatible_warning: Option<&'static str> = if has_jobs {
                        let any_audio = selected_job_ids.iter().any(|id| is_job_audio_only(id, queue_mgr));
                        let any_doc =
                            selected_job_ids.iter().any(|id| get_job_stream_kind(id, queue_mgr).is_some_and(|k| k.is_document() || k.is_vector()));
                        if any_audio && preset.video.is_some() {
                            Some("Cannot apply video presets to audio-only files (MP3/WAV/FLAC)")
                        } else if any_doc && preset.video.is_some() {
                            Some("Cannot apply video presets to Document / Vector files without rasterization")
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let is_incompatible = incompatible_warning.is_some();

                    if let Some(warn_text) = incompatible_warning {
                        ui.horizontal(|ui| {
                            icons::icon_label(ui, icons::Icon::Info, 13.0, tokens.status_failed);
                            ui.label(RichText::new(format!("Format Guard: {warn_text}")).small().color(tokens.status_failed));
                        });
                        ui.add_space(2.0);
                    }

                    // Apply to selected job buttons (batch support)
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(has_jobs && !is_incompatible, |ui| {
                            let apply_btn = icons::button_with_label(
                                ui,
                                icons::Icon::Check,
                                crate::commands::AppCommand::ApplyPreset.info().map_or("Apply Preset", |info| info.label),
                                false,
                                tokens,
                                if is_incompatible {
                                    "Disabled: Preset is incompatible with source media stream type"
                                } else {
                                    "Replace target preset on selected job(s)"
                                },
                            );
                            if apply_btn.clicked() {
                                chosen_command = Some(crate::commands::AppCommand::ApplyPreset);
                            }

                            let add_target_btn = icons::button_with_label(
                                ui,
                                icons::Icon::Plus,
                                crate::commands::AppCommand::AddPresetTarget.info().map_or("Add Preset as Output", |info| info.label),
                                false,
                                tokens,
                                if is_incompatible {
                                    "Disabled: Preset is incompatible with source media stream type"
                                } else {
                                    "Add this preset as an additional output target on selected job(s)"
                                },
                            );
                            if add_target_btn.clicked() {
                                chosen_command = Some(crate::commands::AppCommand::AddPresetTarget);
                            }
                        });
                    });
                });
        }
    });

    // Render preset editor modal dialog if active
    crate::preset_editor::render_preset_editor(ui.ctx(), &mut state.editor, commands, queue_mgr, tokens);
    chosen_command
}

fn get_job_stream_kind(job_id: &str, queue_mgr: &Arc<QueueManager>) -> Option<encodercraft_core::MediaStreamKind> {
    let jobs = queue_mgr.list_jobs();
    jobs.iter().find(|j| j.spec.job_id == job_id).map(|j| match &j.spec.source {
        encodercraft_core::SourceDescriptor::File { path, .. } => encodercraft_demux::classify_stream_kind(path),
        _ => encodercraft_core::MediaStreamKind::VideoWithAudio,
    })
}

fn is_job_audio_only(job_id: &str, queue_mgr: &Arc<QueueManager>) -> bool {
    get_job_stream_kind(job_id, queue_mgr).is_some_and(|k| k.is_audio_only())
}

pub(crate) fn preset_incompatible_with_jobs(preset: &EncodingPreset, job_ids: &[String], queue_mgr: &Arc<QueueManager>) -> bool {
    preset.video.is_some()
        && job_ids.iter().any(|id| {
            is_job_audio_only(id, queue_mgr) || get_job_stream_kind(id, queue_mgr).is_some_and(|kind| kind.is_document() || kind.is_vector())
        })
}
