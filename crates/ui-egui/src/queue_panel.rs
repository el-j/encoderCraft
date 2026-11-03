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
    pub active_action_menu: Option<String>,
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

/// Reveals a file or directory in the platform's default file manager (AME Parity).
#[allow(clippy::zombie_processes)]
pub fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,\"{}\"", path.display())).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

/// Renders the queue panel with batch toolbar, global progress, and hierarchical job table.
pub fn render_queue_panel(
    ui: &mut Ui,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
    on_open_log: &mut bool,
) {
    let jobs = queue_mgr.list_jobs();

    ui.vertical(|ui| {
        // --- Keyboard shortcuts for AME parity ---
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            queue_mgr.start_queue();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            queue_mgr.stop_worker();
        }
        state.is_running = queue_mgr.is_worker_running() && !queue_mgr.is_paused();
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
            let start_btn =
                icons::button_with_label(ui, icons::Icon::Play, "Start Queue", state.is_running, tokens, "Start processing the batch queue (Enter)");
            if start_btn.clicked() {
                queue_mgr.start_queue();
            }

            let pause_btn = icons::button_with_label(ui, icons::Icon::Pause, "Pause", false, tokens, "Pause active encode");
            if pause_btn.clicked() {
                queue_mgr.pause_worker();
            }

            let stop_btn = icons::button_with_label(ui, icons::Icon::Stop, "Stop", false, tokens, "Stop batch queue (Esc)");
            if stop_btn.clicked() {
                queue_mgr.stop_worker();
            }

            ui.separator();

            ui.add_enabled_ui(has_selection, |ui| {
                let add_output_btn =
                    icons::button_with_label(ui, icons::Icon::Plus, "Add Output", false, tokens, "Add another output target to selected job(s)");
                if add_output_btn.clicked() {
                    for id in state.selected_ids_list() {
                        if let Some(j) = jobs.iter().find(|j| j.spec.job_id == id) {
                            let count = j.spec.targets.len() + 1;
                            let stream_kind = match &j.spec.source {
                                encodercraft_core::SourceDescriptor::File { path, .. } => encodercraft_demux::classify_stream_kind(path),
                                _ => encodercraft_core::MediaStreamKind::VideoWithAudio,
                            };

                            let (ext, container, preset_name, cw, ch, br, lufs) = match stream_kind {
                                encodercraft_core::MediaStreamKind::AudioOnly => (
                                    "wav",
                                    encodercraft_core::ContainerType::Wave,
                                    "Broadcast Wave BWF (-23 LUFS)".to_string(),
                                    None,
                                    None,
                                    None,
                                    Some(-23.0),
                                ),
                                encodercraft_core::MediaStreamKind::Vector2D => {
                                    ("svg", encodercraft_core::ContainerType::Svg, "Web Vector SVG (Optimized)".to_string(), None, None, None, None)
                                }
                                encodercraft_core::MediaStreamKind::Document => {
                                    ("pdf", encodercraft_core::ContainerType::Pdf, "Print-Ready PDF/X-4".to_string(), None, None, None, None)
                                }
                                encodercraft_core::MediaStreamKind::Spatial3D => {
                                    ("glb", encodercraft_core::ContainerType::Gltf, "glTF 2.0 Binary (.glb)".to_string(), None, None, None, None)
                                }
                                encodercraft_core::MediaStreamKind::StillImage => {
                                    ("png", encodercraft_core::ContainerType::Png, "Still Image PNG (Lossless)".to_string(), None, None, None, None)
                                }
                                _ => (
                                    "mp4",
                                    encodercraft_core::ContainerType::IsoBmffMp4,
                                    "Match Source - High Bitrate".to_string(),
                                    Some(1920),
                                    Some(1080),
                                    Some(12000),
                                    Some(-14.0),
                                ),
                            };

                            let new_target = encodercraft_core::OutputTarget {
                                target_id: format!("target-{count}"),
                                output_path: std::path::PathBuf::from(format!("output_{count}.{ext}")),
                                preset_name,
                                container,
                                custom_width: cw,
                                custom_height: ch,
                                custom_frame_rate: None,
                                target_bitrate_kbps: br,
                                target_lufs: lufs,
                                ..Default::default()
                            };
                            let _ = queue_mgr.add_output_target(&id, new_target);
                            state.expanded_jobs.insert(id);
                        }
                    }
                }

                let dup_label = if selected_count > 1 { format!("Duplicate ({selected_count})") } else { "Duplicate".into() };
                let dup_btn = icons::button_with_label(ui, icons::Icon::Duplicate, &dup_label, false, tokens, "Duplicate selected job(s) (Cmd+D)");
                if dup_btn.clicked() {
                    let ids = state.selected_ids_list();
                    for id in ids {
                        let _ = queue_mgr.duplicate_job(&id);
                    }
                }

                let move_up_btn = icons::button(ui, icons::Icon::ArrowUp, 20.0, false, tokens, "Move selected up in queue (Alt+Up)");
                if move_up_btn.clicked() {
                    for id in state.selected_ids_list() {
                        let _ = queue_mgr.move_job_up(&id);
                    }
                }

                let move_down_btn = icons::button(ui, icons::Icon::ArrowDown, 20.0, false, tokens, "Move selected down in queue (Alt+Down)");
                if move_down_btn.clicked() {
                    let mut ids = state.selected_ids_list();
                    ids.reverse();
                    for id in ids {
                        let _ = queue_mgr.move_job_down(&id);
                    }
                }

                let reset_label = if selected_count > 1 { format!("Reset ({selected_count})") } else { "Reset".into() };
                let reset_btn = icons::button_with_label(ui, icons::Icon::Reset, &reset_label, false, tokens, "Reset selected job(s) back to Ready");
                if reset_btn.clicked() {
                    for id in state.selected_ids_list() {
                        let _ = queue_mgr.reset_job_status(&id);
                    }
                }

                let remove_label = if selected_count > 1 { format!("Remove ({selected_count})") } else { "Remove".into() };
                let remove_btn = icons::button_with_label(ui, icons::Icon::Trash, &remove_label, false, tokens, "Remove selected job(s) (Delete)");
                if remove_btn.clicked() {
                    let ids = state.selected_ids_list();
                    for id in ids {
                        let _ = queue_mgr.remove_job(&id);
                    }
                    state.clear_selection();
                }

                let inspect_btn = icons::button_with_label(ui, icons::Icon::Gear, "Settings...", false, tokens, "Open Export Settings dialog");
                if inspect_btn.clicked()
                    && let Some(ref id) = state.selected_job_id
                    && let Some(j) = jobs.iter().find(|j| &j.spec.job_id == id)
                {
                    *on_inspect_job = Some(j.spec.clone());
                }
            });

            ui.separator();

            let has_completed = jobs.iter().any(|j| j.status == JobStatus::Completed);
            ui.add_enabled_ui(has_completed, |ui| {
                if ui.button("Clear Finished").on_hover_text("Remove finished jobs from queue").clicked() {
                    let _ = queue_mgr.clear_completed_jobs();
                }
            });

            if selected_count > 1 {
                let stitch_label = format!("Stitch Selected ({selected_count})");
                let stitch_btn = icons::button_with_label(
                    ui,
                    icons::Icon::Link,
                    &stitch_label,
                    false,
                    tokens,
                    "Combine selected jobs into one continuous stitched sequence",
                );
                if stitch_btn.clicked() {
                    let ids = state.selected_ids_list();
                    if let Some(first_job) = jobs.iter().find(|j| ids.contains(&j.spec.job_id))
                        && let Some(target) = first_job.spec.targets.first()
                    {
                        let out_path = target.output_path.with_file_name("stitched_batch.mp4");
                        let _ = queue_mgr.stitch_selected_jobs(&ids, out_path, "Match Source - High Bitrate".into());
                    }
                }
            }

            ui.add_enabled_ui(has_selection, |ui| {
                let set_dir_btn = icons::button_with_label(
                    ui,
                    icons::Icon::Folder,
                    "Destination...",
                    false,
                    tokens,
                    "Batch set output destination directory for selected job(s)",
                );
                if set_dir_btn.clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_title("Select Destination Folder").pick_folder()
                {
                    let ids = state.selected_ids_list();
                    let _ = queue_mgr.set_jobs_output_dir(&ids, folder);
                }
            });

            let log_btn = icons::button_with_label(ui, icons::Icon::Clock, "Show Log...", false, tokens, "Open Encoding Activity Log (Cmd+L)");
            if log_btn.clicked() {
                *on_open_log = true;
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
            let arrow_icon = if is_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
            if icons::button(ui, arrow_icon, 16.0, false, tokens, "Toggle output targets").clicked() {
                if is_expanded {
                    state.expanded_jobs.remove(&job_id);
                } else {
                    state.expanded_jobs.insert(job_id.clone());
                }
            }

            // Status chip
            render_status_chip(ui, &job.status, tokens);

            // Client Origin Chip (All 14 ArtCraft Sibling Applications — el-j/encoderCraft#1)
            render_client_origin_chip(ui, &job.spec.client_info.app_name, tokens);

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

            let display_source_name = if source_name.chars().count() > 38 {
                let prefix: String = source_name.chars().take(35).collect();
                format!("{prefix}…")
            } else {
                source_name.to_string()
            };

            let job_select_btn = ui.selectable_label(is_selected, RichText::new(display_source_name).strong()).on_hover_text(source_name);
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

            job_select_btn.context_menu(|ui| {
                let out_path = job.spec.targets.first().map(|t| t.output_path.clone());
                let src_path = match &job.spec.source {
                    encodercraft_core::SourceDescriptor::File { path, .. } => Some(path.clone()),
                    _ => None,
                };

                if let Some(ref p) = out_path {
                    if ui.button("Reveal Output in Finder / Explorer").clicked() {
                        reveal_in_file_manager(p);
                        ui.close();
                    }
                    if ui.button("Copy Output Path").clicked() {
                        ui.ctx().copy_text(p.display().to_string());
                        ui.close();
                    }
                }

                if let Some(ref p) = src_path {
                    if ui.button("Reveal Source in Finder / Explorer").clicked() {
                        reveal_in_file_manager(p);
                        ui.close();
                    }
                    if ui.button("Copy Source Path").clicked() {
                        ui.ctx().copy_text(p.display().to_string());
                        ui.close();
                    }
                }

                ui.separator();

                if ui.button("Export Settings...").clicked() {
                    *on_inspect_job = Some(job.spec.clone());
                    ui.close();
                }

                if ui.button("Duplicate").clicked() {
                    let _ = queue_mgr.duplicate_job(&job_id);
                    ui.close();
                }

                if ui.button("Reset Status").clicked() {
                    let _ = queue_mgr.reset_job_status(&job_id);
                    ui.close();
                }

                if ui.button("Remove").clicked() {
                    let _ = queue_mgr.remove_job(&job_id);
                    ui.close();
                }

                if state.selected_job_ids.len() > 1 {
                    ui.separator();
                    if ui.button(format!("Stitch {} Selected Clips...", state.selected_job_ids.len())).clicked() {
                        if let Some(ref first_path) = out_path {
                            let out = first_path.with_file_name("stitched_batch.mp4");
                            let ids = state.selected_ids_list();
                            let _ = queue_mgr.stitch_selected_jobs(&ids, out, "Match Source - High Bitrate".into());
                        }
                        ui.close();
                    }
                }
            });

            // Preset Name
            if let Some(target) = job.spec.targets.first() {
                ui.label(RichText::new(format!("-> {}", target.preset_name)).color(tokens.text_secondary));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Actions menu with boundary clamping and upward flipping
                let menu_id = ui.make_persistent_id(format!("job_row_actions_menu_{job_id}"));
                let is_menu_open = state.active_action_menu.as_deref() == Some(&job_id);
                let btn = ui.add(egui::Button::new(RichText::new("...").size(14.0)).frame(true));
                if btn.clicked() {
                    state.active_action_menu = if is_menu_open { None } else { Some(job_id.clone()) };
                }

                if is_menu_open {
                    let screen_rect = ui.ctx().input(|i| i.raw.screen_rect).unwrap_or_else(|| ui.max_rect());
                    let menu_width = 180.0;
                    let menu_height = 170.0;

                    let x = (btn.rect.max.x - menu_width).clamp(screen_rect.min.x + 8.0, screen_rect.max.x - menu_width - 8.0);

                    let y = if btn.rect.max.y + menu_height > screen_rect.max.y - 12.0 {
                        (btn.rect.min.y - menu_height).max(screen_rect.min.y + 8.0)
                    } else {
                        btn.rect.max.y + 2.0
                    };

                    let popup_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(menu_width, menu_height));

                    let close_menu = ui.input(|i| {
                        (i.pointer.any_click() && i.pointer.interact_pos().map(|p| !popup_rect.contains(p) && !btn.rect.contains(p)).unwrap_or(false))
                            || i.key_pressed(egui::Key::Escape)
                    });

                    if close_menu {
                        state.active_action_menu = None;
                    } else {
                        egui::Area::new(menu_id).order(egui::Order::Foreground).fixed_pos(egui::pos2(x, y)).show(ui.ctx(), |ui| {
                            egui::Frame::menu(ui.style())
                                .fill(tokens.bg_panel_darker)
                                .stroke(egui::Stroke::new(1.0, tokens.border_subtle))
                                .corner_radius(6)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.set_width(menu_width - 12.0);
                                    if ui.button("Inspect Settings...").clicked() {
                                        *on_inspect_job = Some(job.spec.clone());
                                        state.active_action_menu = None;
                                    }
                                    if ui.button("Duplicate").clicked() {
                                        let _ = queue_mgr.duplicate_job(&job_id);
                                        state.active_action_menu = None;
                                    }
                                    if ui.button("Start Now").clicked() {
                                        let _ = queue_mgr.start_job(&job_id);
                                        queue_mgr.start_queue();
                                        state.active_action_menu = None;
                                    }
                                    if ui.button("Pause").clicked() {
                                        let _ = queue_mgr.pause_job(&job_id);
                                        state.active_action_menu = None;
                                    }
                                    if ui.button("Remove").clicked() {
                                        let _ = queue_mgr.remove_job(&job_id);
                                        state.active_action_menu = None;
                                    }
                                });
                        });
                    }
                }

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
                        let target_link = ui
                            .link(RichText::new(&target.preset_name).color(tokens.accent_primary))
                            .on_hover_text("Inspect settings for this target");
                        if target_link.clicked() {
                            *on_inspect_job = Some(job.spec.clone());
                        }

                        target_link.context_menu(|ui| {
                            if ui.button("Reveal Output in Finder / Explorer").clicked() {
                                reveal_in_file_manager(&target.output_path);
                                ui.close();
                            }
                            if ui.button("Copy Output Path").clicked() {
                                ui.ctx().copy_text(target.output_path.display().to_string());
                                ui.close();
                            }
                            if ui.button("Inspect Settings...").clicked() {
                                *on_inspect_job = Some(job.spec.clone());
                                ui.close();
                            }
                            if ui.button("Duplicate Target").clicked() {
                                let _ = queue_mgr.duplicate_output_target(&job_id, idx);
                                ui.close();
                            }
                            if job.spec.targets.len() > 1 && ui.button("Remove Target").clicked() {
                                let _ = queue_mgr.remove_output_target(&job_id, idx);
                                ui.close();
                            }
                        });

                        // Container format tag
                        ui.label(RichText::new(format!("{:?}", target.container)).small().color(tokens.text_dim));

                        // Output filename
                        let path_display = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                        let display_out_name = if path_display.chars().count() > 32 {
                            let prefix: String = path_display.chars().take(29).collect();
                            format!("{prefix}…")
                        } else {
                            path_display.to_string()
                        };
                        ui.label(RichText::new(format!("-> {display_out_name}")).small().color(tokens.text_secondary))
                            .on_hover_text(target.output_path.display().to_string());

                        // Actions per target
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if job.spec.targets.len() > 1
                                && icons::button(ui, icons::Icon::Trash, 16.0, false, tokens, "Remove output target").clicked()
                            {
                                let _ = queue_mgr.remove_output_target(&job_id, idx);
                            }
                            if icons::button(ui, icons::Icon::Duplicate, 16.0, false, tokens, "Duplicate output target").clicked() {
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

fn render_client_origin_chip(ui: &mut Ui, client_app: &str, tokens: &Tokens) {
    if !client_app.is_empty() && client_app != "encoderCraft GUI" && client_app != "encoderCraft CLI" {
        let chip_color = match client_app.to_lowercase().as_str() {
            s if s.contains("film") => egui::Color32::from_rgb(99, 102, 241), // FilmCraft (Indigo)
            s if s.contains("effect") => egui::Color32::from_rgb(168, 85, 247), // EffectCraft (Purple)
            s if s.contains("sound") => egui::Color32::from_rgb(34, 197, 94), // SoundCraft (Emerald)
            s if s.contains("photo") => egui::Color32::from_rgb(59, 130, 246), // PhotoCraft (Blue)
            s if s.contains("vector") => egui::Color32::from_rgb(249, 115, 22), // VectorCraft (Orange)
            s if s.contains("design") => egui::Color32::from_rgb(236, 72, 153), // DesignCraft (Magenta/Pink)
            s if s.contains("pdf") => egui::Color32::from_rgb(239, 68, 68),   // PdfCraft (Crimson Red)
            s if s.contains("fig") => egui::Color32::from_rgb(20, 184, 166),  // FigCraft (Teal)
            s if s.contains("cad") => egui::Color32::from_rgb(6, 182, 212),   // CadCraft (Cyan)
            s if s.contains("deck") => egui::Color32::from_rgb(245, 158, 11), // DeckCraft (Amber)
            s if s.contains("solid") => egui::Color32::from_rgb(14, 165, 233), // SolidCraft (Sky Blue)
            s if s.contains("word") => egui::Color32::from_rgb(37, 99, 235),  // WordCraft (Royal Blue)
            s if s.contains("texture") => egui::Color32::from_rgb(217, 119, 6), // TextureCraft (Warm Ochre)
            s if s.contains("material") => egui::Color32::from_rgb(180, 83, 9), // MaterialCraft (Copper Bronze)
            s if s.contains("ani") => egui::Color32::from_rgb(244, 63, 94),   // AniCraft (Coral Rose)
            s if s.contains("grid") => egui::Color32::from_rgb(16, 185, 129), // GridCraft (Mint Green)
            s if s.contains("light") => egui::Color32::from_rgb(139, 92, 246), // LightCraft (Violet)
            _ => tokens.text_dim,
        };
        ui.label(RichText::new(format!("[{client_app}]")).small().color(chip_color));
    }
}
