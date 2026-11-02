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
    selected_job_ids: &[String],
    tokens: &Tokens,
) {
    ui.vertical(|ui| {
        ui.heading(RichText::new("Preset Browser").size(15.0));
        ui.add_space(4.0);

        // Search input & New Preset action
        ui.horizontal(|ui| {
            icons::icon_label(ui, icons::Icon::Search, 14.0, tokens.text_dim);
            ui.add(egui::TextEdit::singleline(&mut state.search_query).hint_text("Search presets..."));
            if icons::button_with_label(ui, icons::Icon::Plus, "New", false, tokens, "Create custom encoding preset").clicked() {
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
                    icons::icon_label(ui, icons::Icon::Preset, 13.0, tokens.accent_primary);
                    let label = ui.selectable_label(is_sel, &preset.name);
                    if label.clicked() {
                        state.selected_preset = Some(preset.clone());
                    }
                    if label.double_clicked() {
                        state.selected_preset = Some(preset.clone());
                        for job_id in selected_job_ids {
                            apply_preset_to_job(job_id, &preset, queue_mgr);
                        }
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
                    let count = selected_job_ids.len();
                    let apply_text = if count > 1 { format!("Apply to {count} Jobs") } else { "Apply to Job".into() };
                    let add_target_text = if count > 1 { format!("Add as Output to {count} Jobs") } else { "Add as Output".into() };

                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(has_jobs && !is_incompatible, |ui| {
                            let apply_btn = icons::button_with_label(
                                ui,
                                icons::Icon::Check,
                                &apply_text,
                                false,
                                tokens,
                                if is_incompatible {
                                    "Disabled: Preset is incompatible with source media stream type"
                                } else {
                                    "Replace target preset on selected job(s)"
                                },
                            );
                            if apply_btn.clicked() {
                                for job_id in selected_job_ids {
                                    apply_preset_to_job(job_id, preset, queue_mgr);
                                }
                            }

                            let add_target_btn = icons::button_with_label(
                                ui,
                                icons::Icon::Plus,
                                &add_target_text,
                                false,
                                tokens,
                                if is_incompatible {
                                    "Disabled: Preset is incompatible with source media stream type"
                                } else {
                                    "Add this preset as an additional output target on selected job(s)"
                                },
                            );
                            if add_target_btn.clicked() {
                                for job_id in selected_job_ids {
                                    add_preset_target_to_job(job_id, preset, queue_mgr);
                                }
                            }
                        });
                    });
                });
        }
    });

    // Render preset editor modal dialog if active
    crate::preset_editor::render_preset_editor(ui.ctx(), &mut state.editor, registry, tokens);
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

fn container_to_extension(container: encodercraft_core::ContainerType) -> &'static str {
    match container {
        encodercraft_core::ContainerType::IsoBmffMp4 => "mp4",
        encodercraft_core::ContainerType::QuickTimeMov => "mov",
        encodercraft_core::ContainerType::MatroskaMkv => "mkv",
        encodercraft_core::ContainerType::WebM => "webm",
        encodercraft_core::ContainerType::Wave => "wav",
        encodercraft_core::ContainerType::Aiff => "aiff",
        encodercraft_core::ContainerType::Flac => "flac",
        encodercraft_core::ContainerType::OggAudio => "ogg",
        encodercraft_core::ContainerType::Mp3 => "mp3",
        encodercraft_core::ContainerType::AnimatedGif => "gif",
        encodercraft_core::ContainerType::Svg => "svg",
        encodercraft_core::ContainerType::Gltf => "glb",
        encodercraft_core::ContainerType::Usdz => "usdz",
        encodercraft_core::ContainerType::Obj => "obj",
        encodercraft_core::ContainerType::Stl => "stl",
        encodercraft_core::ContainerType::Ply => "ply",
        encodercraft_core::ContainerType::Pdf => "pdf",
        encodercraft_core::ContainerType::Docx => "docx",
        encodercraft_core::ContainerType::Idml => "idml",
        encodercraft_core::ContainerType::MxfOp1a | encodercraft_core::ContainerType::MxfOpAtom => "mxf",
        encodercraft_core::ContainerType::Png | encodercraft_core::ContainerType::StillSequence => "png",
        encodercraft_core::ContainerType::Jpeg => "jpg",
        encodercraft_core::ContainerType::WebP => "webp",
        encodercraft_core::ContainerType::Tiff => "tif",
        encodercraft_core::ContainerType::OpenExr => "exr",
        encodercraft_core::ContainerType::Bmp => "bmp",
    }
}

