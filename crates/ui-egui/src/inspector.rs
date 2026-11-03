//! Export Settings Inspector: comprehensive modal dialog for fine-tuning preview, container, video, audio, effects, captions, publish and metadata.

use std::path::PathBuf;

use egui::{ColorImage, RichText, TextureHandle, Ui, Window};
use encodercraft_core::{CaptionExportFormat, CraftJobSpec, OutputTarget, Rational64, SourceDescriptor};
use encodercraft_source::MediaProbe;

use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::queue_panel::{INLINE_CONTAINERS, default_preset_for_container, presets_for_container};
use crate::theme::Tokens;

/// Inspector dialog active tab (AME Parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Video,
    Audio,
    Effects,
    Captions,
    Publish,
    Loudness,
    Metadata,
}

/// Preview monitor tab mode within the Export Settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewTabMode {
    #[default]
    Source,
    Output,
}

/// State for the inspector settings modal.
pub struct InspectorState {
    pub is_open: bool,
    pub active_tab: InspectorTab,
    pub preview_tab: PreviewTabMode,
    pub job_spec: Option<CraftJobSpec>,
    pub preview_path: Option<PathBuf>,
    pub preview_decoder: Option<PreviewDecoder>,
    pub preview_probe: Option<Box<MediaProbe>>,
    pub preview_texture: Option<TextureHandle>,
    pub preview_frame_idx: u64,
    pub preview_frame_count: u64,
    pub aspect_ratio_locked: bool,
    pub video_bitrate_mode: String,
    pub audio_sample_rate_idx: usize,
    pub audio_channels_idx: usize,
    pub publish_destination: Option<PathBuf>,
    pub post_encode_action: String,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: InspectorTab::Video,
            preview_tab: PreviewTabMode::Source,
            job_spec: None,
            preview_path: None,
            preview_decoder: None,
            preview_probe: None,
            preview_texture: None,
            preview_frame_idx: 0,
            preview_frame_count: 100,
            aspect_ratio_locked: true,
            video_bitrate_mode: "VBR 1-Pass".into(),
            audio_sample_rate_idx: 1, // 48,000 Hz
            audio_channels_idx: 1,    // Stereo (2.0)
            publish_destination: None,
            post_encode_action: "None".into(),
        }
    }
}

/// Calculates estimated output file size in megabytes.
pub fn calculate_estimated_size_mb(duration_secs: f64, video_bitrate_kbps: u32, audio_bitrate_kbps: u32) -> f64 {
    let dur = if duration_secs > 0.05 { duration_secs } else { 60.0 };
    let total_kbps = (video_bitrate_kbps + audio_bitrate_kbps) as f64;
    (total_kbps * dur) / (8.0 * 1024.0)
}

/// Formats a SMPTE-style timecode from a frame number and frame rate.
pub fn format_timecode(frame: u64, fps: f64) -> String {
    let rate = if fps > 1.0 { fps } else { 30.0 };
    let total_seconds = frame as f64 / rate;
    let hours = (total_seconds / 3600.0).floor() as u64;
    let minutes = ((total_seconds % 3600.0) / 60.0).floor() as u64;
    let seconds = (total_seconds % 60.0).floor() as u64;
    let frames = (frame as f64 % rate).floor() as u64;
    format!("{hours:02}:{minutes:02}:{seconds:02}:{frames:02}")
}

