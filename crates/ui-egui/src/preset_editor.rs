//! Custom preset editor modal dialog.
//!
//! Provides an interactive dialog for creating, configuring, and saving custom
//! encoding presets with full control over container formats, video codecs,
//! resolutions, bitrates, audio channels, and loudness targets. Parity with
//! Adobe Media Encoder's "Preset > Create Encoding Preset..." dialog.

use std::sync::Arc;

use egui::{Context, RichText, Window};
use encodercraft_color::PixelFormat;
use encodercraft_core::{ChannelLayout, ContainerType, Rational64};
use encodercraft_engine::{QueueManager, cmd::CommandRegistry};
use encodercraft_geom::ScaleMode;
use encodercraft_presets::{EncodingPreset, MatchSourceOr, PresetAudioSettings, PresetCategory, PresetVideoSettings};

use crate::theme::Tokens;

/// UI state for the preset editor modal window.
#[derive(Debug, Clone)]
pub struct PresetEditorState {
    pub is_open: bool,
    pub name: String,
    pub description: String,
    pub category: PresetCategory,
    pub container: ContainerType,
    pub include_video: bool,
    pub video_codec: String,
    pub width: u32,
    pub height: u32,
    pub match_source_dims: bool,
    pub fps_num: i64,
    pub fps_den: i64,
    pub match_source_fps: bool,
    pub bitrate_kbps: u32,
    pub include_audio: bool,
    pub audio_codec: String,
    pub sample_rate: u32,
    pub match_source_sample_rate: bool,
    pub channel_layout: ChannelLayout,
    pub target_lufs: Option<f64>,
    pub error_message: Option<String>,
}

impl Default for PresetEditorState {
    fn default() -> Self {
        Self {
            is_open: false,
            name: "My Custom Preset".into(),
            description: "Custom user-defined export preset".into(),
            category: PresetCategory::Custom,
            container: ContainerType::IsoBmffMp4,
            include_video: true,
            video_codec: "H.264".into(),
            width: 1920,
            height: 1080,
            match_source_dims: true,
            fps_num: 24,
            fps_den: 1,
            match_source_fps: true,
            bitrate_kbps: 10_000,
            include_audio: true,
            audio_codec: "AAC-LC".into(),
            sample_rate: 48_000,
            match_source_sample_rate: true,
            channel_layout: ChannelLayout::Stereo,
            target_lufs: Some(-14.0),
            error_message: None,
        }
    }
}

impl PresetEditorState {
    /// Opens the dialog with fresh default values.
    pub fn open_new(&mut self) {
        *self = Self { is_open: true, ..Default::default() };
    }

    /// Validates and saves the configured preset through the shared engine command.
    pub fn save(&mut self, commands: &CommandRegistry, queue_mgr: &Arc<QueueManager>) -> bool {
        let trimmed_name = self.name.trim();
        if trimmed_name.is_empty() {
            self.error_message = Some("Preset name cannot be empty".into());
            return false;
        }

        let video = if self.include_video {
            let width = if self.match_source_dims { MatchSourceOr::MatchSource } else { MatchSourceOr::Exact(self.width) };
            let height = if self.match_source_dims { MatchSourceOr::MatchSource } else { MatchSourceOr::Exact(self.height) };
            let frame_rate = if self.match_source_fps {
                MatchSourceOr::MatchSource
            } else {
                let den = if self.fps_den == 0 { 1 } else { self.fps_den };
                MatchSourceOr::Exact(Rational64 { num: self.fps_num, den })
            };

            Some(PresetVideoSettings {
                codec: self.video_codec.clone(),
                width,
                height,
                frame_rate,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv420p),
                target_bitrate_kbps: self.bitrate_kbps,
                keyframe_interval: 48,
                profile: None,
                bframes: None,
                rate_mode: Default::default(),
                color: None,
                match_source_bitrate: false,
            })
        } else {
            None
        };

        let audio = if self.include_audio {
            let sample_rate = if self.match_source_sample_rate { MatchSourceOr::MatchSource } else { MatchSourceOr::Exact(self.sample_rate) };

            Some(PresetAudioSettings {
                codec: self.audio_codec.clone(),
                sample_rate,
                layout: MatchSourceOr::Exact(self.channel_layout),
                bitrate_kbps: Some(320),
                target_lufs: self.target_lufs,
            })
        } else {
            None
        };

