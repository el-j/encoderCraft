//! Preset browser: searchable, categorized tree view of factory and user presets.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::CraftJobSpec;
use encodercraft_engine::{QueueManager, cmd::CommandRegistry};
use encodercraft_presets::catalog::{self, PresetRow, SortColumn};
use encodercraft_presets::{EncodingPreset, PresetCategory};

use crate::icons;
use crate::theme::Tokens;

/// Returns a user-friendly human readable name for video codecs.
pub fn human_video_codec_name(codec: Option<&str>) -> &'static str {
    match codec.map(str::to_ascii_lowercase).as_deref() {
        Some("h264" | "avc" | "x264") => "H.264 / AVC",
        Some("hevc" | "h265" | "x265") => "H.265 / HEVC",
        Some("av1" | "rav1e" | "svt-av1") => "AV1",
        Some("vp9") => "VP9",
        Some("vp8") => "VP8",
        Some("prores") => "Apple ProRes",
        Some("dnxhd" | "dnxhr") => "Avid DNxHD / DNxHR",
        Some("mjpeg") => "Motion JPEG",
        Some("rawvideo" | "uncompressed") => "Uncompressed",
        Some(_) => "Custom Codec",
        None => "Match Source",
    }
}

/// Returns a user-friendly human readable name for audio codecs.
pub fn human_audio_codec_name(codec: Option<&str>) -> &'static str {
    match codec.map(str::to_ascii_lowercase).as_deref() {
        Some("aac") => "AAC",
        Some("pcm" | "pcm_s16le" | "pcm_s24le" | "pcm_f32le") => "Uncompressed PCM",
        Some("mp3") => "MP3",
        Some("flac") => "FLAC Lossless",
        Some("vorbis" | "ogg") => "Ogg Vorbis",
        Some("opus") => "Opus",
        Some("ac3") => "Dolby Digital (AC-3)",
        Some(_) => "Custom Audio",
        None => "Match Source",
    }
}

/// State for the preset browser panel.
pub struct PresetBrowserState {
    pub search_query: String,
    pub selected_category: Option<PresetCategory>,
    pub selected_preset: Option<EncodingPreset>,
    pub editor: crate::preset_editor::PresetEditorState,
    pub sort_column: Option<SortColumn>,
    pub sort_descending: bool,
    pub status: Option<String>,
    pub pending_updates: Vec<(String, CraftJobSpec)>,
    pub expanded_groups: std::collections::HashSet<String>,
}