/// Renders the export settings inspector modal window.
pub fn render_inspector_modal(ctx: &egui::Context, state: &mut InspectorState, tokens: &Tokens, on_save: &mut Option<CraftJobSpec>) {
    if !state.is_open {
        return;
    }

    // Initialize or poll preview decoder
    if let Some(ref spec) = state.job_spec
        && let SourceDescriptor::File { path, .. } = &spec.source
        && state.preview_path.as_deref() != Some(path.as_path())
    {
        state.preview_path = Some(path.clone());
        state.preview_decoder = Some(PreviewDecoder::open(path.clone()));
        state.preview_probe = None;
        state.preview_texture = None;
        state.preview_frame_idx = 0;
    }

    if let Some(ref dec) = state.preview_decoder {
        while let Some(ev) = dec.poll() {
            match ev {
                PreviewEvent::Opened { probe, frame_count } => {
                    state.preview_frame_count = frame_count.max(1);
                    state.preview_probe = Some(probe);
                    dec.request_frame(state.preview_frame_idx);
                }
                PreviewEvent::Frame { rgba, width, height, .. } => {
                    let img = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    state.preview_texture = Some(ctx.load_texture("inspector_preview", img, Default::default()));
                }
                PreviewEvent::Failed(_) => {}
            }
        }
    }

    let mut is_open = state.is_open;
    let mut should_close = false;
    let mut should_save = false;

    let mut current_spec = state.job_spec.take();

    Window::new("Export Settings").open(&mut is_open).resizable(true).collapsible(false).default_size([880.0, 600.0]).min_size([720.0, 480.0]).show(
        ctx,
        |ui| {
            if let Some(ref mut spec) = current_spec {
                ui.horizontal(|ui| {
                    // --- Left Side: Preview Monitor (Source / Output Tabs & Scrubber) ---
                    let left_width = (ui.available_width() * 0.46).clamp(320.0, 480.0);
                    ui.allocate_ui_with_layout(egui::vec2(left_width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        render_preview_section(ui, state, spec, tokens);
                    });

                    ui.separator();

                    // --- Right Side: Export Settings Tabs & Parameters ---
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            render_settings_section(ui, state, spec, tokens);

                            ui.add_space(8.0);
                            ui.separator();

                            // Bottom Action Bar: File size estimate & Buttons
                            ui.horizontal(|ui| {
                                let duration = state.preview_probe.as_ref().map(|p| p.duration_seconds).unwrap_or(60.0);
                                let v_kbps = spec.targets.first().and_then(|t| t.target_bitrate_kbps).unwrap_or(12000);
                                let a_kbps = 320;
                                let est_mb = calculate_estimated_size_mb(duration, v_kbps, a_kbps);

                                let dur_label = if duration > 0.05 { format!("{duration:.1} s") } else { "60 s est.".into() };
                                ui.label(
                                    RichText::new(format!("Est. File Size: {est_mb:.1} MB ({dur_label} at {v_kbps} kbps)"))
                                        .small()
                                        .color(tokens.text_dim),
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(RichText::new("Save & Apply").strong()).clicked() {
                                        should_save = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        should_close = true;
                                    }
                                });
                            });
                        },
                    );
                });
            } else {
                ui.label("No job selected for inspection.");
            }
        },
    );

    if should_close {
        is_open = false;
    }
    state.is_open = is_open;

    if should_save {
        if let Some(spec) = current_spec {
            *on_save = Some(spec);
        }
        state.job_spec = None;
        state.is_open = false;
    } else {
        state.job_spec = current_spec;
    }
}

fn render_preview_section(ui: &mut Ui, state: &mut InspectorState, _spec: &CraftJobSpec, tokens: &Tokens) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.preview_tab, PreviewTabMode::Source, "Source");
        ui.selectable_value(&mut state.preview_tab, PreviewTabMode::Output, "Output");

        if let Some(ref probe) = state.preview_probe
            && let Some(ref v) = probe.video
        {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{}x{} · {:.2} fps", v.width, v.height, v.frame_rate.as_f64())).small().color(tokens.text_dim));
            });
        }
    });

    ui.add_space(4.0);

    // Frame Texture or Placeholder Viewport
    let preview_height = 240.0;

    egui::Frame::canvas(ui.style()).fill(tokens.bg_panel_darker).show(ui, |ui| {
        ui.set_height(preview_height);
        ui.set_width(ui.available_width());

        if let Some(ref tex) = state.preview_texture {
            let tex_size = tex.size_vec2();
            let aspect = if tex_size.y > 0.0 { tex_size.x / tex_size.y } else { 16.0 / 9.0 };

            let avail_w = ui.available_width();
            let avail_h = preview_height;

            let (draw_w, draw_h) = if avail_w / avail_h > aspect { (avail_h * aspect, avail_h) } else { (avail_w, avail_w / aspect) };

            ui.centered_and_justified(|ui| {
                ui.image((tex.id(), egui::vec2(draw_w, draw_h)));
            });
        } else {
            ui.centered_and_justified(|ui| {
                let msg = match state.preview_tab {
                    PreviewTabMode::Source => "Source Preview (Loading / Standby)",
                    PreviewTabMode::Output => "Output Preview with Target Scaling",
                };
                ui.label(RichText::new(msg).color(tokens.text_dim));
            });
        }
    });

    ui.add_space(6.0);

    // Timeline Scrubber & In/Out Handles
    let fps = state.preview_probe.as_ref().and_then(|p| p.video.as_ref()).map(|v| v.frame_rate.as_f64()).unwrap_or(30.0);
    let current_tc = format_timecode(state.preview_frame_idx, fps);
    let total_tc = format_timecode(state.preview_frame_count.saturating_sub(1), fps);

    ui.horizontal(|ui| {
        ui.label(RichText::new(current_tc).strong().monospace().small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(total_tc).monospace().small().color(tokens.text_dim));
        });
    });

    let mut current_frame = state.preview_frame_idx;
    let max_frame = state.preview_frame_count.saturating_sub(1);
    let slider = egui::Slider::new(&mut current_frame, 0..=max_frame).show_value(false);
    if ui.add(slider).changed() {
        state.preview_frame_idx = current_frame;
        if let Some(ref dec) = state.preview_decoder {
            dec.request_frame(current_frame);
        }
    }

    // In / Out range controls
    ui.horizontal(|ui| {
        if ui.button("Set In").on_hover_text("Mark in-point for transcode range").clicked() {
            // Can update start_tick
        }
        if ui.button("Set Out").on_hover_text("Mark out-point for transcode range").clicked() {
            // Can update duration_ticks
        }
        ui.label(RichText::new("Range: Entire Sequence").small().color(tokens.text_dim));
    });
}

