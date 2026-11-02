//! Queue panel: hierarchical data table showing queued sources, output targets, progress, and batch controls.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::{CraftJobSpec, JobPriority, JobStatus};
use encodercraft_engine::{QueueManager, QueuedJob};

use crate::icons;
use crate::theme::Tokens;

/// State for the queue panel widget supporting full multi-job batch operations (AME Parity).
#[derive(Default)]
pub struct QueuePanelState {
    pub selected_job_id: Option<String>,
    pub selected_job_ids: std::collections::HashSet<String>,
    pub expanded_jobs: std::collections::HashSet<String>,
    pub is_running: bool,
    pub last_clicked_index: Option<usize>,
}

impl QueuePanelState {
    /// Selects a single job, clearing any previous multi-selection.
    pub fn select_single(&mut self, job_id: String, index: usize) {
        self.selected_job_ids.clear();
        self.selected_job_ids.insert(job_id.clone());
        self.selected_job_id = Some(job_id);
        self.last_clicked_index = Some(index);
    }

    /// Toggles a job in the current selection (Cmd/Ctrl+Click).
    pub fn toggle_select(&mut self, job_id: String, index: usize) {
        if self.selected_job_ids.contains(&job_id) {
            self.selected_job_ids.remove(&job_id);
            if self.selected_job_id.as_deref() == Some(&job_id) {
                self.selected_job_id = self.selected_job_ids.iter().next().cloned();
            }
        } else {
            self.selected_job_ids.insert(job_id.clone());
            self.selected_job_id = Some(job_id);
        }
        self.last_clicked_index = Some(index);
    }

    /// Selects a contiguous range of jobs (Shift+Click).
    pub fn range_select(&mut self, jobs: &[QueuedJob], target_index: usize) {
        let start = self.last_clicked_index.unwrap_or(0).min(target_index);
        let end = self.last_clicked_index.unwrap_or(0).max(target_index);
        for i in start..=end {
            if let Some(j) = jobs.get(i) {
                self.selected_job_ids.insert(j.spec.job_id.clone());
            }
        }
        if let Some(j) = jobs.get(target_index) {
            self.selected_job_id = Some(j.spec.job_id.clone());
        }
        self.last_clicked_index = Some(target_index);
    }

    /// Selects all jobs in the queue (Cmd+A).
    pub fn select_all(&mut self, jobs: &[QueuedJob]) {
        self.selected_job_ids.clear();
        for j in jobs {
            self.selected_job_ids.insert(j.spec.job_id.clone());
        }
        self.selected_job_id = self.selected_job_ids.iter().next().cloned();
    }

    /// Clears active selection.
    pub fn clear_selection(&mut self) {
        self.selected_job_ids.clear();
        self.selected_job_id = None;
        self.last_clicked_index = None;
    }

    /// Returns a list of currently selected job IDs.
    pub fn selected_ids_list(&self) -> Vec<String> {
        if !self.selected_job_ids.is_empty() {
            self.selected_job_ids.iter().cloned().collect()
        } else if let Some(ref id) = self.selected_job_id {
            vec![id.clone()]
        } else {
            Vec::new()
        }
    }
}