impl Default for PresetBrowserState {
    fn default() -> Self {
        let mut expanded_groups = std::collections::HashSet::new();
        expanded_groups.insert("User Presets & Groups".to_string());
        expanded_groups.insert("System Presets".to_string());
        expanded_groups.insert("Broadcast & Production".to_string());
        expanded_groups.insert("Web Video & Streaming".to_string());
        Self {
            search_query: String::new(),
            selected_category: None,
            selected_preset: None,
            editor: crate::preset_editor::PresetEditorState::default(),
            sort_column: None,
            sort_descending: false,
            status: None,
            pending_updates: Vec::new(),
            expanded_groups,
        }
    }
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
        // --- 1. Top Search & Action Bar (Full Width Search with Apply and New buttons) ---
        ui.horizontal(|ui| {
            icons::icon_label(ui, icons::Icon::Search, 14.0, tokens.text_dim);
            let search_width = (ui.available_width() - 170.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut state.search_query).desired_width(search_width).hint_text("Search presets..."));

            let can_apply = state.selected_preset.is_some() && !selected_job_ids.is_empty();
            ui.add_enabled_ui(can_apply, |ui| {
                if icons::button_with_label(ui, icons::Icon::Check, "Apply", false, tokens, "Apply selected preset to queue (double-click)").clicked()
                {
                    chosen_command = Some(crate::commands::AppCommand::ApplyPreset);
                }
            });

            if icons::button_with_label(ui, icons::Icon::Plus, "New", false, tokens, "Create custom encoding preset").clicked() {
                chosen_command = Some(crate::commands::AppCommand::CreatePreset);
            }
        });

        ui.add_space(tokens.section_gap);

        // --- 2. Secondary Action Bar (Import / Export / Delete) ---
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

        ui.add_space(2.0);

        // --- 3. Presets Query & Table Header ---
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

        render_preset_table_header(ui, state, tokens);
        ui.separator();

        // --- 4. Hierarchical Tree Scroll Area ---
        egui::ScrollArea::both().auto_shrink([false; 2]).show(ui, |ui| {
            let is_searching = !state.search_query.trim().is_empty();

            if is_searching {
                // Flat search results list
                let mut table = catalog::filter_rows(catalog::rows(&presets), &state.search_query, state.selected_category);
                if let Some(col) = state.sort_column {
                    catalog::sort_rows(&mut table, col, state.sort_descending);
                }

                if table.is_empty() {
                    ui.add_space(16.0);
                    ui.label(RichText::new("No matching presets found.").color(tokens.text_dim));
                } else {
                    for row in &table {
                        let Some(preset) = presets.iter().find(|p| p.name == row.name) else { continue };
                        if let Some(cmd) = render_preset_row(ui, row, preset, state, selected_job_ids, queue_mgr, tokens) {
                            chosen_command = Some(cmd);
                        }
                    }
                }
            } else {
                // Tree Group 1: User Presets & Groups
                let user_group_name = "User Presets & Groups";
                let is_user_expanded = state.expanded_groups.contains(user_group_name);
                let user_presets: Vec<EncodingPreset> = presets.iter().filter(|p| p.category == PresetCategory::Custom).cloned().collect();

                ui.horizontal(|ui| {
                    let chevron = if is_user_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
                    if icons::button(ui, chevron, 16.0, false, tokens, "Toggle User Presets").clicked() {
                        if is_user_expanded {
                            state.expanded_groups.remove(user_group_name);
                        } else {
                            state.expanded_groups.insert(user_group_name.to_string());
                        }
                    }
                    icons::icon_label(ui, icons::Icon::Folder, 14.0, tokens.accent_primary);
                    ui.label(RichText::new(format!("{user_group_name} ({})", user_presets.len())).strong());
                });

                if is_user_expanded {
                    if user_presets.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new("(No user presets created yet. Click 'New' above to create one.)").small().color(tokens.text_dim));
                        });
                    } else {
                        let user_rows = catalog::rows(&user_presets);
                        for row in &user_rows {
                            let Some(preset) = user_presets.iter().find(|p| p.name == row.name) else { continue };
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                if let Some(cmd) = render_preset_row(ui, row, preset, state, selected_job_ids, queue_mgr, tokens) {
                                    chosen_command = Some(cmd);
                                }
                            });
                        }
                    }
                }

                ui.add_space(2.0);

                // Tree Group 2: System Presets
                let sys_group_name = "System Presets";
                let is_sys_expanded = state.expanded_groups.contains(sys_group_name);
                let system_presets: Vec<EncodingPreset> = presets.iter().filter(|p| p.category != PresetCategory::Custom).cloned().collect();

                ui.horizontal(|ui| {
                    let chevron = if is_sys_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
                    if icons::button(ui, chevron, 16.0, false, tokens, "Toggle System Presets").clicked() {
                        if is_sys_expanded {
                            state.expanded_groups.remove(sys_group_name);
                        } else {
                            state.expanded_groups.insert(sys_group_name.to_string());
                        }
                    }
                    icons::icon_label(ui, icons::Icon::Folder, 14.0, tokens.text_dim);
                    ui.label(RichText::new(format!("{sys_group_name} ({})", system_presets.len())).strong());
                });

                if is_sys_expanded {
                    for cat in PresetCategory::ALL_SYSTEM {
                        let cat_presets: Vec<EncodingPreset> = system_presets.iter().filter(|p| p.category == *cat).cloned().collect();
                        if cat_presets.is_empty() {
                            continue;
                        }
                        let cat_name = cat.label();
                        let is_cat_expanded = state.expanded_groups.contains(cat_name);

                        ui.horizontal(|ui| {
                            ui.add_space(18.0);
                            let chevron = if is_cat_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
                            if icons::button(ui, chevron, 14.0, false, tokens, "Toggle category").clicked() {
                                if is_cat_expanded {
                                    state.expanded_groups.remove(cat_name);
                                } else {
                                    state.expanded_groups.insert(cat_name.to_string());
                                }
                            }
                            icons::icon_label(ui, icons::Icon::Preset, 13.0, tokens.text_dim);
                            ui.label(RichText::new(format!("{cat_name} ({})", cat_presets.len())).small().color(tokens.text_secondary));
                        });

                        if is_cat_expanded {
                            let cat_rows = catalog::rows(&cat_presets);
                            for row in &cat_rows {
                                let Some(preset) = cat_presets.iter().find(|p| p.name == row.name) else { continue };
                                ui.horizontal(|ui| {
                                    ui.add_space(36.0);
                                    if let Some(cmd) = render_preset_row(ui, row, preset, state, selected_job_ids, queue_mgr, tokens) {
                                        chosen_command = Some(cmd);
                                    }
                                });
                            }
                        }
                    }
                }
            }
        });

        // --- 5. Detail Card & Actions for Selected Preset ---
        if let Some(ref preset) = state.selected_preset {
            ui.separator();
            egui::Frame::new()
                .fill(tokens.bg_panel_darker)
                .stroke(egui::Stroke::new(1.0, tokens.border_subtle))
                .corner_radius(tokens.radius_sm)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(&preset.name).strong().color(tokens.accent_primary));
                    ui.label(RichText::new(&preset.description).small().color(tokens.text_dim));

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Container:").small().color(tokens.text_dim));
                        ui.label(RichText::new(preset.container.display_name()).small());
                    });

                    if let Some(ref v) = preset.video {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Video:").small().color(tokens.text_dim));
                            ui.label(RichText::new(human_video_codec_name(Some(&v.codec))).small());
                        });
                    }

                    if let Some(ref a) = preset.audio {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Audio:").small().color(tokens.text_dim));
                            ui.label(RichText::new(human_audio_codec_name(Some(&a.codec))).small());
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

fn render_preset_table_header(ui: &mut Ui, state: &mut PresetBrowserState, tokens: &Tokens) {
    egui::Frame::new().fill(tokens.bg_panel_darker).inner_margin(egui::vec2(tokens.panel_inset, 3.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            for (label, col, width) in [
                ("Preset Name", SortColumn::Name, 170.0),
                ("Format", SortColumn::Format, 110.0),
                ("Frame Size", SortColumn::FrameSize, 90.0),
                ("Frame Rate", SortColumn::FrameRate, 85.0),
                ("Target Rate", SortColumn::TargetRate, 85.0),
            ] {
                let arrow = match (state.sort_column == Some(col), state.sort_descending) {
                    (true, true) => " ▼",
                    (true, false) => " ▲",
                    _ => "",
                };
                let resp = ui.add_sized([width, 18.0], egui::Button::new(RichText::new(format!("{label}{arrow}")).small().strong()).frame(false));
                if resp.clicked() {
                    state.sort_descending = state.sort_column == Some(col) && !state.sort_descending;
                    state.sort_column = Some(col);
                }
            }
        });
    });
}

