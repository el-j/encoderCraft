//! Encoding Activity Log dialog: searchable historical transcode records (AME Parity).

use std::sync::Arc;

use egui::RichText;
use encodercraft_engine::{LogEntry, QueueManager, cmd::CommandRegistry};

use crate::icons;
use crate::theme::Tokens;

/// State for the encoding activity log modal dialog.
#[derive(Default)]
pub struct LogDialogState {
    pub is_open: bool,
    pub filter: String,
    pub status_message: Option<String>,
}

/// Renders the Encoding Activity Log modal window.
pub fn render_log_dialog(
    ctx: &egui::Context,
    state: &mut LogDialogState,
    queue_mgr: &Arc<QueueManager>,
    commands: &CommandRegistry,
    tokens: &Tokens,
) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;
    let mut close_requested = false;

    egui::Window::new("Encoding Activity Log").open(&mut is_open).resizable(true).default_size(egui::vec2(720.0, 440.0)).collapsible(false).show(
        ctx,
        |ui| {
            let entries = match commands.dispatch(queue_mgr, "queue.log", serde_json::json!({})) {
                Ok(value) => match serde_json::from_value::<Vec<LogEntry>>(value) {
                    Ok(entries) => {
                        state.status_message = None;
                        entries
                    }
                    Err(error) => {
                        state.status_message = Some(format!("The log command returned invalid data: {error}"));
                        Vec::new()
                    }
                },
                Err(error) => {
                    state.status_message = Some(format!("Could not load the encoding log: {error}"));
                    Vec::new()
                }
            };

            ui.horizontal(|ui| {
                ui.label(RichText::new("Filter:").strong());
                ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Search by clip, preset, or status..."));

                if !state.filter.is_empty() && icons::button(ui, icons::Icon::Close, 16.0, false, tokens, "Clear filter").clicked() {
                    state.filter.clear();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} entries recorded", entries.len())).small().color(tokens.text_dim));
                });
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            if let Some(message) = &state.status_message {
                ui.label(RichText::new(message).color(tokens.status_failed));
            }

            // Filter entries
            let filter_lower = state.filter.to_lowercase();
            let filtered: Vec<_> = entries
                .iter()
                .filter(|e| {
                    if filter_lower.is_empty() {
                        return true;
                    }
                    e.source_name.to_lowercase().contains(&filter_lower)
                        || e.preset_name.to_lowercase().contains(&filter_lower)
                        || e.output_path.to_lowercase().contains(&filter_lower)
                        || e.status.to_lowercase().contains(&filter_lower)
                })
                .collect();

            if filtered.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    if entries.is_empty() {
                        ui.label(RichText::new("No transcode events logged yet.").color(tokens.text_dim));
                        ui.label(RichText::new("Completed and failed batch jobs will appear here.").small().color(tokens.text_dim));
                    } else {
                        ui.label(RichText::new("No matching log entries found for current filter.").color(tokens.text_dim));
                    }
                });
            } else {
                // Table header
                egui::Frame::new().fill(tokens.bg_surface_active).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Status").strong().small());
                        ui.separator();
                        ui.label(RichText::new("Source Clip").strong().small());
                        ui.separator();
                        ui.label(RichText::new("Preset").strong().small());
                        ui.separator();
                        ui.label(RichText::new("Duration").strong().small());
                        ui.separator();
                        ui.label(RichText::new("Output Path").strong().small());
                    });
                });

                ui.add_space(2.0);

                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false; 2]).show(ui, |ui| {
                    for entry in filtered.iter().rev() {
                        let is_ok = entry.status == "Completed";
                        let status_color = if is_ok { tokens.status_completed } else { tokens.status_failed };

                        egui::Frame::new().fill(tokens.bg_surface).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Status chip
                                egui::Frame::new()
                                    .fill(status_color.gamma_multiply(0.2))
                                    .stroke(egui::Stroke::new(1.0, status_color))
                                    .corner_radius(3)
                                    .inner_margin(egui::vec2(4.0, 1.0))
                                    .show(ui, |ui| {
                                        let lbl = ui.label(RichText::new(&entry.status).small().color(status_color));
                                        if let Some(ref err) = entry.error {
                                            lbl.on_hover_text(format!("Error: {err}"));
                                        }
                                    });

                                // Source clip
                                ui.label(RichText::new(&entry.source_name).strong());

                                // Preset
                                ui.label(RichText::new(format!("-> {}", entry.preset_name)).color(tokens.text_secondary));

                                // Duration
                                let duration_sec = entry.duration_ms as f64 / 1000.0;
                                ui.label(RichText::new(format!("{duration_sec:.2}s")).small().color(tokens.accent_primary));

                                // Output path
                                ui.label(RichText::new(&entry.output_path).small().color(tokens.text_dim));
                            });
                        });
                        ui.add_space(2.0);
                    }
                });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Bottom actions
            ui.horizontal(|ui| {
                if ui.button("Clear Log").on_hover_text("Erase all transcode log entries").clicked() {
                    match commands.dispatch(queue_mgr, "queue.clear_log", serde_json::json!({})) {
                        Ok(_) => state.status_message = None,
                        Err(error) => state.status_message = Some(format!("Could not clear log: {error}")),
                    }
                }

                if ui.button("Copy to Clipboard").on_hover_text("Copy formatted log text to clipboard").clicked() {
                    let mut text = String::from("Status\tSource\tPreset\tDuration(s)\tOutput\n");
                    for e in &entries {
                        let sec = e.duration_ms as f64 / 1000.0;
                        text.push_str(&format!("{}\t{}\t{}\t{:.2}\t{}\n", e.status, e.source_name, e.preset_name, sec, e.output_path));
                    }
                    ui.ctx().copy_text(text);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
                if let Some(message) = &state.status_message {
                    ui.label(RichText::new(message).small().color(tokens.status_failed));
                }
            });
        },
    );

    if close_requested {
        is_open = false;
    }
    state.is_open = is_open;
}
