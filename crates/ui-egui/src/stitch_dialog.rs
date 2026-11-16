//! Stitch clips configuration modal dialog (AME Parity).
//!
//! Allows the user to inspect and re-order clips to be stitched, choose an output destination
//! file path and pick the target export preset rather than dumping silently to temporary storage.

use std::path::PathBuf;

use egui::{RichText, Ui};
use encodercraft_core::{ContainerType, SourceDescriptor};
use encodercraft_engine::QueuedJob;

use crate::queue_panel::job_source_name;
use crate::theme::Tokens;

/// Candidate presets offered in the stitch dialog.
pub const STITCH_PRESETS: &[(&str, ContainerType, &str)] = &[
    ("Match Source - High Bitrate", ContainerType::IsoBmffMp4, "mp4"),
    ("YouTube 4K Ultra HD", ContainerType::IsoBmffMp4, "mp4"),
    ("YouTube 1080p Full HD", ContainerType::IsoBmffMp4, "mp4"),
    ("Apple ProRes 422 HQ", ContainerType::QuickTimeMov, "mov"),
    ("Apple ProRes 422", ContainerType::QuickTimeMov, "mov"),
    ("Broadcast MXF OP1a (DNxHD / PCM)", ContainerType::MxfOp1a, "mxf"),
];

/// State for the stitch sequence configuration dialog.
pub struct StitchDialogState {
    pub is_open: bool,
    pub job_ids: Vec<String>,
    pub clip_names: Vec<String>,
    pub output_path: PathBuf,
    pub preset_name: String,
    pub container: ContainerType,
}

impl Default for StitchDialogState {
    fn default() -> Self {
        Self {
            is_open: false,
            job_ids: Vec::new(),
            clip_names: Vec::new(),
            output_path: PathBuf::new(),
            preset_name: "Match Source - High Bitrate".into(),
            container: ContainerType::IsoBmffMp4,
        }
    }
}

impl StitchDialogState {
    /// Opens the dialog and initializes clips from selected queue jobs.
    pub fn open_for_jobs(&mut self, job_ids: Vec<String>, all_jobs: &[QueuedJob]) {
        self.job_ids = job_ids.clone();
        self.clip_names.clear();
        let mut default_parent = std::env::temp_dir();

        for id in &job_ids {
            if let Some(j) = all_jobs.iter().find(|candidate| &candidate.spec.job_id == id) {
                let name = job_source_name(j);
                if let SourceDescriptor::File { path, .. } = &j.spec.source
                    && let Some(parent) = path.parent()
                {
                    default_parent = parent.to_path_buf();
                }
                self.clip_names.push(name);
            }
        }

        self.output_path = default_parent.join("stitched_sequence.mp4");
        self.preset_name = "Match Source - High Bitrate".to_string();
        self.container = ContainerType::IsoBmffMp4;
        self.is_open = true;
    }

    /// Moves a clip up in the sequence order.
    pub fn move_up(&mut self, idx: usize) {
        if idx > 0 && idx < self.job_ids.len() {
            self.job_ids.swap(idx - 1, idx);
            self.clip_names.swap(idx - 1, idx);
        }
    }

    /// Moves a clip down in the sequence order.
    pub fn move_down(&mut self, idx: usize) {
        if idx + 1 < self.job_ids.len() {
            self.job_ids.swap(idx, idx + 1);
            self.clip_names.swap(idx, idx + 1);
        }
    }
}

/// Renders the modal stitch configuration dialog.
pub fn render_stitch_dialog(
    ctx: &egui::Context,
    state: &mut StitchDialogState,
    tokens: &Tokens,
    on_stitch: &mut Option<(Vec<String>, PathBuf, String)>,
) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;

    egui::Window::new("Stitch Selected Clips into Sequence")
        .open(&mut is_open)
        .resizable(true)
        .collapsible(false)
        .default_width(520.0)
        .default_height(400.0)
        .show(ctx, |ui| {
            render_dialog_contents(ui, state, tokens, on_stitch);
        });

    state.is_open = is_open;
}