fn apply_preset_to_job(job_id: &str, preset: &EncodingPreset, queue_mgr: &Arc<QueueManager>) {
    if is_job_audio_only(job_id, queue_mgr) && preset.video.is_some() {
        log::warn!("Rejected applying video preset '{}' to audio-only job '{}'", preset.name, job_id);
        return;
    }

    let jobs = queue_mgr.list_jobs();
    if let Some(job) = jobs.iter().find(|j| j.spec.job_id == job_id) {
        let mut new_spec = job.spec.clone();
        if let Some(target) = new_spec.targets.first_mut() {
            target.preset_name = preset.name.clone();
            target.container = preset.container;
            let ext = container_to_extension(preset.container);
            target.output_path.set_extension(ext);

            if let Some(ref v) = preset.video {
                target.custom_width = match v.width {
                    encodercraft_presets::MatchSourceOr::Exact(w) => Some(w),
                    encodercraft_presets::MatchSourceOr::MatchSource => None,
                };
                target.custom_height = match v.height {
                    encodercraft_presets::MatchSourceOr::Exact(h) => Some(h),
                    encodercraft_presets::MatchSourceOr::MatchSource => None,
                };
                target.target_bitrate_kbps = Some(v.target_bitrate_kbps);
            } else {
                target.custom_width = None;
                target.custom_height = None;
                target.target_bitrate_kbps = None;
            }
            if let Some(ref a) = preset.audio {
                target.target_lufs = a.target_lufs.map(|x| x as f32);
            } else {
                target.target_lufs = None;
            }
        }
        // Update job
        let _ = queue_mgr.remove_job(job_id);
        let _ = queue_mgr.submit_job(new_spec);
    }
}

fn add_preset_target_to_job(job_id: &str, preset: &EncodingPreset, queue_mgr: &Arc<QueueManager>) {
    if is_job_audio_only(job_id, queue_mgr) && preset.video.is_some() {
        log::warn!("Rejected adding video preset target '{}' to audio-only job '{}'", preset.name, job_id);
        return;
    }

    let jobs = queue_mgr.list_jobs();
    if let Some(job) = jobs.iter().find(|j| j.spec.job_id == job_id) {
        let count = job.spec.targets.len() + 1;
        let ext = container_to_extension(preset.container);
        let (cw, ch, br) = if let Some(ref v) = preset.video {
            let w = match v.width {
                encodercraft_presets::MatchSourceOr::Exact(w) => Some(w),
                encodercraft_presets::MatchSourceOr::MatchSource => None,
            };
            let h = match v.height {
                encodercraft_presets::MatchSourceOr::Exact(h) => Some(h),
                encodercraft_presets::MatchSourceOr::MatchSource => None,
            };
            (w, h, Some(v.target_bitrate_kbps))
        } else {
            (None, None, None)
        };
        let lufs = preset.audio.as_ref().and_then(|a| a.target_lufs.map(|x| x as f32));

        let new_target = encodercraft_core::OutputTarget {
            target_id: format!("target-{count}"),
            output_path: std::path::PathBuf::from(format!("output_{count}.{ext}")),
            preset_name: preset.name.clone(),
            container: preset.container,
            custom_width: cw,
            custom_height: ch,
            custom_frame_rate: None,
            target_bitrate_kbps: br,
            target_lufs: lufs,
            ..Default::default()
        };
        let _ = queue_mgr.add_output_target(job_id, new_target);
    }
}