fn render_preset_row(
    ui: &mut Ui,
    row: &PresetRow,
    preset: &EncodingPreset,
    state: &mut PresetBrowserState,
    selected_job_ids: &[String],
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
) -> Option<crate::commands::AppCommand> {
    let mut chosen = None;
    let is_sel = state.selected_preset.as_ref().map(|p| &p.name) == Some(&row.name);

    ui.horizontal(|ui| {
        let name_resp = ui.add_sized([170.0, tokens.row_height], egui::Button::new(&row.name).selected(is_sel));
        if name_resp.clicked() {
            state.selected_preset = Some(preset.clone());
        }
        if name_resp.double_clicked() {
            state.selected_preset = Some(preset.clone());
            chosen = Some(crate::commands::AppCommand::ApplyPreset);
        }
        name_resp.context_menu(|ui| {
            state.selected_preset = Some(preset.clone());
            let preset_deletable = preset.category == PresetCategory::Custom;
            let applicable = !selected_job_ids.is_empty() && !preset_incompatible_with_jobs(preset, selected_job_ids, queue_mgr);
            for info in crate::commands::context_menu_commands(crate::commands::CommandContextMenu::Preset) {
                let enabled = if info.command == crate::commands::AppCommand::DeletePreset { preset_deletable } else { applicable };
                if ui.add_enabled(enabled, egui::Button::new(info.context_label(crate::commands::CommandContextMenu::Preset))).clicked() {
                    chosen = Some(info.command);
                    ui.close();
                }
            }
        });

        ui.add_sized([110.0, tokens.row_height], egui::Label::new(RichText::new(&row.format).small().color(tokens.text_secondary)));
        ui.add_sized([90.0, tokens.row_height], egui::Label::new(RichText::new(&row.frame_size).small().color(tokens.text_secondary)));
        ui.add_sized([85.0, tokens.row_height], egui::Label::new(RichText::new(&row.frame_rate).small().color(tokens.text_secondary)));
        ui.add_sized([85.0, tokens.row_height], egui::Label::new(RichText::new(&row.target_rate).small().color(tokens.text_secondary)));
    });

    chosen
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_readable_codec_names() {
        assert_eq!(human_video_codec_name(Some("h264")), "H.264 / AVC");
        assert_eq!(human_video_codec_name(Some("hevc")), "H.265 / HEVC");
        assert_eq!(human_video_codec_name(Some("prores")), "Apple ProRes");
        assert_eq!(human_video_codec_name(None), "Match Source");

        assert_eq!(human_audio_codec_name(Some("aac")), "AAC");
        assert_eq!(human_audio_codec_name(Some("pcm")), "Uncompressed PCM");
        assert_eq!(human_audio_codec_name(None), "Match Source");
    }

    #[test]
    fn preset_browser_state_defaults_expand_system_and_user_groups() {
        let state = PresetBrowserState::default();
        assert!(state.expanded_groups.contains("User Presets & Groups"));
        assert!(state.expanded_groups.contains("System Presets"));
    }
}
