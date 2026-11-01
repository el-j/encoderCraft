//! Queue panel: hierarchical data table showing queued sources, output targets, progress, and controls.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::{CraftJobSpec, JobPriority, JobStatus};
use encodercraft_engine::{QueueManager, QueuedJob};

use crate::icons;
use crate::theme::Tokens;

/// State for the queue panel widget.
#[derive(Default)]
pub struct QueuePanelState {
    pub selected_job_id: Option<String>,
    pub expanded_jobs: std::collections::HashSet<String>,
    pub is_running: bool,
}

/// Renders the queue panel with toolbar and hierarchical job table.
pub fn render_queue_panel(
    ui: &mut Ui,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    ui.vertical(|ui| {
        // --- 1. Top Queue Toolbar ---
        ui.horizontal(|ui| {
            let start_btn = ui.button(format!("{} Start Queue", icons::START_QUEUE));
            if start_btn.clicked() {
                state.is_running = true;
                // If there are queued jobs, process next in background or trigger
                let _ = queue_mgr.process_next_job();
            }

            let pause_btn = ui.button(format!("{} Pause", icons::PAUSE_QUEUE));
            if pause_btn.clicked() {
                state.is_running = false;
                if let Some(ref id) = state.selected_job_id {
                    let _ = queue_mgr.pause_job(id);
                }
            }

            let stop_btn = ui.button(format!("{} Stop", icons::STOP_QUEUE));
            if stop_btn.clicked() {
                state.is_running = false;
                if let Some(ref id) = state.selected_job_id {
                    let _ = queue_mgr.cancel_job(id);
                }
            }

            ui.separator();

            let dup_btn = ui.add_enabled(state.selected_job_id.is_some(), egui::Button::new(format!("{} Duplicate", icons::DUPLICATE_JOB)));
            if dup_btn.clicked()
                && let Some(ref id) = state.selected_job_id
            {
                let _ = queue_mgr.duplicate_job(id);
            }

            let remove_btn = ui.add_enabled(state.selected_job_id.is_some(), egui::Button::new(format!("{} Remove", icons::REMOVE_JOB)));
            if remove_btn.clicked()
                && let Some(ref id) = state.selected_job_id
            {
                let _ = queue_mgr.remove_job(id);
                state.selected_job_id = None;
            }

            let inspect_btn = ui.add_enabled(state.selected_job_id.is_some(), egui::Button::new(format!("{} Settings...", icons::SETTINGS)));
            if inspect_btn.clicked()
                && let Some(ref id) = state.selected_job_id
            {
                let jobs = queue_mgr.list_jobs();
                if let Some(j) = jobs.iter().find(|j| &j.spec.job_id == id) {
                    *on_inspect_job = Some(j.spec.clone());
                }
            }
        });

        ui.add_space(4.0);
        ui.separator();

        // --- 2. Hierarchical Job Table ---
        let jobs = queue_mgr.list_jobs();

        if jobs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("The transcode queue is empty.").color(tokens.text_dim));
                ui.label(RichText::new("Drag and drop media files or select from Presets to begin.").color(tokens.text_dim));
            });
            return;
        }

        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            for job in &jobs {
                render_job_row(ui, job, state, queue_mgr, tokens, on_inspect_job);
            }
        });
    });
}

fn render_job_row(
    ui: &mut Ui,
    job: &QueuedJob,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    let is_selected = state.selected_job_id.as_deref() == Some(&job.spec.job_id);
    let job_id = job.spec.job_id.clone();
    let is_expanded = state.expanded_jobs.contains(&job_id);

    let row_bg = if is_selected { tokens.bg_surface_active } else { tokens.bg_surface };

    egui::Frame::new().fill(row_bg).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            // Expand / Collapse arrow for multi-targets
            let arrow = if is_expanded { "▼" } else { "▶" };
            if ui.small_button(arrow).clicked() {
                if is_expanded {
                    state.expanded_jobs.remove(&job_id);
                } else {
                    state.expanded_jobs.insert(job_id.clone());
                }
            }

            // Status chip
            render_status_chip(ui, &job.status, tokens);

            // Priority Badge
            if job.spec.priority != JobPriority::Normal {
                let prio_label = format!("{:?}", job.spec.priority);
                ui.label(RichText::new(prio_label).small().color(tokens.accent_primary));
            }

            // Source Name
            let source_name = match &job.spec.source {
                encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("File"),
                encodercraft_core::SourceDescriptor::StitchedClips { .. } => "Stitched Sequence",
                encodercraft_core::SourceDescriptor::Pipe { endpoint, .. } => endpoint.as_str(),
                encodercraft_core::SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.as_str(),
            };

            let job_select_btn = ui.selectable_label(is_selected, RichText::new(source_name).strong());
            if job_select_btn.clicked() {
                state.selected_job_id = Some(job_id.clone());
            }

            if job_select_btn.double_clicked() {
                *on_inspect_job = Some(job.spec.clone());
            }

            // Preset Name
            if let Some(target) = job.spec.targets.first() {
                ui.label(RichText::new(format!("➔ {}", target.preset_name)).color(tokens.text_secondary));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Actions menu
                ui.menu_button("•••", |ui| {
                    if ui.button("Inspect Settings...").clicked() {
                        *on_inspect_job = Some(job.spec.clone());
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        let _ = queue_mgr.duplicate_job(&job_id);
                        ui.close();
                    }
                    if ui.button("Start Now").clicked() {
                        let _ = queue_mgr.start_job(&job_id);
                        let _ = queue_mgr.process_next_job();
                        ui.close();
                    }
                    if ui.button("Pause").clicked() {
                        let _ = queue_mgr.pause_job(&job_id);
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        let _ = queue_mgr.remove_job(&job_id);
                        ui.close();
                    }
                });

                // FPS & Metrics
                if job.status == JobStatus::Processing {
                    ui.label(RichText::new(format!("{:.1} FPS", job.current_fps)).small().color(tokens.text_dim));
                }

                // Progress bar
                let progress = (job.progress_percent / 100.0).clamp(0.0, 1.0) as f32;
                let pbar = egui::ProgressBar::new(progress).show_percentage().desired_width(120.0);
                ui.add(pbar);
            });
        });

        // Expanded Child Output Targets
        if is_expanded {
            ui.indent(job_id.clone(), |ui| {
                for (idx, target) in job.spec.targets.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Target #{}:", idx + 1)).small().color(tokens.text_dim));
                        ui.label(RichText::new(&target.preset_name).color(tokens.accent_primary));
                        ui.label(RichText::new(format!("➔ {}", target.output_path.display())).small().color(tokens.text_secondary));
                    });
                }
            });
        }
    });

    ui.add_space(2.0);
}

fn render_status_chip(ui: &mut Ui, status: &JobStatus, tokens: &Tokens) {
    let (text, color) = match status {
        JobStatus::Queued => ("Queued", tokens.status_queued),
        JobStatus::Processing => ("Encoding", tokens.status_processing),
        JobStatus::Paused => ("Paused", tokens.status_paused),
        JobStatus::Completed => ("Done", tokens.status_completed),
        JobStatus::Failed(_) => ("Failed", tokens.status_failed),
        JobStatus::Cancelled => ("Canceled", tokens.status_cancelled),
    };

    egui::Frame::new()
        .fill(color.gamma_multiply(0.2))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(3)
        .inner_margin(egui::vec2(5.0, 1.5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });
}
