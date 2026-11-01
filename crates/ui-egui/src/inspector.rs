//! Export Settings Inspector: detailed modal dialog for fine-tuning container, video, audio, and loudness.

use egui::{RichText, Ui, Window};
use encodercraft_core::{ContainerType, CraftJobSpec, OutputTarget};

use crate::theme::Tokens;

/// Inspector dialog active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Video,
    Audio,
    Effects,
    Captions,
    Loudness,
    Metadata,
}

/// State for the inspector settings modal.
pub struct InspectorState {
    pub is_open: bool,
    pub active_tab: InspectorTab,
    pub job_spec: Option<CraftJobSpec>,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self { is_open: false, active_tab: InspectorTab::Video, job_spec: None }
    }
}

/// Renders the export settings inspector modal window.
pub fn render_inspector_modal(ctx: &egui::Context, state: &mut InspectorState, tokens: &Tokens, on_save: &mut Option<CraftJobSpec>) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;
    let mut should_close = false;
    let mut should_save = false;

    Window::new("Export Settings").open(&mut is_open).resizable(true).collapsible(false).default_size([520.0, 420.0]).show(ctx, |ui| {
        if let Some(ref mut spec) = state.job_spec {
            render_inspector_content(ui, spec, &mut state.active_tab, tokens);

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Save & Apply").clicked() {
                    should_save = true;
                }

                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
            });
        } else {
            ui.label("No job selected for inspection.");
        }
    });

    if should_close {
        is_open = false;
    }
    state.is_open = is_open;

    if should_save {
        if let Some(spec) = state.job_spec.take() {
            *on_save = Some(spec);
        }
        state.is_open = false;
    }
}

fn render_inspector_content(ui: &mut Ui, spec: &mut CraftJobSpec, active_tab: &mut InspectorTab, tokens: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Job ID:").strong());
        ui.label(RichText::new(&spec.job_id).color(tokens.accent_primary));
    });

    ui.add_space(4.0);

    // Target Selection / Container
    if let Some(target) = spec.targets.first_mut() {
        ui.horizontal(|ui| {
            ui.label("Container Format:");
            egui::ComboBox::from_id_salt("container_combo").selected_text(format!("{:?}", target.container)).show_ui(ui, |ui| {
                ui.selectable_value(&mut target.container, ContainerType::IsoBmffMp4, "MP4 (MPEG-4 Part 14)");
                ui.selectable_value(&mut target.container, ContainerType::QuickTimeMov, "MOV (QuickTime Movie)");
                ui.selectable_value(&mut target.container, ContainerType::MatroskaMkv, "MKV (Matroska)");
                ui.selectable_value(&mut target.container, ContainerType::WebM, "WebM");
                ui.selectable_value(&mut target.container, ContainerType::Wave, "WAV (Broadcast Wave Format)");
                ui.selectable_value(&mut target.container, ContainerType::Gltf, "glTF 2.0 / GLB");
            });
        });

        ui.add_space(4.0);
        ui.separator();

        // Tabs
        ui.horizontal(|ui| {
            ui.selectable_value(active_tab, InspectorTab::Video, "Video");
            ui.selectable_value(active_tab, InspectorTab::Audio, "Audio");
            ui.selectable_value(active_tab, InspectorTab::Effects, "Effects & Overlays");
            ui.selectable_value(active_tab, InspectorTab::Captions, "Captions");
            ui.selectable_value(active_tab, InspectorTab::Loudness, "Loudness & Mastering");
            ui.selectable_value(active_tab, InspectorTab::Metadata, "Metadata");
        });

        ui.separator();

        match active_tab {
            InspectorTab::Video => render_video_tab(ui, target),
            InspectorTab::Audio => render_audio_tab(ui, target),
            InspectorTab::Effects => render_effects_tab(ui, target),
            InspectorTab::Captions => render_captions_tab(ui, target),
            InspectorTab::Loudness => render_loudness_tab(ui, target),
            InspectorTab::Metadata => render_metadata_tab(ui, spec),
        }
    }
}

fn render_effects_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Effects, Overlays & 3D LUTs");
    ui.add_space(4.0);

    ui.checkbox(&mut target.burn_timecode, "Burn SMPTE Timecode (HH:MM:SS:FF)");

    ui.add_space(6.0);

    let mut lut_str = target.lut_path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("3D LUT (.cube):");
        if ui.text_edit_singleline(&mut lut_str).changed() {
            if lut_str.trim().is_empty() {
                target.lut_path = None;
            } else {
                target.lut_path = Some(std::path::PathBuf::from(lut_str.trim()));
            }
        }
    });
}

