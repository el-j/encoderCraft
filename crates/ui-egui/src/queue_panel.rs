//! Queue panel: hierarchical data table showing queued sources, output targets, progress, and batch controls.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::{ContainerType, CraftJobSpec, JobPriority, JobStatus, OutputTarget, SourceDescriptor};
use encodercraft_engine::{QueueManager, QueuedJob};

use crate::icons;
use crate::theme::Tokens;

/// Column identifiers for sortable queue table headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueSortColumn {
    Source,
    Format,
    Preset,
    Output,
    Status,
}

/// Sorting direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

/// State for the queue panel widget supporting full multi-job batch operations (AME Parity).
#[derive(Default)]
pub struct QueuePanelState {
    pub selected_job_id: Option<String>,
    pub selected_job_ids: std::collections::HashSet<String>,
    pub expanded_jobs: std::collections::HashSet<String>,
    pub is_running: bool,
    pub last_clicked_index: Option<usize>,
    pub active_action_menu: Option<String>,
    pub active_preset_menu: Option<(String, usize)>, // (job_id, target_idx)
    pub sort_column: Option<QueueSortColumn>,
    pub sort_direction: SortDirection,
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

    /// Toggles sorting on a given column.
    pub fn toggle_sort(&mut self, col: QueueSortColumn) {
        if self.sort_column == Some(col) {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = Some(col);
            self.sort_direction = SortDirection::Ascending;
        }
    }
}

/// Truncates a string in the middle with an ellipsis ('…') if it exceeds `max_len` unicode chars.
pub fn middle_truncate(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_len || max_len < 5 {
        return text.to_string();
    }
    let keep_side = (max_len.saturating_sub(1)) / 2;
    let head: String = text.chars().take(keep_side).collect();
    let tail: String = text.chars().skip(char_count.saturating_sub(keep_side)).collect();
    format!("{head}…{tail}")
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

/// Available containers for inline dropdown selection.
pub const INLINE_CONTAINERS: &[(ContainerType, &str, &str)] = &[
    (ContainerType::IsoBmffMp4, "MP4", "mp4"),
    (ContainerType::QuickTimeMov, "MOV (QuickTime)", "mov"),
    (ContainerType::WebM, "WebM", "webm"),
    (ContainerType::MatroskaMkv, "MKV", "mkv"),
    (ContainerType::MxfOp1a, "MXF OP1a", "mxf"),
    (ContainerType::Wave, "WAV (BWF)", "wav"),
    (ContainerType::Aiff, "AIFF", "aif"),
    (ContainerType::Flac, "FLAC", "flac"),
    (ContainerType::Mp3, "MP3", "mp3"),
    (ContainerType::Png, "PNG", "png"),
    (ContainerType::Jpeg, "JPEG", "jpg"),
    (ContainerType::WebP, "WebP", "webp"),
    (ContainerType::Tiff, "TIFF", "tif"),
    (ContainerType::OpenExr, "OpenEXR", "exr"),
    (ContainerType::AnimatedGif, "GIF", "gif"),
];

/// Returns the default preset name for a given container format.
pub fn default_preset_for_container(c: ContainerType) -> &'static str {
    match c {
        ContainerType::IsoBmffMp4 => "Match Source - High Bitrate",
        ContainerType::QuickTimeMov => "Apple ProRes 422 HQ",
        ContainerType::WebM => "YouTube 1080p Full HD",
        ContainerType::MatroskaMkv => "Match Source - High Bitrate",
        ContainerType::MxfOp1a | ContainerType::MxfOpAtom => "Broadcast MXF OP1a (DNxHD / PCM)",
        ContainerType::Wave => "Broadcast Wave BWF (-23 LUFS)",
        ContainerType::Aiff => "Lossless Audio Master (AIFF 24-bit)",
        ContainerType::Flac => "Archival Audio FLAC (Lossless 24-bit)",
        ContainerType::Mp3 => "Podcast MP3 (192 kbps)",
        ContainerType::Png => "Still Image PNG (Lossless)",
        ContainerType::Jpeg => "Still Image JPEG",
        ContainerType::WebP => "Still Image WebP (Lossless)",
        ContainerType::Tiff => "Broadcast TIFF (16-bit RGB)",
        ContainerType::OpenExr => "VFX OpenEXR (Half Float)",
        ContainerType::Pdf | ContainerType::Docx | ContainerType::Idml | ContainerType::AdobeInDesignIndd | ContainerType::ArtcraftDesigncraft => {
            "Print-Ready PDF/X-4"
        }
        ContainerType::Svg
        | ContainerType::PostScriptEps
        | ContainerType::AdobeIllustratorAi
        | ContainerType::ArtcraftVectorcraft
        | ContainerType::AutocadDxf
        | ContainerType::AutocadDwg
        | ContainerType::HpglPlotter
        | ContainerType::ArtcraftCadcraft => "Web Vector SVG (Optimized)",
        ContainerType::AdobePhotoshopPsd | ContainerType::AdobePhotoshopPsb | ContainerType::ArtcraftPhotocraft => "Broadcast TIFF (16-bit RGB)",
        ContainerType::AdobeAfterEffectsAep | ContainerType::ArtcraftEffectcraft => "Apple ProRes 422 HQ",
        ContainerType::AdobePremierePrproj | ContainerType::ArtcraftFilmcraft => "Broadcast MXF OP1a (DNxHD / PCM)",
        ContainerType::AdobeAuditionSesx | ContainerType::ArtcraftSoundcraft => "Broadcast Wave BWF (-23 LUFS)",
        _ => "Match Source - High Bitrate",
    }
}