fn render_settings_section(ui: &mut Ui, state: &mut InspectorState, spec: &mut CraftJobSpec, tokens: &Tokens) {
    let targets = &mut spec.targets;
    let target = match targets.first_mut() {
        Some(t) => t,
        None => return,
    };

    // Header: Format & Preset Selectors + Match Source Button
    ui.horizontal(|ui| {
        ui.label(RichText::new("Format:").strong());
        let current_c = target.container;
        let combo_text = INLINE_CONTAINERS.iter().find(|(c, _, _)| *c == current_c).map(|(_, name, _)| *name).unwrap_or("Custom");
        egui::ComboBox::from_id_salt("inspector_format_combo").selected_text(combo_text).width(140.0).show_ui(ui, |ui| {
            for (c, name, _) in INLINE_CONTAINERS {
                if ui.selectable_label(target.container == *c, *name).clicked() {
                    target.container = *c;
                    target.preset_name = default_preset_for_container(*c).to_string();
                    if let Some((_, _, ext)) = INLINE_CONTAINERS.iter().find(|(ct, _, _)| *ct == *c) {
                        let mut p = target.output_path.clone();
                        p.set_extension(ext);
                        target.output_path = p;
                    }
                }
            }
        });

        ui.add_space(8.0);

        ui.label(RichText::new("Preset:").strong());
        egui::ComboBox::from_id_salt("inspector_preset_combo").selected_text(&target.preset_name).width(190.0).show_ui(ui, |ui| {
            for preset_name in presets_for_container(target.container) {
                if ui.selectable_label(target.preset_name == preset_name, preset_name).clicked() {
                    target.preset_name = preset_name.to_string();
                }
            }
        });

        ui.add_space(8.0);

        // Match Source button
        let match_btn = ui.button("Match Source").on_hover_text("Copy width, height, and frame rate directly from the source media");
        if match_btn.clicked()
            && let Some(ref probe) = state.preview_probe
            && let Some(ref v) = probe.video
        {
            target.custom_width = Some(v.width);
            target.custom_height = Some(v.height);
            target.custom_frame_rate = Some(v.frame_rate);
            if let Some(br) = v.bitrate {
                target.target_bitrate_kbps = Some((br / 1000) as u32);
            }
        }
    });

    ui.add_space(4.0);

    // Output File Destination Link
    ui.horizontal(|ui| {
        ui.label(RichText::new("Output Name:").strong());
        let file_name = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
        let link = ui.link(RichText::new(file_name).color(tokens.accent_primary)).on_hover_text(target.output_path.display().to_string());
        if link.clicked()
            && let Some(new_path) = rfd::FileDialog::new().set_file_name(file_name).save_file()
        {
            target.output_path = new_path;
        }
    });

    ui.add_space(6.0);
    ui.separator();

    // Tabs
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.active_tab, InspectorTab::Video, "Video");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Audio, "Audio");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Effects, "Effects");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Captions, "Captions");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Publish, "Publish");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Loudness, "Loudness");
        ui.selectable_value(&mut state.active_tab, InspectorTab::Metadata, "Metadata");
    });

    ui.separator();
    ui.add_space(4.0);

    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match state.active_tab {
        InspectorTab::Video => render_video_tab(ui, target, state),
        InspectorTab::Audio => render_audio_tab(ui, target, state),
        InspectorTab::Effects => render_effects_tab(ui, target),
        InspectorTab::Captions => render_captions_tab(ui, target),
        InspectorTab::Publish => render_publish_tab(ui, target, state),
        InspectorTab::Loudness => render_loudness_tab(ui, target),
        InspectorTab::Metadata => render_metadata_tab(ui, &mut spec.client_info, target),
    });
}