        let preset = EncodingPreset {
            name: trimmed_name.to_string(),
            description: self.description.clone(),
            category: self.category,
            container: self.container,
            video,
            audio,
        };

        match commands.dispatch(queue_mgr, "preset.create", serde_json::json!({ "preset": preset })) {
            Ok(_) => {
                self.is_open = false;
                self.error_message = None;
                true
            }
            Err(error) => {
                self.error_message = Some(format!("Failed to save preset: {error}"));
                false
            }
        }
    }
}

/// Renders the preset editor window if open.
pub fn render_preset_editor(
    ctx: &Context,
    state: &mut PresetEditorState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;
    Window::new(RichText::new("Preset Settings - Create Encoding Preset").strong()).open(&mut is_open).default_width(520.0).resizable(false).show(
        ctx,
        |ui| {
            ui.vertical(|ui| {
                if let Some(ref err) = state.error_message {
                    ui.colored_label(tokens.status_failed, err);
                    ui.add_space(4.0);
                }

                // General metadata
                ui.group(|ui| {
                    ui.heading(RichText::new("Preset Details").size(14.0));
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut state.name);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        ui.text_edit_singleline(&mut state.description);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Format Container:");
                        egui::ComboBox::from_id_salt("editor_container_box").selected_text(format!("{:?}", state.container)).show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.container, ContainerType::IsoBmffMp4, "MP4 (H.264/HEVC/AV1)");
                            ui.selectable_value(&mut state.container, ContainerType::QuickTimeMov, "QuickTime MOV (ProRes/DNxHR)");
                            ui.selectable_value(&mut state.container, ContainerType::MatroskaMkv, "Matroska MKV");
                            ui.selectable_value(&mut state.container, ContainerType::WebM, "WebM (VP9/AV1/Opus)");
                            ui.selectable_value(&mut state.container, ContainerType::MxfOp1a, "MXF OP1a (SMPTE Broadcast)");
                            ui.selectable_value(&mut state.container, ContainerType::MxfOpAtom, "MXF OP-Atom (Avid NLE)");
                            ui.separator();
                            ui.selectable_value(&mut state.container, ContainerType::Wave, "Broadcast Wave (WAV)");
                            ui.selectable_value(&mut state.container, ContainerType::Flac, "FLAC Lossless Audio");
                            ui.selectable_value(&mut state.container, ContainerType::Aiff, "AIFF Audio");
                            ui.selectable_value(&mut state.container, ContainerType::OggAudio, "Ogg Opus Audio");
                            ui.selectable_value(&mut state.container, ContainerType::Mp3, "MP3 Audio");
                            ui.separator();
                            ui.selectable_value(&mut state.container, ContainerType::Png, "PNG Image");
                            ui.selectable_value(&mut state.container, ContainerType::Jpeg, "JPEG Image");
                            ui.selectable_value(&mut state.container, ContainerType::WebP, "WebP Image");
                            ui.selectable_value(&mut state.container, ContainerType::Tiff, "TIFF 16-bit Image");
                            ui.selectable_value(&mut state.container, ContainerType::OpenExr, "OpenEXR HDR Image");
                            ui.selectable_value(&mut state.container, ContainerType::Bmp, "BMP Image");
                            ui.selectable_value(&mut state.container, ContainerType::StillSequence, "Still Image Sequence");
                            ui.selectable_value(&mut state.container, ContainerType::AnimatedGif, "Animated GIF");
                            ui.separator();
                            ui.selectable_value(&mut state.container, ContainerType::Svg, "SVG Vector");
                            ui.selectable_value(&mut state.container, ContainerType::Pdf, "PDF Document");
                            ui.selectable_value(&mut state.container, ContainerType::Docx, "DOCX Document");
                            ui.selectable_value(&mut state.container, ContainerType::Idml, "IDML Package");
                            ui.separator();
                            ui.selectable_value(&mut state.container, ContainerType::Gltf, "glTF 2.0 / GLB 3D");
                            ui.selectable_value(&mut state.container, ContainerType::Usdz, "USDZ 3D Scene");
                            ui.selectable_value(&mut state.container, ContainerType::Obj, "Wavefront OBJ 3D");
                            ui.selectable_value(&mut state.container, ContainerType::Stl, "Binary STL 3D");
                            ui.selectable_value(&mut state.container, ContainerType::Ply, "Stanford PLY 3D");
                        });
                    });
                });

                ui.add_space(6.0);

                // Video Settings
                ui.group(|ui| {
                    ui.checkbox(&mut state.include_video, RichText::new("Export Video").strong());
                    if state.include_video {
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Video Codec:");
                            egui::ComboBox::from_id_salt("editor_vcodec_box").selected_text(&state.video_codec).show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.video_codec, "H.264".into(), "H.264 (AVC)");
                                ui.selectable_value(&mut state.video_codec, "HEVC".into(), "HEVC (H.265)");
                                ui.selectable_value(&mut state.video_codec, "ProRes".into(), "Apple ProRes 422 / 4444");
                                ui.selectable_value(&mut state.video_codec, "DNxHR".into(), "Avid DNxHR (SMPTE VC-3)");
                                ui.selectable_value(&mut state.video_codec, "AV1".into(), "AOMedia AV1");
                                ui.selectable_value(&mut state.video_codec, "VP9".into(), "Google VP9");
                                ui.selectable_value(&mut state.video_codec, "MJPEG".into(), "Motion JPEG");
                            });
                        });

                        ui.checkbox(&mut state.match_source_dims, "Match Source Resolution");
                        if !state.match_source_dims {
                            ui.horizontal(|ui| {
                                ui.label("Width:");
                                ui.add(egui::DragValue::new(&mut state.width).range(160..=7680));
                                ui.label("Height:");
                                ui.add(egui::DragValue::new(&mut state.height).range(120..=4320));
                            });
                        }

                        ui.checkbox(&mut state.match_source_fps, "Match Source Frame Rate");
                        if !state.match_source_fps {
                            ui.horizontal(|ui| {
                                ui.label("FPS:");
                                egui::ComboBox::from_id_salt("editor_fps_box").selected_text(format!("{}/{}", state.fps_num, state.fps_den)).show_ui(
                                    ui,
                                    |ui| {
                                        ui.selectable_value(&mut state.fps_num, 24, "24 fps");
                                        ui.selectable_value(&mut state.fps_num, 25, "25 fps (PAL)");
                                        ui.selectable_value(&mut state.fps_num, 30, "30 fps");
                                        ui.selectable_value(&mut state.fps_num, 60, "60 fps");
                                    },
                                );
                            });
                        }

                        ui.horizontal(|ui| {
                            ui.label("Target Bitrate (kbps):");
                            ui.add(egui::DragValue::new(&mut state.bitrate_kbps).range(500..=100_000));
                        });
                    }
                });

                ui.add_space(6.0);

                // Audio Settings
                ui.group(|ui| {
                    ui.checkbox(&mut state.include_audio, RichText::new("Export Audio").strong());
                    if state.include_audio {
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label("Audio Codec:");
                            egui::ComboBox::from_id_salt("editor_acodec_box").selected_text(&state.audio_codec).show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.audio_codec, "AAC-LC".into(), "AAC-LC (FilmCraft)");
                                ui.selectable_value(&mut state.audio_codec, "Opus".into(), "Opus Interactive Audio (IETF RFC 6716)");
                                ui.selectable_value(&mut state.audio_codec, "PCM".into(), "Uncompressed PCM 24-bit");
                                ui.selectable_value(&mut state.audio_codec, "FLAC".into(), "FLAC Lossless");
                            });
                        });

                        ui.checkbox(&mut state.match_source_sample_rate, "Match Source Sample Rate");
                        if !state.match_source_sample_rate {
                            ui.horizontal(|ui| {
                                ui.label("Sample Rate:");
                                egui::ComboBox::from_id_salt("editor_srate_box").selected_text(format!("{} Hz", state.sample_rate)).show_ui(
                                    ui,
                                    |ui| {
                                        ui.selectable_value(&mut state.sample_rate, 48000, "48000 Hz (Broadcast)");
                                        ui.selectable_value(&mut state.sample_rate, 44100, "44100 Hz (CD/Web)");
                                        ui.selectable_value(&mut state.sample_rate, 96000, "96000 Hz (High-Res)");
                                    },
                                );
                            });
                        }

                        ui.horizontal(|ui| {
                            ui.label("Channels:");
                            egui::ComboBox::from_id_salt("editor_layout_box").selected_text(format!("{:?}", state.channel_layout)).show_ui(
                                ui,
                                |ui| {
                                    ui.selectable_value(&mut state.channel_layout, ChannelLayout::Stereo, "Stereo (2.0)");
                                    ui.selectable_value(&mut state.channel_layout, ChannelLayout::Mono, "Mono (1.0)");
                                    ui.selectable_value(&mut state.channel_layout, ChannelLayout::Surround51, "5.1 Surround");
                                },
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("Loudness Target (EBU R128):");
                            let lufs_str = state.target_lufs.map(|l| format!("{:.1} LUFS", l)).unwrap_or_else(|| "Off".into());
                            egui::ComboBox::from_id_salt("editor_lufs_box").selected_text(&lufs_str).show_ui(ui, |ui| {
                                if ui.selectable_label(state.target_lufs.is_none(), "Off").clicked() {
                                    state.target_lufs = None;
                                }
                                if ui.selectable_label(state.target_lufs == Some(-14.0), "YouTube / Web (-14 LUFS)").clicked() {
                                    state.target_lufs = Some(-14.0);
                                }
                                if ui.selectable_label(state.target_lufs == Some(-16.0), "Podcast (-16 LUFS)").clicked() {
                                    state.target_lufs = Some(-16.0);
                                }
                                if ui.selectable_label(state.target_lufs == Some(-23.0), "EBU R128 (-23 LUFS)").clicked() {
                                    state.target_lufs = Some(-23.0);
                                }
                                if ui.selectable_label(state.target_lufs == Some(-24.0), "ATSC A/85 (-24 LUFS)").clicked() {
                                    state.target_lufs = Some(-24.0);
                                }
                            });
                        });
                    }
                });

                ui.add_space(8.0);

                // Action buttons
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save Preset").strong().color(tokens.accent_primary)).clicked() {
                        let _ = state.save(commands, queue_mgr);
                    }
                    if ui.button("Cancel").clicked() {
                        state.is_open = false;
                        state.error_message = None;
                    }
                });
            });
        },
    );

    state.is_open = is_open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_presets::PresetRegistry;
    use std::sync::Mutex;

    #[test]
    fn test_preset_editor_validation_and_save() {
        let registry = Arc::new(Mutex::new(PresetRegistry::default()));
        let queue = Arc::new(QueueManager::new_in_memory());
        let commands = CommandRegistry::with_preset_registry(Arc::clone(&registry));
        let mut state = PresetEditorState::default();
        state.open_new();

        // 1. Empty name should fail validation
        state.name = "   ".into();
        assert!(!state.save(&commands, &queue));
        assert!(state.error_message.is_some());

        // 2. Valid configuration should succeed
        state.name = "Vlog 4K High Bitrate".into();
        state.description = "Custom YouTube 4K Preset".into();
        state.width = 3840;
        state.height = 2160;
        state.match_source_dims = false;
        state.bitrate_kbps = 45000;
        state.channel_layout = ChannelLayout::Stereo;
        state.target_lufs = Some(-14.0);

        assert!(state.save(&commands, &queue));
        assert!(!state.is_open);
        assert!(state.error_message.is_none());

        // 3. Verify preset exists in registry
        let reg = registry.lock().unwrap();
        let saved = reg.find_by_name("Vlog 4K High Bitrate");
        assert!(saved.is_some());
        let p = saved.unwrap();
        assert_eq!(p.category, PresetCategory::Custom);
        assert_eq!(p.video.as_ref().unwrap().target_bitrate_kbps, 45000);
        assert_eq!(p.audio.as_ref().unwrap().target_lufs, Some(-14.0));
    }
}