/// Returns matching preset names for a container, offering only presets whose codec the container can store.
pub fn presets_for_container(c: ContainerType) -> Vec<&'static str> {
    let registry = encodercraft_presets::PresetRegistry::default();
    candidate_presets_for_container(c)
        .into_iter()
        .filter(|name| {
            let codec = registry.find_by_name(name).and_then(|p| p.video_encode_settings().codec);
            codec.is_none_or(|id| {
                encodercraft_video_codecs::VideoCodecKind::parse(&id)
                    .is_some_and(|k| encodercraft_video_codecs::capabilities::container_accepts(c, k))
            })
        })
        .collect()
}

fn candidate_presets_for_container(c: ContainerType) -> Vec<&'static str> {
    match c {
        ContainerType::IsoBmffMp4 => vec![
            "Match Source - High Bitrate",
            "YouTube 4K Ultra HD",
            "YouTube 1080p Full HD",
            "Social Media Square (1080x1080)",
            "Social Media Vertical 9:16 (1080x1920)",
        ],
        ContainerType::QuickTimeMov => vec!["Apple ProRes 422 HQ", "Apple ProRes 422", "Apple ProRes 4444", "Avid DNxHR HQ 1080p"],
        ContainerType::MxfOp1a | ContainerType::MxfOpAtom => vec!["Broadcast MXF OP1a (DNxHD / PCM)"],
        ContainerType::Wave
        | ContainerType::Aiff
        | ContainerType::Flac
        | ContainerType::Mp3
        | ContainerType::OggAudio
        | ContainerType::AdobeAuditionSesx
        | ContainerType::ArtcraftSoundcraft => vec![
            "Broadcast Wave BWF (-23 LUFS)",
            "Lossless Audio Master (AIFF 24-bit)",
            "Archival Audio FLAC (Lossless 24-bit)",
            "Podcast MP3 (192 kbps)",
        ],
        ContainerType::Png
        | ContainerType::Jpeg
        | ContainerType::WebP
        | ContainerType::Tiff
        | ContainerType::OpenExr
        | ContainerType::Bmp
        | ContainerType::AdobePhotoshopPsd
        | ContainerType::AdobePhotoshopPsb
        | ContainerType::ArtcraftPhotocraft => vec![
            "Still Image PNG (Lossless)",
            "Still Image JPEG",
            "Still Image WebP (Lossless)",
            "Broadcast TIFF (16-bit RGB)",
            "VFX OpenEXR (Half Float)",
            "Windows Bitmap BMP (Uncompressed)",
        ],
        ContainerType::Pdf | ContainerType::Docx | ContainerType::Idml | ContainerType::AdobeInDesignIndd | ContainerType::ArtcraftDesigncraft => {
            vec!["Print-Ready PDF/X-4"]
        }
        ContainerType::Svg
        | ContainerType::PostScriptEps
        | ContainerType::AdobeIllustratorAi
        | ContainerType::ArtcraftVectorcraft
        | ContainerType::AutocadDxf
        | ContainerType::AutocadDwg
        | ContainerType::HpglPlotter
        | ContainerType::ArtcraftCadcraft => vec!["Web Vector SVG (Optimized)"],
        ContainerType::AdobeAfterEffectsAep | ContainerType::ArtcraftEffectcraft => {
            vec!["Apple ProRes 422 HQ", "YouTube 1080p Full HD"]
        }
        ContainerType::AdobePremierePrproj | ContainerType::ArtcraftFilmcraft => {
            vec!["Broadcast MXF OP1a (DNxHD / PCM)", "Apple ProRes 422 HQ"]
        }
        _ => vec!["Match Source - High Bitrate"],
    }
}