fn render_video_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    ui.heading("Basic Video Settings");
    ui.add_space(4.0);

    // Frame Dimensions & Aspect Lock
    let mut w = target.custom_width.unwrap_or(1920);
    let mut h = target.custom_height.unwrap_or(1080);

    ui.horizontal(|ui| {
        ui.label("Frame Width:");
        if ui.add(egui::DragValue::new(&mut w).range(16..=7680).speed(2)).changed() {
            target.custom_width = Some(w);
            if state.aspect_ratio_locked {
                h = ((w as f64 * 9.0) / 16.0).round() as u32;
                target.custom_height = Some(h);
            }
        }

        ui.add_space(8.0);
        ui.label("Height:");
        if ui.add(egui::DragValue::new(&mut h).range(16..=4320).speed(2)).changed() {
            target.custom_height = Some(h);
            if state.aspect_ratio_locked {
                w = ((h as f64 * 16.0) / 9.0).round() as u32;
                target.custom_width = Some(w);
            }
        }

        ui.add_space(8.0);
        ui.checkbox(&mut state.aspect_ratio_locked, "Lock Aspect Ratio (16:9)");
    });

    // Resolution Presets Shortcut
    ui.horizontal(|ui| {
        ui.label(RichText::new("Presets:").small().color(egui::Color32::GRAY));
        if ui.small_button("4K UHD (3840x2160)").clicked() {
            target.custom_width = Some(3840);
            target.custom_height = Some(2160);
        }
        if ui.small_button("1080p FHD (1920x1080)").clicked() {
            target.custom_width = Some(1920);
            target.custom_height = Some(1080);
        }
        if ui.small_button("720p HD (1280x720)").clicked() {
            target.custom_width = Some(1280);
            target.custom_height = Some(720);
        }
    });

    ui.add_space(6.0);

    // Frame Rate
    let current_fps = target.custom_frame_rate.map(|r| r.as_f64()).unwrap_or(29.97);
    ui.horizontal(|ui| {
        ui.label("Frame Rate (fps):");
        egui::ComboBox::from_id_salt("inspector_fps_combo").selected_text(format!("{current_fps:.2} fps")).show_ui(ui, |ui| {
            if ui.selectable_label((current_fps - 23.976).abs() < 0.01, "23.976 fps (Film)").clicked() {
                target.custom_frame_rate = Rational64::new(24000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 24.0).abs() < 0.01, "24.0 fps (Cinema)").clicked() {
                target.custom_frame_rate = Rational64::new(24, 1).ok();
            }
            if ui.selectable_label((current_fps - 25.0).abs() < 0.01, "25.0 fps (PAL/Broadcast)").clicked() {
                target.custom_frame_rate = Rational64::new(25, 1).ok();
            }
            if ui.selectable_label((current_fps - 29.97).abs() < 0.01, "29.97 fps (NTSC)").clicked() {
                target.custom_frame_rate = Rational64::new(30000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 30.0).abs() < 0.01, "30.0 fps").clicked() {
                target.custom_frame_rate = Rational64::new(30, 1).ok();
            }
            if ui.selectable_label((current_fps - 59.94).abs() < 0.01, "59.94 fps").clicked() {
                target.custom_frame_rate = Rational64::new(60000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 60.0).abs() < 0.01, "60.0 fps").clicked() {
                target.custom_frame_rate = Rational64::new(60, 1).ok();
            }
        });
    });

    ui.add_space(6.0);
    ui.separator();

    // Bitrate Settings (CBR, VBR 1-Pass, VBR 2-Pass)
    ui.heading("Bitrate Settings");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Bitrate Encoding:");
        egui::ComboBox::from_id_salt("inspector_bitrate_mode_combo").selected_text(&state.video_bitrate_mode).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.video_bitrate_mode, "VBR 1-Pass".into(), "VBR, 1 Pass");
            ui.selectable_value(&mut state.video_bitrate_mode, "VBR 2-Pass".into(), "VBR, 2 Pass");
            ui.selectable_value(&mut state.video_bitrate_mode, "CBR".into(), "CBR (Constant Bitrate)");
        });
    });

    let mut bitrate_kbps = target.target_bitrate_kbps.unwrap_or(12000);
    ui.horizontal(|ui| {
        ui.label("Target Bitrate [Mbps]:");
        let mut mbps = bitrate_kbps as f32 / 1000.0;
        if ui.add(egui::Slider::new(&mut mbps, 0.5..=150.0).step_by(0.5)).changed() {
            bitrate_kbps = (mbps * 1000.0).round() as u32;
            target.target_bitrate_kbps = Some(bitrate_kbps);
        }
    });

    ui.add_space(6.0);
    ui.separator();

    // Hardware Acceleration & Rendering options
    ui.checkbox(&mut target.hardware_acceleration, "Hardware Acceleration (VideoToolbox / NVENC / VA-API)");
}