fn render_dialog_contents(ui: &mut Ui, state: &mut StitchDialogState, tokens: &Tokens, on_stitch: &mut Option<(Vec<String>, PathBuf, String)>) {
    ui.label(RichText::new("Configure destination and order for stitched sequence transcode:").color(tokens.text_dim));
    ui.add_space(8.0);

    // --- 1. Ordered Clips List ---
    egui::CollapsingHeader::new(RichText::new(format!("Clips in Sequence Order ({})", state.clip_names.len())).strong()).default_open(true).show(
        ui,
        |ui| {
            egui::ScrollArea::vertical().id_salt("stitch_dialog_clips_scroll_area").max_height(160.0).show(ui, |ui| {
                let mut swap_up = None;
                let mut swap_down = None;

                for (idx, name) in state.clip_names.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("#{}:", idx + 1)).strong().color(tokens.accent_primary));
                        ui.label(name);

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can_down = idx + 1 < state.clip_names.len();
                            let can_up = idx > 0;

                            if can_down && ui.small_button("▼").on_hover_text("Move later in sequence").clicked() {
                                swap_down = Some(idx);
                            }
                            if can_up && ui.small_button("▲").on_hover_text("Move earlier in sequence").clicked() {
                                swap_up = Some(idx);
                            }
                        });
                    });
                    ui.separator();
                }

                if let Some(idx) = swap_up {
                    state.move_up(idx);
                }
                if let Some(idx) = swap_down {
                    state.move_down(idx);
                }
            });
        },
    );

    ui.add_space(8.0);

    // --- 2. Destination File ---
    ui.label(RichText::new("Destination Output File:").strong());
    ui.horizontal(|ui| {
        let display_path = state.output_path.display().to_string();
        ui.label(RichText::new(display_path).small().color(tokens.accent_primary));

        if ui.button("Browse...").clicked() {
            let initial_name = state.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("stitched_sequence.mp4");
            if let Some(chosen) = rfd::FileDialog::new().set_title("Choose Stitched Output File").set_file_name(initial_name).save_file() {
                state.output_path = chosen;
            }
        }
    });

    ui.add_space(8.0);

    // --- 3. Preset Selector ---
    ui.label(RichText::new("Export Preset:").strong());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("stitch_preset_selector").selected_text(RichText::new(&state.preset_name).strong()).width(280.0).show_ui(
            ui,
            |ui| {
                for (name, c, ext) in STITCH_PRESETS {
                    let is_selected = state.preset_name == *name;
                    if ui.selectable_label(is_selected, *name).clicked() {
                        state.preset_name = (*name).to_string();
                        state.container = *c;
                        state.output_path.set_extension(ext);
                    }
                }
            },
        );
    });

    ui.add_space(16.0);
    ui.separator();

    // --- 4. Action Buttons ---
    ui.horizontal(|ui| {
        let can_stitch = state.job_ids.len() >= 2;
        ui.add_enabled_ui(can_stitch, |ui| {
            if ui.button(RichText::new("Stitch into Queue").strong().color(tokens.status_completed)).clicked() {
                *on_stitch = Some((state.job_ids.clone(), state.output_path.clone(), state.preset_name.clone()));
                state.is_open = false;
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                state.is_open = false;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_up_and_down() {
        let mut state = StitchDialogState {
            job_ids: vec!["1".into(), "2".into(), "3".into()],
            clip_names: vec!["A".into(), "B".into(), "C".into()],
            ..Default::default()
        };

        state.move_down(0);
        assert_eq!(state.clip_names, vec!["B", "A", "C"]);
        assert_eq!(state.job_ids, vec!["2", "1", "3"]);

        state.move_up(1);
        assert_eq!(state.clip_names, vec!["A", "B", "C"]);
        assert_eq!(state.job_ids, vec!["1", "2", "3"]);
    }
}