/// Helper to get the display name of a job source.
pub fn job_source_name(job: &QueuedJob) -> String {
    match &job.spec.source {
        SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("File").to_string(),
        SourceDescriptor::StitchedClips { .. } => "Stitched Sequence".to_string(),
        SourceDescriptor::Pipe { endpoint, .. } => endpoint.clone(),
        SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.clone(),
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
    let mut jobs = queue_mgr.list_jobs();

    // Handle drag and drop files onto the queue table
    let dropped_files = ui.ctx().input(|i| i.raw.dropped_files.clone());
    for file in dropped_files {
        let path = file.path().to_path_buf();
        if path.is_file() {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
            let out_path = parent.join(format!("{stem}_encoded.mp4"));

            let spec = CraftJobSpec {
                version: "1.0".into(),
                job_id: format!("job_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)),
                client_info: encodercraft_core::ClientInfo {
                    app_name: "encoderCraft GUI".into(),
                    app_version: "0.1.0-alpha.2".into(),
                    pid: std::process::id(),
                    user_notes: None,
                },
                priority: JobPriority::Normal,
                start_policy: encodercraft_core::StartPolicy::AutoStart,
                source: SourceDescriptor::File { path: path.clone(), start_tick: None, duration_ticks: None, timebase: None },
                targets: vec![OutputTarget {
                    target_id: "target-1".into(),
                    output_path: out_path,
                    preset_name: "Match Source - High Bitrate".into(),
                    container: ContainerType::IsoBmffMp4,
                    ..Default::default()
                }],
            };
            let _ = queue_mgr.submit_job(spec);
        }
    }

    // Apply sorting if a sort column is set
    if let Some(col) = state.sort_column {
        jobs.sort_by(|a, b| {
            let ord = match col {
                QueueSortColumn::Source => job_source_name(a).to_lowercase().cmp(&job_source_name(b).to_lowercase()),
                QueueSortColumn::Format => {
                    let fa = a.spec.targets.first().map(|t| format!("{:?}", t.container)).unwrap_or_default();
                    let fb = b.spec.targets.first().map(|t| format!("{:?}", t.container)).unwrap_or_default();
                    fa.cmp(&fb)
                }
                QueueSortColumn::Preset => {
                    let pa = a.spec.targets.first().map(|t| &t.preset_name).unwrap_or(&a.spec.job_id);
                    let pb = b.spec.targets.first().map(|t| &t.preset_name).unwrap_or(&b.spec.job_id);
                    pa.cmp(pb)
                }
                QueueSortColumn::Output => {
                    let oa = a.spec.targets.first().map(|t| t.output_path.to_string_lossy()).unwrap_or_default();
                    let ob = b.spec.targets.first().map(|t| t.output_path.to_string_lossy()).unwrap_or_default();
                    oa.cmp(&ob)
                }
                QueueSortColumn::Status => format!("{:?}", a.status).cmp(&format!("{:?}", b.status)),
            };
            match state.sort_direction {
                SortDirection::Ascending => ord,
                SortDirection::Descending => ord.reverse(),
            }
        });
    }

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
                            let new_target = OutputTarget {
                                target_id: format!("target-{count}"),
                                output_path: std::path::PathBuf::from(format!("output_{count}.mp4")),
                                preset_name: "Match Source - High Bitrate".into(),
                                container: ContainerType::IsoBmffMp4,
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

            let overall_progress = overall_fraction(&jobs);

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

        // --- 3. Hierarchical Job Table with Columns ---
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

        // Table Header
        render_table_header(ui, state, tokens);

        ui.separator();

        // Table Rows Scroll Area
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            for (idx, job) in jobs.iter().enumerate() {
                let item = GroupedJobItem { job, job_idx: idx, all_jobs: &jobs };
                render_grouped_job_row(ui, item, state, queue_mgr, tokens, on_inspect_job);
            }
        });
    });
}