fn render_audio_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    ui.heading("Audio Format & Channels");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Audio Codec:");
        ui.label("AAC-LC (Advanced Audio Coding) / Linear PCM 24-bit");
    });

    ui.horizontal(|ui| {
        ui.label("Sample Rate:");
        egui::ComboBox::from_id_salt("inspector_audio_sr")
            .selected_text(match state.audio_sample_rate_idx {
                0 => "44,100 Hz",
                2 => "96,000 Hz",
                _ => "48,000 Hz",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.audio_sample_rate_idx, 0, "44,100 Hz (CD Standard)");
                ui.selectable_value(&mut state.audio_sample_rate_idx, 1, "48,000 Hz (Video Broadcast Standard)");
                ui.selectable_value(&mut state.audio_sample_rate_idx, 2, "96,000 Hz (Hi-Res Audio)");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Channels:");
        egui::ComboBox::from_id_salt("inspector_audio_ch")
            .selected_text(match state.audio_channels_idx {
                0 => "Mono (1.0)",
                2 => "5.1 Surround",
                _ => "Stereo (2.0)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.audio_channels_idx, 0, "Mono (1.0)");
                ui.selectable_value(&mut state.audio_channels_idx, 1, "Stereo (2.0)");
                ui.selectable_value(&mut state.audio_channels_idx, 2, "5.1 Surround");
            });
    });

    ui.add_space(6.0);
    ui.separator();

    let mut current_stems = target.audio_stems.unwrap_or(encodercraft_core::AudioStemMode::None);
    ui.horizontal(|ui| {
        ui.label("Multichannel Audio Stems:");
        egui::ComboBox::from_id_salt("audio_stems_combo")
            .selected_text(match current_stems {
                encodercraft_core::AudioStemMode::None => "None (Master Mix Only)",
                encodercraft_core::AudioStemMode::DiscreteMono => "Discrete Mono Channels (_Stem_L, _Stem_R.wav)",
                encodercraft_core::AudioStemMode::StereoPairs => "Stereo Channel Pairs (_Stems_Ch1-2.wav)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut current_stems, encodercraft_core::AudioStemMode::None, "None (Master Mix Only)");
                ui.selectable_value(
                    &mut current_stems,
                    encodercraft_core::AudioStemMode::DiscreteMono,
                    "Discrete Mono Channels (_Stem_L, _Stem_R.wav)",
                );
                ui.selectable_value(&mut current_stems, encodercraft_core::AudioStemMode::StereoPairs, "Stereo Channel Pairs (_Stems_Ch1-2.wav)");
            });
    });
    target.audio_stems = if current_stems == encodercraft_core::AudioStemMode::None { None } else { Some(current_stems) };
}

