//! Application Preferences dialog: general, media, appearance, performance, and audio configuration (AME Parity).

use egui::RichText;
use encodercraft_engine::AppPreferences;

use crate::icons;
use crate::theme::Tokens;

/// Categories for preferences navigation (AME Parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrefsCategory {
    #[default]
    General,
    Appearance,
    Media,
    Performance,
    Audio,
    Shortcuts,
}

impl PrefsCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Media => "Media & Storage",
            Self::Performance => "Memory & Performance",
            Self::Audio => "Audio",
            Self::Shortcuts => "Keyboard Shortcuts",
        }
    }
}

/// State for the Preferences modal dialog.
#[derive(Default)]
pub struct PrefsDialogState {
    pub is_open: bool,
    pub prefs: AppPreferences,
    pub shortcut_recording: Option<crate::commands::AppCommand>,
    pub shortcut_message: Option<String>,
    pub selected_category: PrefsCategory,
}

/// Renders the Preferences modal window.
pub fn render_prefs_dialog(
    ctx: &egui::Context,
    state: &mut PrefsDialogState,
    shortcuts: &mut crate::settings::WorkspaceShortcuts,
    tokens: &Tokens,
    on_theme_change: &mut Option<String>,
) {
    if !state.is_open {
        return;
    }

    if let Some(command) = state.shortcut_recording
        && let Some((key, modifiers, event)) = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers, event.clone())),
                _ => None,
            })
        })
    {
        ctx.input_mut(|input| {
            let _ = input.consume_key(modifiers, key);
        });
        state.shortcut_recording = None;
        if key == egui::Key::Escape && !modifiers.any() {
            state.shortcut_message = Some("Shortcut recording cancelled.".into());
        } else if let Some(binding) = crate::settings::WorkspaceShortcuts::capture(&event) {
            match shortcuts.set_command_binding(command, binding) {
                Ok(()) => {
                    let label = command.info().map_or("Command", |info| info.label);
                    let shortcut = shortcuts.command_display(command).unwrap_or_else(|| "Not assigned".into());
                    state.shortcut_message = Some(format!("{label} set to {shortcut}."));
                }
                Err(error) => state.shortcut_message = Some(error),
            }
        } else {
            state.shortcut_message = Some("That key cannot be used as a shortcut.".into());
        }
    }

    let mut is_open = state.is_open;
    let mut close_requested = false;

    egui::Window::new("Preferences")
        .open(&mut is_open)
        .resizable(true)
        .default_size(egui::vec2(720.0, 520.0))
        .min_size(egui::vec2(600.0, 360.0))
        .collapsible(false)
        .show(ctx, |ui| {
            ui.heading(RichText::new("encoderCraft Preferences").size(16.0));
            ui.add_space(8.0);

            let available_h = (ui.available_height() - 48.0).max(260.0);

            ui.horizontal(|ui| {
                // Left sidebar: categories
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    ui.set_min_height(available_h);
                    egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                        ui.set_min_height(available_h - 12.0);
                        let categories = [
                            PrefsCategory::General,
                            PrefsCategory::Appearance,
                            PrefsCategory::Media,
                            PrefsCategory::Performance,
                            PrefsCategory::Audio,
                            PrefsCategory::Shortcuts,
                        ];
                        for cat in categories {
                            let is_selected = state.selected_category == cat;
                            if ui.selectable_label(is_selected, cat.label()).clicked() {
                                state.selected_category = cat;
                            }
                        }
                    });
                });

                ui.separator();

                // Right pane: selected category content inside scroll area
                egui::ScrollArea::vertical()
                    .id_salt("prefs_dialog_category_scroll")
                    .auto_shrink([false, false])
                    .min_scrolled_height(available_h)
                    .show(ui, |ui| {
                        ui.set_min_height(available_h);
                        match state.selected_category {
                            PrefsCategory::General => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("General Options").strong());
                                    ui.add_space(6.0);

                                    ui.checkbox(&mut state.prefs.play_completion_sound, "Play chime when finished encoding");
                                    ui.checkbox(&mut state.prefs.auto_start_queue, "Auto-start queue when media is added");
                                    ui.checkbox(&mut state.prefs.auto_start_on_idle, "Start queue automatically when idle for");
                                    if state.prefs.auto_start_on_idle {
                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            ui.add(egui::Slider::new(&mut state.prefs.idle_timeout_minutes, 1..=60).text("minutes"));
                                        });
                                    }
                                    ui.checkbox(&mut state.prefs.increment_output_filename, "Increment output file names if file exists");
                                    ui.checkbox(&mut state.prefs.remove_finished_on_exit, "Remove completed items from queue on exit");
                                    ui.checkbox(&mut state.prefs.auto_encode_watch_folders, "Automatically encode watch folders");
                                    ui.checkbox(&mut state.prefs.auto_shutdown, "Shut down the computer when the queue finishes");
                                });
                            }
                            PrefsCategory::Appearance => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("Appearance & Theme").strong());
                                    ui.add_space(6.0);

                                    ui.horizontal(|ui| {
                                        ui.label("Interface Theme:");
                                        let is_dark = state.prefs.theme == "Dark";
                                        let is_light = state.prefs.theme == "Light";
                                        let is_hc = state.prefs.theme == "High Contrast";

                                        if ui.selectable_label(is_dark, "Dark").clicked() {
                                            state.prefs.theme = "Dark".into();
                                            *on_theme_change = Some("Dark".into());
                                        }
                                        if ui.selectable_label(is_light, "Light").clicked() {
                                            state.prefs.theme = "Light".into();
                                            *on_theme_change = Some("Light".into());
                                        }
                                        if ui.selectable_label(is_hc, "High Contrast").clicked() {
                                            state.prefs.theme = "High Contrast".into();
                                            *on_theme_change = Some("High Contrast".into());
                                        }
                                    });
                                    ui.add_space(4.0);
                                    ui.checkbox(&mut state.prefs.system_title_bar, "Use the system title bar (applies after restart)");
                                });
                            }
                            PrefsCategory::Media => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("Media & Storage").strong());
                                    ui.add_space(6.0);

                                    ui.label("Default Output Directory:");
                                    ui.horizontal(|ui| {
                                        let path_str = state
                                            .prefs
                                            .default_output_dir
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_else(|| "Same as source directory".into());
                                        ui.label(RichText::new(path_str).color(tokens.text_secondary));

                                        if ui.button("Browse...").clicked()
                                            && let Some(folder) = rfd::FileDialog::new().set_title("Select Default Output Directory").pick_folder()
                                        {
                                            state.prefs.default_output_dir = Some(folder);
                                        }

                                        if state.prefs.default_output_dir.is_some() && ui.small_button("Clear").clicked() {
                                            state.prefs.default_output_dir = None;
                                        }
                                    });
                                    ui.add_space(6.0);
                                    ui.checkbox(&mut state.prefs.increment_output_filename, "Prevent overwriting by appending incremental suffix");
                                });
                            }
                            PrefsCategory::Performance => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("Rendering & Hardware Acceleration").strong());
                                    ui.add_space(6.0);

                                    let hw = encodercraft_platform::detect_hardware();
                                    let mut available_renderers = vec!["Pure-Rust Software (CPU)".to_string()];
                                    if hw.backend != encodercraft_platform::HardwareBackend::SoftwareOnly {
                                        let label = match hw.backend {
                                            encodercraft_platform::HardwareBackend::VideoToolbox => {
                                                "Hardware: Apple VideoToolbox (Metal)".to_string()
                                            }
                                            encodercraft_platform::HardwareBackend::Nvenc => "Hardware: NVIDIA NVENC (CUDA)".to_string(),
                                            encodercraft_platform::HardwareBackend::Vaapi => "Hardware: Linux VA-API (VAAPI)".to_string(),
                                            encodercraft_platform::HardwareBackend::Amf => "Hardware: AMD AMF".to_string(),
                                            encodercraft_platform::HardwareBackend::MediaFoundation => {
                                                "Hardware: Windows Media Foundation (MF)".to_string()
                                            }
                                            encodercraft_platform::HardwareBackend::SoftwareOnly => String::new(),
                                        };
                                        if !label.is_empty() {
                                            available_renderers.push(label);
                                        }
                                    }

                                    // Ensure current selection is valid for active hardware
                                    if !state.prefs.selected_renderer.is_empty()
                                        && !available_renderers.iter().any(|r| r == &state.prefs.selected_renderer)
                                    {
                                        state.prefs.selected_renderer =
                                            available_renderers.first().cloned().unwrap_or_else(|| "Pure-Rust Software (CPU)".to_string());
                                    }

                                    ui.horizontal(|ui| {
                                        ui.label("Renderer Backend:");
                                        let current = if state.prefs.selected_renderer.is_empty() {
                                            "Pure-Rust Software (CPU)"
                                        } else {
                                            &state.prefs.selected_renderer
                                        };
                                        egui::ComboBox::from_id_salt("prefs_renderer_combo").selected_text(current).show_ui(ui, |ui| {
                                            for r in &available_renderers {
                                                if ui.selectable_label(state.prefs.selected_renderer == *r, r).clicked() {
                                                    state.prefs.selected_renderer = r.clone();
                                                }
                                            }
                                        });
                                    });

                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        icons::icon_label(ui, icons::Icon::Info, 13.0, tokens.text_dim);
                                        ui.label(RichText::new(format!("Detected Device: {}", hw.device_name)).small().color(tokens.text_dim));
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Parallel render concurrency:");
                                        ui.add(egui::Slider::new(&mut state.prefs.max_parallel_jobs, 1..=16).text("workers"));
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("GPU Memory / VRAM Budget:");
                                        ui.add(egui::Slider::new(&mut state.prefs.vram_budget_percent, 10..=100).text("%"));
                                    });
                                });
                            }
                            PrefsCategory::Audio => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("Audio Master Clock & Channels").strong());
                                    ui.add_space(6.0);

                                    ui.horizontal(|ui| {
                                        ui.label("Default Sample Rate:");
                                        let current_rate = format!("{} Hz", state.prefs.default_audio_sample_rate);
                                        egui::ComboBox::from_id_salt("prefs_audio_rate_combo").selected_text(current_rate).show_ui(ui, |ui| {
                                            for rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
                                                let label = format!("{rate} Hz");
                                                if ui.selectable_label(state.prefs.default_audio_sample_rate == rate, label).clicked() {
                                                    state.prefs.default_audio_sample_rate = rate;
                                                }
                                            }
                                        });
                                    });

                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Default Channels:");
                                        let current_ch = match state.prefs.default_audio_channels {
                                            1 => "Mono (1 Channel)",
                                            6 => "5.1 Surround (6 Channels)",
                                            _ => "Stereo (2 Channels)",
                                        };
                                        egui::ComboBox::from_id_salt("prefs_audio_ch_combo").selected_text(current_ch).show_ui(ui, |ui| {
                                            if ui.selectable_label(state.prefs.default_audio_channels == 1, "Mono (1 Channel)").clicked() {
                                                state.prefs.default_audio_channels = 1;
                                            }
                                            if ui.selectable_label(state.prefs.default_audio_channels == 2, "Stereo (2 Channels)").clicked() {
                                                state.prefs.default_audio_channels = 2;
                                            }
                                            if ui.selectable_label(state.prefs.default_audio_channels == 6, "5.1 Surround (6 Channels)").clicked() {
                                                state.prefs.default_audio_channels = 6;
                                            }
                                        });
                                    });
                                });
                            }
                            PrefsCategory::Shortcuts => {
                                egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                                    ui.label(RichText::new("Keyboard Shortcuts").strong());
                                    ui.label(RichText::new("Select Change, then press the shortcut you want.").small().color(tokens.text_secondary));
                                    egui::ScrollArea::vertical().id_salt("prefs_shortcuts_scroll_area").max_height(280.0).show(ui, |ui| {
                                        for info in crate::commands::COMMANDS.iter().filter(|info| info.shortcut_configurable) {
                                            let command = info.command;
                                            let display = shortcuts.command_display(command).unwrap_or_else(|| "Not assigned".into());
                                            ui.horizontal(|ui| {
                                                ui.label(info.label);
                                                ui.label(RichText::new(display).color(tokens.text_secondary));
                                                if state.shortcut_recording == Some(command) {
                                                    ui.label(RichText::new("Press a key...").color(tokens.accent_primary));
                                                } else if ui.button("Change").clicked() {
                                                    state.shortcut_recording = Some(command);
                                                    state.shortcut_message = None;
                                                }
                                            });
                                        }
                                    });
                                    if ui.button("Reset Keyboard Shortcuts").clicked() {
                                        *shortcuts = crate::settings::WorkspaceShortcuts::default();
                                        state.shortcut_recording = None;
                                        state.shortcut_message = Some("Keyboard shortcuts restored to defaults.".into());
                                    }
                                    if let Some(message) = &state.shortcut_message {
                                        ui.label(RichText::new(message).small().color(tokens.text_secondary));
                                    }
                                });
                            }
                        };
                    });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Actions
            ui.horizontal(|ui| {
                if ui.button("Reset to Defaults").clicked() {
                    state.prefs = AppPreferences::default();
                    *on_theme_change = Some("Dark".into());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Done").clicked() {
                        close_requested = true;
                    }
                });
            });
        });

    if close_requested {
        is_open = false;
    }
    state.is_open = is_open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefs_categories_labels_and_defaults() {
        let state = PrefsDialogState::default();
        assert_eq!(state.selected_category, PrefsCategory::General);
        assert_eq!(PrefsCategory::General.label(), "General");
        assert_eq!(PrefsCategory::Appearance.label(), "Appearance");
        assert_eq!(PrefsCategory::Media.label(), "Media & Storage");
        assert_eq!(PrefsCategory::Performance.label(), "Memory & Performance");
        assert_eq!(PrefsCategory::Audio.label(), "Audio");
        assert_eq!(PrefsCategory::Shortcuts.label(), "Keyboard Shortcuts");
    }

    #[test]
    fn test_prefs_audio_and_concurrency_defaults() {
        let state = PrefsDialogState::default();
        assert_eq!(state.prefs.default_audio_sample_rate, 48_000);
        assert_eq!(state.prefs.default_audio_channels, 2);
        assert_eq!(state.prefs.max_parallel_jobs, 2);
        assert_eq!(state.prefs.vram_budget_percent, 80);
    }

    #[test]
    fn test_hardware_renderer_detection_contains_software_and_valid_backend() {
        let hw = encodercraft_platform::detect_hardware();
        let mut available = vec!["Pure-Rust Software (CPU)".to_string()];
        if hw.backend != encodercraft_platform::HardwareBackend::SoftwareOnly {
            let label = match hw.backend {
                encodercraft_platform::HardwareBackend::VideoToolbox => "Hardware: Apple VideoToolbox (Metal)".to_string(),
                encodercraft_platform::HardwareBackend::Nvenc => "Hardware: NVIDIA NVENC (CUDA)".to_string(),
                encodercraft_platform::HardwareBackend::Vaapi => "Hardware: Linux VA-API (VAAPI)".to_string(),
                encodercraft_platform::HardwareBackend::Amf => "Hardware: AMD AMF".to_string(),
                encodercraft_platform::HardwareBackend::MediaFoundation => "Hardware: Windows Media Foundation (MF)".to_string(),
                encodercraft_platform::HardwareBackend::SoftwareOnly => String::new(),
            };
            if !label.is_empty() {
                available.push(label);
            }
        }
        assert!(available.contains(&"Pure-Rust Software (CPU)".to_string()));
        assert!(!available.is_empty());
    }
}