fn sort_indicator(state: &QueuePanelState, col: QueueSortColumn) -> &'static str {
    if state.sort_column == Some(col) {
        match state.sort_direction {
            SortDirection::Ascending => " ▲",
            SortDirection::Descending => " ▼",
        }
    } else {
        ""
    }
}

fn render_table_header(ui: &mut Ui, state: &mut QueuePanelState, tokens: &Tokens) {
    egui::Frame::new().fill(tokens.bg_panel_darker).inner_margin(egui::vec2(6.0, 4.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            // Source Column (expands)
            let src_label = format!("Source Name{}", sort_indicator(state, QueueSortColumn::Source));
            let src_resp = ui.add_sized(
                [ui.available_width() - 650.0_f32.min(ui.available_width() * 0.65), 18.0],
                egui::Button::new(RichText::new(src_label).strong().small()).frame(false),
            );
            if src_resp.clicked() {
                state.toggle_sort(QueueSortColumn::Source);
            }

            // Format Column
            let fmt_label = format!("Format{}", sort_indicator(state, QueueSortColumn::Format));
            let fmt_resp = ui.add_sized([110.0, 18.0], egui::Button::new(RichText::new(fmt_label).strong().small()).frame(false));
            if fmt_resp.clicked() {
                state.toggle_sort(QueueSortColumn::Format);
            }

            // Preset Column
            let pre_label = format!("Preset{}", sort_indicator(state, QueueSortColumn::Preset));
            let pre_resp = ui.add_sized([190.0, 18.0], egui::Button::new(RichText::new(pre_label).strong().small()).frame(false));
            if pre_resp.clicked() {
                state.toggle_sort(QueueSortColumn::Preset);
            }

            // Output File Column
            let out_label = format!("Output File{}", sort_indicator(state, QueueSortColumn::Output));
            let out_resp = ui.add_sized([220.0, 18.0], egui::Button::new(RichText::new(out_label).strong().small()).frame(false));
            if out_resp.clicked() {
                state.toggle_sort(QueueSortColumn::Output);
            }

            // Status Column
            let st_label = format!("Status{}", sort_indicator(state, QueueSortColumn::Status));
            let st_resp = ui.add_sized([120.0, 18.0], egui::Button::new(RichText::new(st_label).strong().small()).frame(false));
            if st_resp.clicked() {
                state.toggle_sort(QueueSortColumn::Status);
            }
        });
    });
}

struct GroupedJobItem<'a> {
    pub job: &'a QueuedJob,
    pub job_idx: usize,
    pub all_jobs: &'a [QueuedJob],
}