fn render_captions_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Closed Captions & Subtitles");
    ui.add_space(4.0);

    let mut cap_str = target.caption_path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Caption File (.srt, .vtt, .scc, .ttml):");
        if ui.text_edit_singleline(&mut cap_str).changed() {
            if cap_str.trim().is_empty() {
                target.caption_path = None;
            } else {
                target.caption_path = Some(std::path::PathBuf::from(cap_str.trim()));
            }
        }
    });

    ui.add_space(6.0);

    ui.checkbox(&mut target.burn_captions, "Burn Captions into Video (Open Captions)");

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let mut has_sidecar = target.caption_format.is_some();
        if ui.checkbox(&mut has_sidecar, "Generate Sidecar File").changed() {
            if has_sidecar {
                target.caption_format = Some(encodercraft_core::CaptionExportFormat::WebVtt);
            } else {
                target.caption_format = None;
            }
        }

        if let Some(ref mut fmt) = target.caption_format {
            egui::ComboBox::from_id_salt("caption_sidecar_format_combo").selected_text(format!("{:?}", fmt)).show_ui(ui, |ui| {
                ui.selectable_value(fmt, encodercraft_core::CaptionExportFormat::WebVtt, "WebVTT (.vtt)");
                ui.selectable_value(fmt, encodercraft_core::CaptionExportFormat::Srt, "SubRip (.srt)");
                ui.selectable_value(fmt, encodercraft_core::CaptionExportFormat::Scc, "Scenarist SCC (.scc)");
                ui.selectable_value(fmt, encodercraft_core::CaptionExportFormat::Ttml, "TTML IMSC1 (.ttml)");
            });
        }
    });

    ui.add_space(6.0);

    let mut offset = target.caption_offset_ms.unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label("Timecode Offset (ms):");
        if ui.add(egui::DragValue::new(&mut offset).range(-60000..=60000).speed(50)).changed() {
            target.caption_offset_ms = if offset == 0 { None } else { Some(offset) };
        }
    });
}

fn render_video_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Video Encoding Parameters");
    ui.add_space(4.0);

    let mut w = target.custom_width.unwrap_or(3840);
    ui.horizontal(|ui| {
        ui.label("Width (px):");
        if ui.add(egui::DragValue::new(&mut w).range(16..=7680)).changed() {
            target.custom_width = Some(w);
        }
    });

    let mut h = target.custom_height.unwrap_or(2160);
    ui.horizontal(|ui| {
        ui.label("Height (px):");
        if ui.add(egui::DragValue::new(&mut h).range(16..=4320)).changed() {
            target.custom_height = Some(h);
        }
    });

    let mut bitrate = target.target_bitrate_kbps.unwrap_or(45000);
    ui.horizontal(|ui| {
        ui.label("Target Bitrate (kbps):");
        if ui.add(egui::DragValue::new(&mut bitrate).range(500..=200000)).changed() {
            target.target_bitrate_kbps = Some(bitrate);
        }
    });
}

fn render_audio_tab(ui: &mut Ui, _target: &mut OutputTarget) {
    ui.heading("Audio Encoding Parameters");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Audio Codec:");
        ui.label("AAC-LC (Stereo, 320 kbps)");
    });

    ui.horizontal(|ui| {
        ui.label("Sample Rate:");
        ui.label("48,000 Hz");
    });
}

fn render_loudness_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("EBU R128 / ITU-R BS.1770 Loudness Normalization");
    ui.add_space(4.0);

    let mut lufs = target.target_lufs.unwrap_or(-23.0);
    ui.horizontal(|ui| {
        ui.label("Integrated Target (LUFS):");
        if ui.add(egui::DragValue::new(&mut lufs).range(-36.0..=-6.0).speed(0.5)).changed() {
            target.target_lufs = Some(lufs);
        }
    });

    ui.horizontal(|ui| {
        ui.label("True Peak Ceiling:");
        ui.label("-1.0 dBFS (4x Oversampled Brickwall)");
    });
}

fn render_metadata_tab(ui: &mut Ui, spec: &mut CraftJobSpec) {
    ui.heading("Job & Client Metadata");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Submitting Application:");
        ui.label(&spec.client_info.app_name);
    });

    let mut notes = spec.client_info.user_notes.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("User Notes:");
        if ui.text_edit_singleline(&mut notes).changed() {
            spec.client_info.user_notes = Some(notes);
        }
    });
}
