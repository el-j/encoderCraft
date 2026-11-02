//! Application Preferences dialog: general, media, and appearance configuration (AME Parity).

use egui::RichText;
use encodercraft_engine::AppPreferences;

use crate::theme::Tokens;

/// State for the Preferences modal dialog.
#[derive(Default)]
pub struct PrefsDialogState {
    pub is_open: bool,
    pub prefs: AppPreferences,
}

/// Renders the Preferences modal window.
pub fn render_prefs_dialog(ctx: &egui::Context, state: &mut PrefsDialogState, tokens: &Tokens, on_theme_change: &mut Option<String>) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;
    let mut close_requested = false;

    egui::Window::new("Preferences").open(&mut is_open).resizable(false).default_size(egui::vec2(520.0, 380.0)).collapsible(false).show(ctx, |ui| {
        ui.heading(RichText::new("encoderCraft Preferences").size(16.0));
        ui.add_space(8.0);

        // 1. General Section
        egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
            ui.label(RichText::new("General Options").strong());
            ui.add_space(4.0);

            ui.checkbox(&mut state.prefs.play_completion_sound, "Play completion chime when queue finishes");
            ui.checkbox(&mut state.prefs.auto_start_queue, "Auto-start queue when media is added");
            ui.checkbox(&mut state.prefs.remove_finished_on_exit, "Remove finished jobs from queue on exit");

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Parallel render concurrency:");
                ui.add(egui::Slider::new(&mut state.prefs.max_parallel_jobs, 1..=8).text("workers"));
            });
        });

        ui.add_space(8.0);

        // 2. Media & Storage Section
        egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
            ui.label(RichText::new("Media & Storage").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Default Output Directory:");
                let path_str =
                    state.prefs.default_output_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Same as source directory".into());
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
        });

        ui.add_space(8.0);

        // 3. Appearance Section
        egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
            ui.label(RichText::new("Appearance & Theme").strong());
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Interface Theme:");
                let is_dark = state.prefs.theme == "Dark";
                let is_hc = state.prefs.theme == "High Contrast";

                if ui.selectable_label(is_dark, "Dark").clicked() {
                    state.prefs.theme = "Dark".into();
                    *on_theme_change = Some("Dark".into());
                }
                if ui.selectable_label(is_hc, "High Contrast").clicked() {
                    state.prefs.theme = "High Contrast".into();
                    *on_theme_change = Some("High Contrast".into());
                }
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