fn render_grouped_job_row(
    ui: &mut Ui,
    item: GroupedJobItem<'_>,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    tokens: &Tokens,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    let job = item.job;
    let job_idx = item.job_idx;
    let all_jobs = item.all_jobs;
    let job_id = job.spec.job_id.clone();
    let is_selected = state.selected_job_ids.contains(&job_id) || state.selected_job_id.as_deref() == Some(&job_id);
    let is_expanded = state.expanded_jobs.contains(&job_id);

    let row_bg = if is_selected { tokens.bg_surface_active } else { tokens.bg_surface };

    egui::Frame::new().fill(row_bg).corner_radius(4).inner_margin(4.0).show(ui, |ui| {
        // --- 1. Parent Source Row ---
        ui.horizontal(|ui| {
            let src_col_width = (ui.available_width() - 650.0_f32.min(ui.available_width() * 0.65)).max(160.0);

            ui.allocate_ui_with_layout(egui::vec2(src_col_width, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Expand / Collapse Chevron
                let arrow_icon = if is_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
                if icons::button(ui, arrow_icon, 16.0, false, tokens, "Toggle output targets").clicked() {
                    if is_expanded {
                        state.expanded_jobs.remove(&job_id);
                    } else {
                        state.expanded_jobs.insert(job_id.clone());
                    }
                }

                // Origin Chip (ArtCraft siblings)
                render_client_origin_chip(ui, &job.spec.client_info.app_name, tokens);

                // Priority Badge
                if job.spec.priority != JobPriority::Normal {
                    let prio_label = format!("{:?}", job.spec.priority);
                    ui.label(RichText::new(prio_label).small().color(tokens.accent_primary));
                }

                // Source Name
                let source_name = job_source_name(job);
                let display_source_name = middle_truncate(&source_name, 36);

                let job_select_btn = ui.selectable_label(is_selected, RichText::new(display_source_name).strong()).on_hover_text(&source_name);
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
                    render_job_context_menu(ui, job, all_jobs, state, queue_mgr, on_inspect_job);
                });
            });

            // Format Summary Column
            ui.allocate_ui_with_layout(egui::vec2(110.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if job.spec.targets.len() == 1 {
                    if let Some(t) = job.spec.targets.first() {
                        ui.label(RichText::new(format!("{:?}", t.container)).small().color(tokens.text_dim));
                    }
                } else {
                    ui.label(RichText::new(format!("({} outputs)", job.spec.targets.len())).small().color(tokens.text_dim));
                }
            });

            // Preset Summary Column
            ui.allocate_ui_with_layout(egui::vec2(190.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if job.spec.targets.len() == 1 {
                    if let Some(t) = job.spec.targets.first() {
                        let link =
                            ui.link(RichText::new(middle_truncate(&t.preset_name, 24)).color(tokens.accent_primary)).on_hover_text(&t.preset_name);
                        if link.clicked() {
                            *on_inspect_job = Some(job.spec.clone());
                        }
                    }
                } else {
                    ui.label(RichText::new("(multiple presets)").small().color(tokens.text_dim));
                }
            });

            // Output File Summary Column
            ui.allocate_ui_with_layout(egui::vec2(220.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if job.spec.targets.len() == 1 {
                    if let Some(t) = job.spec.targets.first() {
                        let path_str = t.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                        let out_link = ui
                            .link(RichText::new(middle_truncate(path_str, 26)).color(tokens.accent_primary))
                            .on_hover_text(t.output_path.display().to_string());
                        if out_link.clicked()
                            && let Some(new_path) = rfd::FileDialog::new().set_file_name(path_str).save_file()
                        {
                            let mut updated = t.clone();
                            updated.output_path = new_path;
                            let _ = queue_mgr.update_output_target(&job_id, 0, updated);
                        }
                    }
                } else {
                    ui.label(RichText::new(format!("({} destination files)", job.spec.targets.len())).small().color(tokens.text_dim));
                }
            });

            // Status Column
            ui.allocate_ui_with_layout(egui::vec2(120.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                render_status_chip(ui, &job.status, tokens);
                if job.status == JobStatus::Processing {
                    let progress = (job.progress_percent / 100.0).clamp(0.0, 1.0) as f32;
                    let pbar = egui::ProgressBar::new(progress).desired_width(50.0);
                    ui.add(pbar);
                }
            });
        });

        // --- 2. Indented Target Child Rows ---
        if is_expanded || job.spec.targets.len() > 1 {
            for (idx, target) in job.spec.targets.iter().enumerate() {
                ui.horizontal(|ui| {
                    let src_col_width = (ui.available_width() - 650.0_f32.min(ui.available_width() * 0.65)).max(160.0);

                    // Child Indent / Bullet Column
                    ui.allocate_ui_with_layout(egui::vec2(src_col_width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new(format!("↳ Target #{}", idx + 1)).small().color(tokens.text_dim));

                        if job.spec.targets.len() > 1 && icons::button(ui, icons::Icon::Trash, 14.0, false, tokens, "Remove output target").clicked()
                        {
                            let _ = queue_mgr.remove_output_target(&job_id, idx);
                        }
                        if icons::button(ui, icons::Icon::Duplicate, 14.0, false, tokens, "Duplicate output target").clicked() {
                            let _ = queue_mgr.duplicate_output_target(&job_id, idx);
                        }
                    });

                    // Format Dropdown Column
                    ui.allocate_ui_with_layout(egui::vec2(110.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let mut current_container = target.container;
                        let combo_text =
                            INLINE_CONTAINERS.iter().find(|(c, _, _)| *c == current_container).map(|(_, name, _)| *name).unwrap_or("Custom");
                        let combo = egui::ComboBox::from_id_salt(format!("target_fmt_{job_id}_{idx}"))
                            .selected_text(RichText::new(combo_text).small())
                            .width(95.0)
                            .show_ui(ui, |ui| {
                                for (c, name, _) in INLINE_CONTAINERS {
                                    ui.selectable_value(&mut current_container, *c, *name);
                                }
                            });

                        if combo.response.changed() && current_container != target.container {
                            let mut updated = target.clone();
                            updated.container = current_container;
                            if let Some((_, _, ext)) = INLINE_CONTAINERS.iter().find(|(c, _, _)| *c == current_container) {
                                let mut p = updated.output_path.clone();
                                p.set_extension(ext);
                                updated.output_path = p;
                            }
                            updated.preset_name = default_preset_for_container(current_container).to_string();
                            let _ = queue_mgr.update_output_target(&job_id, idx, updated);
                        }
                    });

                    // Preset Selector Column (Blue Link style)
                    ui.allocate_ui_with_layout(egui::vec2(190.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let is_menu_open = state.active_preset_menu == Some((job_id.clone(), idx));
                        let pre_btn = ui
                            .link(RichText::new(middle_truncate(&target.preset_name, 22)).color(tokens.accent_primary))
                            .on_hover_text("Click to choose preset; double-click for settings");
                        if pre_btn.clicked() {
                            state.active_preset_menu = if is_menu_open { None } else { Some((job_id.clone(), idx)) };
                        }
                        if pre_btn.double_clicked() {
                            *on_inspect_job = Some(job.spec.clone());
                        }

                        if is_menu_open {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                for preset_name in presets_for_container(target.container) {
                                    if ui.button(preset_name).clicked() {
                                        let mut updated = target.clone();
                                        updated.preset_name = preset_name.to_string();
                                        let _ = queue_mgr.update_output_target(&job_id, idx, updated);
                                        state.active_preset_menu = None;
                                    }
                                }
                                ui.separator();
                                if ui.button("Custom Export Settings...").clicked() {
                                    *on_inspect_job = Some(job.spec.clone());
                                    state.active_preset_menu = None;
                                }
                            });
                        }
                    });

                    // Output File Path Column (Blue Link style)
                    ui.allocate_ui_with_layout(egui::vec2(220.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let path_str = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                        let out_link = ui
                            .link(RichText::new(middle_truncate(path_str, 26)).color(tokens.accent_primary))
                            .on_hover_text(target.output_path.display().to_string());
                        if out_link.clicked()
                            && let Some(new_path) = rfd::FileDialog::new().set_file_name(path_str).save_file()
                        {
                            let mut updated = target.clone();
                            updated.output_path = new_path;
                            let _ = queue_mgr.update_output_target(&job_id, idx, updated);
                        }

                        out_link.context_menu(|ui| {
                            if ui.button("Reveal in Finder / Explorer").clicked() {
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
                        });
                    });

                    // Status Column
                    ui.allocate_ui_with_layout(egui::vec2(120.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{:?}", target.container)).small().color(tokens.text_dim));
                    });
                });
            }
        }
    });

    ui.add_space(2.0);
}