/// Renders the queue panel with batch toolbar, global progress, and hierarchical job table.
pub fn render_queue_panel(
    ui: &mut Ui,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    let jobs = queue_mgr.list_jobs();

    ui.vertical(|ui| {
        // --- Keyboard shortcuts for AME parity ---
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            state.is_running = true;
            let _ = queue_mgr.process_next_job();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.is_running = false;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
            state.select_all(&jobs);
        }
        if ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp)) {
            let ids = state.selected_ids_list();
            for id in ids {
                let _ = queue_mgr.move_job_up(&id);
            }
        }
        if ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowDown)) {
            let mut ids = state.selected_ids_list();
            ids.reverse();
            for id in ids {
                let _ = queue_mgr.move_job_down(&id);
            }
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D)) {
            let ids = state.selected_ids_list();
            let mut new_ids = Vec::new();
            for id in ids {
                if let Ok(new_id) = queue_mgr.duplicate_job(&id) {
                    new_ids.push(new_id);
                }
            }
            if !new_ids.is_empty() {
                state.selected_job_ids.clear();
                for id in new_ids {
                    state.selected_job_ids.insert(id.clone());
                    state.selected_job_id = Some(id);
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            let ids = state.selected_ids_list();
            for id in ids {
                let _ = queue_mgr.remove_job(&id);
            }
            state.clear_selection();
        }

        // --- 1. Top Batch Queue Toolbar ---
        let selected_count = state.selected_job_ids.len().max(if state.selected_job_id.is_some() { 1 } else { 0 });
        let has_selection = selected_count > 0;

        ui.horizontal(|ui| {
            let start_btn = ui.button(format!("{} Start Queue", icons::START_QUEUE)).on_hover_text("Start processing the batch queue (Enter)");
            if start_btn.clicked() {
                state.is_running = true;
                let _ = queue_mgr.process_next_job();
            }

            let pause_btn = ui.button(format!("{} Pause", icons::PAUSE_QUEUE)).on_hover_text("Pause active encode");
            if pause_btn.clicked() {
                state.is_running = false;
                for id in state.selected_ids_list() {
                    let _ = queue_mgr.pause_job(&id);
                }
            }

            let stop_btn = ui.button(format!("{} Stop", icons::STOP_QUEUE)).on_hover_text("Stop batch queue (Esc)");
            if stop_btn.clicked() {
                state.is_running = false;
                for id in state.selected_ids_list() {
                    let _ = queue_mgr.cancel_job(&id);
                }
            }

            ui.separator();

            let add_output_btn =
                ui.add_enabled(has_selection, egui::Button::new("+ Add Output")).on_hover_text("Add another output target to selected job(s)");
            if add_output_btn.clicked() {
                for id in state.selected_ids_list() {
                    if let Some(j) = jobs.iter().find(|j| j.spec.job_id == id) {
                        let count = j.spec.targets.len() + 1;
                        let new_target = encodercraft_core::OutputTarget {
                            target_id: format!("target-{count}"),
                            output_path: std::path::PathBuf::from(format!("output_{count}.mp4")),
                            preset_name: "Match Source - High Bitrate".into(),
                            container: encodercraft_core::ContainerType::IsoBmffMp4,
                            custom_width: Some(1920),
                            custom_height: Some(1080),
                            custom_frame_rate: None,
                            target_bitrate_kbps: Some(12000),
                            target_lufs: Some(-14.0),
                            ..Default::default()
                        };
                        let _ = queue_mgr.add_output_target(&id, new_target);
                        state.expanded_jobs.insert(id);
                    }
                }
            }

            let dup_label = if selected_count > 1 {
                format!("{} Duplicate ({selected_count})", icons::DUPLICATE_JOB)
            } else {
                format!("{} Duplicate", icons::DUPLICATE_JOB)
            };
            let dup_btn = ui.add_enabled(has_selection, egui::Button::new(dup_label)).on_hover_text("Duplicate selected job(s) (Cmd+D)");
            if dup_btn.clicked() {
                let ids = state.selected_ids_list();
                for id in ids {
                    let _ = queue_mgr.duplicate_job(&id);
                }
            }

            let can_move_up = has_selection;
            let move_up_btn = ui.add_enabled(can_move_up, egui::Button::new("▲")).on_hover_text("Move selected up in queue (Alt+Up)");
            if move_up_btn.clicked() {
                for id in state.selected_ids_list() {
                    let _ = queue_mgr.move_job_up(&id);
                }
            }

            let can_move_down = has_selection;
            let move_down_btn = ui.add_enabled(can_move_down, egui::Button::new("▼")).on_hover_text("Move selected down in queue (Alt+Down)");
            if move_down_btn.clicked() {
                let mut ids = state.selected_ids_list();
                ids.reverse();
                for id in ids {
                    let _ = queue_mgr.move_job_down(&id);
                }
            }

            let reset_label = if selected_count > 1 { format!("⟳ Reset ({selected_count})") } else { "⟳ Reset".into() };
            let reset_btn = ui.add_enabled(has_selection, egui::Button::new(reset_label)).on_hover_text("Reset selected job(s) back to Ready");
            if reset_btn.clicked() {
                for id in state.selected_ids_list() {
                    let _ = queue_mgr.reset_job_status(&id);
                }
            }

            let remove_label =
                if selected_count > 1 { format!("{} Remove ({selected_count})", icons::REMOVE_JOB) } else { format!("{} Remove", icons::REMOVE_JOB) };
            let remove_btn = ui.add_enabled(has_selection, egui::Button::new(remove_label)).on_hover_text("Remove selected job(s) (Delete)");
            if remove_btn.clicked() {
                let ids = state.selected_ids_list();
                for id in ids {
                    let _ = queue_mgr.remove_job(&id);
                }
                state.clear_selection();
            }

            let inspect_btn = ui
                .add_enabled(has_selection, egui::Button::new(format!("{} Settings...", icons::SETTINGS)))
                .on_hover_text("Open Export Settings dialog");
            if inspect_btn.clicked()
                && let Some(ref id) = state.selected_job_id
                && let Some(j) = jobs.iter().find(|j| &j.spec.job_id == id)
            {
                *on_inspect_job = Some(j.spec.clone());
            }

            ui.separator();

            let has_completed = jobs.iter().any(|j| j.status == JobStatus::Completed);
            let clear_btn = ui.add_enabled(has_completed, egui::Button::new("Clear Finished")).on_hover_text("Remove finished jobs from queue");
            if clear_btn.clicked() {
                let _ = queue_mgr.clear_completed_jobs();
            }
        });

        ui.add_space(4.0);

        // --- 2. Batch Progress Summary Bar (AME Parity) ---
        if !jobs.is_empty() {
            let total_jobs = jobs.len();
            let queued_count = jobs.iter().filter(|j| matches!(j.status, JobStatus::Queued)).count();
            let processing_count = jobs.iter().filter(|j| matches!(j.status, JobStatus::Processing)).count();
            let completed_count = jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed)).count();
            let failed_count = jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed(_))).count();

            let sum_pct: f64 = jobs
                .iter()
                .map(|j| match j.status {
                    JobStatus::Completed => 100.0,
                    JobStatus::Processing => j.progress_percent,
                    _ => 0.0,
                })
                .sum();
            let overall_progress = (sum_pct / (total_jobs as f64 * 100.0)).clamp(0.0, 1.0) as f32;

            egui::Frame::new().fill(tokens.bg_panel_darker).corner_radius(4).inner_margin(egui::vec2(8.0, 4.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Batch Queue: {total_jobs} total")).strong().small());
                    if queued_count > 0 {
                        ui.label(RichText::new(format!("· {queued_count} ready")).small().color(tokens.status_queued));
                    }
                    if processing_count > 0 {
                        ui.label(RichText::new(format!("· {processing_count} encoding")).small().color(tokens.status_processing));
                    }
                    if completed_count > 0 {
                        ui.label(RichText::new(format!("· {completed_count} done")).small().color(tokens.status_completed));
                    }
                    if failed_count > 0 {
                        ui.label(RichText::new(format!("· {failed_count} failed")).small().color(tokens.status_failed));
                    }
                    if selected_count > 0 {
                        ui.label(RichText::new(format!("({selected_count} selected)")).small().color(tokens.accent_primary));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let pbar = egui::ProgressBar::new(overall_progress).show_percentage().desired_width(140.0);
                        ui.add(pbar);
                        ui.label(RichText::new("Overall:").small().color(tokens.text_dim));
                    });
                });
            });
            ui.add_space(2.0);
        }

        ui.separator();

        // --- 3. Hierarchical Job Table ---
        if jobs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("The transcode queue is empty.").color(tokens.text_dim));
                ui.label(
                    RichText::new("Drag and drop media files, use File > Add Media Files..., or pick from Presets to begin.").color(tokens.text_dim),
                );
            });
            return;
        }

        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            for (idx, job) in jobs.iter().enumerate() {
                let item = JobRowItem { job, idx, all_jobs: &jobs };
                render_job_row(ui, item, state, queue_mgr, tokens, on_inspect_job);
            }
        });
    });
}