fn render_effects_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Effects, Overlays & 3D LUTs");
    ui.add_space(4.0);

    ui.checkbox(&mut target.burn_timecode, "Burn SMPTE Timecode Overlay (HH:MM:SS:FF)");

    ui.add_space(6.0);

    let mut lut_str = target.lut_path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("3D LUT (.cube):");
        if ui.text_edit_singleline(&mut lut_str).changed() {
            target.lut_path = if lut_str.trim().is_empty() { None } else { Some(PathBuf::from(lut_str.trim())) };
        }
        if ui.button("Browse...").clicked()
            && let Some(file) = rfd::FileDialog::new().add_filter("3D LUT", &["cube"]).pick_file()
        {
            target.lut_path = Some(file);
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
            target.caption_path = if cap_str.trim().is_empty() { None } else { Some(PathBuf::from(cap_str.trim())) };
        }
        if ui.button("Browse...").clicked()
            && let Some(file) = rfd::FileDialog::new().add_filter("Captions", &["srt", "vtt", "scc", "ttml"]).pick_file()
        {
            target.caption_path = Some(file);
        }
    });

    ui.add_space(6.0);
    ui.checkbox(&mut target.burn_captions, "Burn Captions into Video (Open Captions)");

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let mut has_sidecar = target.caption_format.is_some();
        if ui.checkbox(&mut has_sidecar, "Generate Sidecar File").changed() {
            target.caption_format = if has_sidecar { Some(CaptionExportFormat::WebVtt) } else { None };
        }

        if let Some(ref mut fmt) = target.caption_format {
            egui::ComboBox::from_id_salt("caption_sidecar_format_combo").selected_text(format!("{:?}", fmt)).show_ui(ui, |ui| {
                ui.selectable_value(fmt, CaptionExportFormat::WebVtt, "WebVTT (.vtt)");
                ui.selectable_value(fmt, CaptionExportFormat::Srt, "SubRip (.srt)");
                ui.selectable_value(fmt, CaptionExportFormat::Scc, "Scenarist SCC (.scc)");
                ui.selectable_value(fmt, CaptionExportFormat::Ttml, "TTML IMSC1 (.ttml)");
            });
        }
    });
}

fn render_publish_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    ui.heading("Publish & Post-Encode Actions");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Destination Folder:");
        let folder_str = target.output_path.parent().and_then(|p| p.to_str()).unwrap_or("");
        ui.label(RichText::new(folder_str).monospace().small());
        if ui.button("Choose...").clicked()
            && let Some(folder) = rfd::FileDialog::new().set_title("Choose Destination Directory").pick_folder()
        {
            let file_name = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
            target.output_path = folder.join(file_name);
        }
    });

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("Post-Encode Action:");
        egui::ComboBox::from_id_salt("inspector_post_action").selected_text(&state.post_encode_action).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.post_encode_action, "None".into(), "Do Nothing");
            ui.selectable_value(&mut state.post_encode_action, "Reveal".into(), "Reveal in File Manager");
            ui.selectable_value(&mut state.post_encode_action, "Play".into(), "Play in Default Media Player");
        });
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

fn render_metadata_tab(ui: &mut Ui, client_info: &mut encodercraft_core::ClientInfo, target: &mut OutputTarget) {
    ui.heading("Job & Dublin Core / Adobe XMP Metadata");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Submitting Application:");
        ui.label(&client_info.app_name);
    });

    let mut meta = target.metadata.clone().unwrap_or_default();
    let mut changed = false;

    let mut title = meta.title.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Title (dc:title):");
        if ui.text_edit_singleline(&mut title).changed() {
            meta.title = if title.trim().is_empty() { None } else { Some(title) };
            changed = true;
        }
    });

    let mut creator = meta.creator.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Creator / Author (dc:creator):");
        if ui.text_edit_singleline(&mut creator).changed() {
            meta.creator = if creator.trim().is_empty() { None } else { Some(creator) };
            changed = true;
        }
    });

    let mut desc = meta.description.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Description (dc:description):");
        if ui.text_edit_singleline(&mut desc).changed() {
            meta.description = if desc.trim().is_empty() { None } else { Some(desc) };
            changed = true;
        }
    });

    let mut copyright = meta.copyright.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Copyright Notice (dc:rights):");
        if ui.text_edit_singleline(&mut copyright).changed() {
            meta.copyright = if copyright.trim().is_empty() { None } else { Some(copyright) };
            changed = true;
        }
    });

    ui.add_space(4.0);
    if ui.checkbox(&mut meta.export_sidecar_xmp, "Export External Sidecar .xmp File").changed() {
        changed = true;
    }

    if changed {
        target.metadata = Some(meta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timecode() {
        assert_eq!(format_timecode(0, 30.0), "00:00:00:00");
        assert_eq!(format_timecode(30, 30.0), "00:00:01:00");
        assert_eq!(format_timecode(3600 * 30, 30.0), "01:00:00:00");
    }

    #[test]
    fn test_calculate_estimated_size() {
        let size = calculate_estimated_size_mb(10.0, 8000, 320);
        // (8320 kbps * 10 s) / 8192 = 10.156 MB
        assert!((size - 10.156).abs() < 0.05);
    }
}