fn render_job_context_menu(
    ui: &mut Ui,
    job: &QueuedJob,
    _all_jobs: &[QueuedJob],
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    on_inspect_job: &mut Option<CraftJobSpec>,
) {
    let job_id = &job.spec.job_id;
    let out_path = job.spec.targets.first().map(|t| t.output_path.clone());
    let src_path = match &job.spec.source {
        SourceDescriptor::File { path, .. } => Some(path.clone()),
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
        let _ = queue_mgr.duplicate_job(job_id);
        ui.close();
    }

    if ui.button("Reset Status").clicked() {
        let _ = queue_mgr.reset_job_status(job_id);
        ui.close();
    }

    if ui.button("Remove").clicked() {
        let _ = queue_mgr.remove_job(job_id);
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

/// Overall batch progress weighted by output targets; cancelled jobs are excluded and failed jobs
/// count as finished so a drained queue reads 100 %.
pub(crate) fn overall_fraction(jobs: &[QueuedJob]) -> f32 {
    let mut weight = 0.0;
    let mut done = 0.0;
    for j in jobs {
        let w = j.spec.targets.len().max(1) as f64;
        match j.status {
            JobStatus::Cancelled => continue,
            JobStatus::Completed | JobStatus::Failed(_) => done += w,
            JobStatus::Processing | JobStatus::Paused => done += w * (j.progress_percent / 100.0).clamp(0.0, 1.0),
            _ => {}
        }
        weight += w;
    }
    if weight <= 0.0 { 0.0 } else { (done / weight).clamp(0.0, 1.0) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};

    fn job(targets: usize, status: JobStatus, pct: f64) -> QueuedJob {
        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "j".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "a.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: (0..targets).map(|_| OutputTarget::default()).collect(),
        };
        let mut j = QueuedJob::new(spec);
        j.status = status;
        j.progress_percent = pct;
        j
    }

    #[test]
    fn overall_progress_weights_by_targets_and_skips_cancelled() {
        let jobs = [job(3, JobStatus::Completed, 100.0), job(1, JobStatus::Processing, 50.0), job(5, JobStatus::Cancelled, 0.0)];
        assert!((overall_fraction(&jobs) - 0.875).abs() < 1e-6);
        assert_eq!(overall_fraction(&[]), 0.0);
        assert_eq!(overall_fraction(&[job(1, JobStatus::Failed("x".into()), 0.0)]), 1.0);
    }

    #[test]
    fn test_middle_truncate() {
        assert_eq!(middle_truncate("short.mov", 20), "short.mov");
        assert_eq!(middle_truncate("very_long_file_name_from_camera_roll.mov", 15), "very_lo…oll.mov");
    }

    #[test]
    fn test_queue_sorting_and_selection() {
        let mut state = QueuePanelState::default();
        state.toggle_sort(QueueSortColumn::Source);
        assert_eq!(state.sort_column, Some(QueueSortColumn::Source));
        assert_eq!(state.sort_direction, SortDirection::Ascending);
        state.toggle_sort(QueueSortColumn::Source);
        assert_eq!(state.sort_direction, SortDirection::Descending);

        let jobs = [job(1, JobStatus::Queued, 0.0)];
        state.select_all(&jobs);
        assert_eq!(state.selected_ids_list(), vec!["j".to_string()]);
        state.clear_selection();
        assert!(state.selected_ids_list().is_empty());
    }

    #[test]
    fn test_default_presets_coverage() {
        for (c, _, _) in INLINE_CONTAINERS {
            let p = default_preset_for_container(*c);
            assert!(!p.is_empty());
        }
    }
}