struct JobRowItem<'a> {
    job: &'a QueuedJob,
    idx: usize,
    all_jobs: &'a [QueuedJob],
}

fn render_job_row(
    ui: &mut Ui,
    item: JobRowItem<'_>,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    let job = item.job;
    let job_idx = item.idx;
    let all_jobs = item.all_jobs;
    let job_id = job.spec.job_id.clone();
    let is_selected = state.selected_job_ids.contains(&job_id) || state.selected_job_id.as_deref() == Some(&job_id);
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

            // Client Origin Chip (FilmCraft, EffectCraft, SoundCraft, PhotoCraft)
            let client_app = &job.spec.client_info.app_name;
            if !client_app.is_empty() && client_app != "encoderCraft GUI" && client_app != "encoderCraft CLI" {
                let chip_color = match client_app.to_lowercase().as_str() {
                    s if s.contains("film") => egui::Color32::from_rgb(99, 102, 241),
                    s if s.contains("effect") => egui::Color32::from_rgb(168, 85, 247),
                    s if s.contains("sound") => egui::Color32::from_rgb(34, 197, 94),
                    s if s.contains("photo") => egui::Color32::from_rgb(59, 130, 246),
                    _ => tokens.text_dim,
                };
                ui.label(RichText::new(format!("[{client_app}]")).small().color(chip_color));
            }

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
                let cmd_held = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                let shift_held = ui.input(|i| i.modifiers.shift);

                if shift_held {
                    state.range_select(all_jobs, job_idx);
                } else if cmd_held {
                    state.toggle_select(job_id.clone(), job_idx);
                } else {
                    state.select_single(job_id.clone(), job_idx);
                }
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

                        // Preset name clickable link
                        if ui
                            .link(RichText::new(&target.preset_name).color(tokens.accent_primary))
                            .on_hover_text("Inspect settings for this target")
                            .clicked()
                        {
                            *on_inspect_job = Some(job.spec.clone());
                        }

                        // Container format tag
                        ui.label(RichText::new(format!("{:?}", target.container)).small().color(tokens.text_dim));

                        // Output filename
                        let path_display = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                        ui.label(RichText::new(format!("➔ {path_display}")).small().color(tokens.text_secondary));

                        // Actions per target
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if job.spec.targets.len() > 1 && ui.small_button("🗑").on_hover_text("Remove output target").clicked() {
                                let _ = queue_mgr.remove_output_target(&job_id, idx);
                            }
                            if ui.small_button("⎘").on_hover_text("Duplicate output target").clicked() {
                                let _ = queue_mgr.duplicate_output_target(&job_id, idx);
                            }
                        });
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

    let chip = egui::Frame::new()
        .fill(color.gamma_multiply(0.2))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(3)
        .inner_margin(egui::vec2(5.0, 1.5))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });

    if let JobStatus::Failed(err) = status {
        chip.response.on_hover_text(format!("Failure detail: {err}"));
    }
}
