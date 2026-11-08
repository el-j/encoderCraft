//! Queue panel: hierarchical data table showing queued sources, output targets, progress, and batch controls.

use std::sync::Arc;

use egui::{RichText, Ui};
use encodercraft_core::{ContainerType, CraftJobSpec, JobPriority, JobStatus, SourceDescriptor};
use encodercraft_engine::{QueueManager, QueuedJob, cmd::CommandRegistry};

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
    pub pending_specs: Vec<CraftJobSpec>,
    pub pending_updates: Vec<(String, CraftJobSpec)>,
    pub status_message: Option<String>,
}

fn execute_queue_command(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    command_id: &str,
    params: serde_json::Value,
) -> Option<serde_json::Value> {
    match commands.dispatch(queue_mgr, command_id, params) {
        Ok(result) => Some(result),
        Err(error) => {
            state.status_message = Some(format!("Command '{command_id}' failed: {error}"));
            None
        }
    }
}

fn execute_app_queue_command(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    command: crate::commands::AppCommand,
) -> Option<serde_json::Value> {
    execute_app_queue_command_with_params(state, commands, queue_mgr, command, serde_json::json!({}))
}

fn execute_app_queue_command_with_params(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    command: crate::commands::AppCommand,
    params: serde_json::Value,
) -> Option<serde_json::Value> {
    let Some(info) = command.info() else {
        state.status_message = Some(format!("Application command {command:?} is not registered."));
        return None;
    };
    execute_command_info(state, commands, queue_mgr, info, params)
}

fn prepare_app_job_spec(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    command: crate::commands::AppCommand,
    params: serde_json::Value,
) -> Option<CraftJobSpec> {
    let Some(info) = command.info() else {
        state.status_message = Some(format!("Application command {command:?} is not registered."));
        return None;
    };
    let Some(engine_id) = info.engine_id else {
        state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
        return None;
    };
    prepare_job_spec(state, commands, queue_mgr, engine_id, params)
}

fn execute_command_info(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    info: &crate::commands::CommandInfo,
    params: serde_json::Value,
) -> Option<serde_json::Value> {
    let Some(engine_id) = info.engine_id else {
        state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
        return None;
    };
    execute_queue_command(state, commands, queue_mgr, engine_id, params)
}

fn prepare_job_spec(
    state: &mut QueuePanelState,
    commands: &CommandRegistry,
    queue_mgr: &Arc<QueueManager>,
    command_id: &str,
    params: serde_json::Value,
) -> Option<CraftJobSpec> {
    let result = execute_queue_command(state, commands, queue_mgr, command_id, params)?;
    match serde_json::from_value(result) {
        Ok(spec) => Some(spec),
        Err(error) => {
            state.status_message = Some(format!("Command '{command_id}' returned an invalid job specification: {error}"));
            None
        }
    }
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

/// Calculated column widths for strict alignment between queue headers and table cells (AME Parity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueueColumnWidths {
    pub src: f32,
    pub format: f32,
    pub preset: f32,
    pub output: f32,
    pub status: f32,
}

impl QueueColumnWidths {
    pub const FORMAT: f32 = 100.0;
    pub const PRESET: f32 = 175.0;
    pub const OUTPUT: f32 = 210.0;
    pub const STATUS: f32 = 110.0;

    pub fn calculate(available_width: f32, spacing: f32) -> Self {
        let fixed = Self::FORMAT + Self::PRESET + Self::OUTPUT + Self::STATUS + (spacing * 4.0);
        let src = (available_width - fixed).max(160.0);
        Self { src, format: Self::FORMAT, preset: Self::PRESET, output: Self::OUTPUT, status: Self::STATUS }
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
    (ContainerType::Caf, "CAF", "caf"),
    (ContainerType::Opus, "Opus", "opus"),
    (ContainerType::Png, "PNG", "png"),
    (ContainerType::Jpeg, "JPEG", "jpg"),
    (ContainerType::WebP, "WebP", "webp"),
    (ContainerType::Tiff, "TIFF", "tif"),
    (ContainerType::OpenExr, "OpenEXR", "exr"),
    (ContainerType::AnimatedGif, "GIF", "gif"),
];

pub const AUDIO_INLINE_CONTAINERS: &[(ContainerType, &str, &str)] = &[
    (ContainerType::Wave, "WAV (BWF)", "wav"),
    (ContainerType::Aiff, "AIFF", "aif"),
    (ContainerType::Flac, "FLAC", "flac"),
    (ContainerType::Mp3, "MP3", "mp3"),
    (ContainerType::OggAudio, "Ogg Vorbis", "ogg"),
    (ContainerType::Caf, "Apple Core Audio (.caf)", "caf"),
    (ContainerType::Opus, "Ogg Opus (.opus)", "opus"),
];

pub const IMAGE_INLINE_CONTAINERS: &[(ContainerType, &str, &str)] = &[
    (ContainerType::Png, "PNG", "png"),
    (ContainerType::Jpeg, "JPEG", "jpg"),
    (ContainerType::WebP, "WebP", "webp"),
    (ContainerType::Tiff, "TIFF", "tif"),
    (ContainerType::OpenExr, "OpenEXR", "exr"),
    (ContainerType::AnimatedGif, "GIF", "gif"),
];

pub const VIDEO_INLINE_CONTAINERS: &[(ContainerType, &str, &str)] = &[
    (ContainerType::IsoBmffMp4, "MP4", "mp4"),
    (ContainerType::QuickTimeMov, "MOV (QuickTime)", "mov"),
    (ContainerType::WebM, "WebM", "webm"),
    (ContainerType::MatroskaMkv, "MKV", "mkv"),
    (ContainerType::MxfOp1a, "MXF OP1a", "mxf"),
    (ContainerType::Wave, "WAV (BWF Audio)", "wav"),
    (ContainerType::Mp3, "MP3 (Audio Extract)", "mp3"),
    (ContainerType::Png, "PNG (Image Sequence)", "png"),
    (ContainerType::AnimatedGif, "GIF Animation", "gif"),
];

/// Returns context-aware candidate containers appropriate for the source stream type.
pub fn candidate_containers_for_job(job: &QueuedJob) -> &'static [(ContainerType, &'static str, &'static str)] {
    let stream_kind = match &job.spec.source {
        encodercraft_core::SourceDescriptor::File { path, .. } => encodercraft_demux::classify_stream_kind(path),
        _ => encodercraft_core::MediaStreamKind::VideoWithAudio,
    };
    if stream_kind.is_audio_only() {
        AUDIO_INLINE_CONTAINERS
    } else if stream_kind.is_image() {
        IMAGE_INLINE_CONTAINERS
    } else {
        VIDEO_INLINE_CONTAINERS
    }
}

/// Renders a distinctive, color-coded essence chip indicating media type (Video, Audio, Image, 3D, Stitched).
fn render_media_kind_chip(ui: &mut egui::Ui, job: &QueuedJob, tokens: &Tokens) {
    let (tag, bg, fg, glyph) = match &job.spec.source {
        encodercraft_core::SourceDescriptor::StitchedClips { .. } => ("SEQ", egui::Color32::from_rgb(180, 60, 180), egui::Color32::WHITE, "🎞"),
        encodercraft_core::SourceDescriptor::File { path, .. } => {
            let kind = encodercraft_demux::classify_stream_kind(path);
            if kind.is_audio_only() {
                ("AUD", egui::Color32::from_rgb(35, 134, 54), egui::Color32::WHITE, "🎵")
            } else if kind.is_image() {
                ("IMG", egui::Color32::from_rgb(187, 128, 9), egui::Color32::WHITE, "🖼")
            } else if kind.is_document() {
                ("DOC", egui::Color32::from_rgb(137, 87, 229), egui::Color32::WHITE, "📄")
            } else if kind.is_spatial_3d() {
                ("3D", egui::Color32::from_rgb(41, 140, 150), egui::Color32::WHITE, "📦")
            } else {
                ("VID", tokens.accent_primary, egui::Color32::WHITE, "🎬")
            }
        }
        _ => ("VID", tokens.accent_primary, egui::Color32::WHITE, "🎬"),
    };

    let chip_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(36.0, 15.0));
    ui.allocate_exact_size(egui::vec2(36.0, 15.0), egui::Sense::hover());
    ui.painter().rect_filled(chip_rect, 2.0, bg.gamma_multiply(0.25));
    ui.painter().rect_stroke(chip_rect, 2.0, egui::Stroke::new(1.0, bg), egui::StrokeKind::Inside);
    ui.painter().text(chip_rect.center(), egui::Align2::CENTER_CENTER, format!("{glyph} {tag}"), egui::FontId::proportional(8.5), fg);
}

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
        ContainerType::Caf => "Apple Core Audio CAF (24-bit PCM)",
        ContainerType::Opus => "Voice & Streaming Opus (128 kbps)",
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

/// Context and callbacks passed to the queue panel renderer.
pub struct QueuePanelContext<'a> {
    pub queue_mgr: &'a Arc<QueueManager>,
    pub commands: &'a CommandRegistry,
    pub tokens: &'a Tokens,
    pub on_inspect_job: &'a mut Option<CraftJobSpec>,
    pub on_open_log: &'a mut bool,
    pub auto_shutdown: &'a mut bool,
    pub on_add_media: &'a mut bool,
    pub on_add_folder: &'a mut bool,
    pub selected_renderer: &'a mut String,
    pub on_show_source_properties: &'a mut Option<std::path::PathBuf>,
}

/// Renders the queue panel with batch toolbar, global progress, and hierarchical job table.
pub fn render_queue_panel(ui: &mut Ui, state: &mut QueuePanelState, ctx: QueuePanelContext<'_>) {
    let queue_mgr = ctx.queue_mgr;
    let commands = ctx.commands;
    let tokens = ctx.tokens;
    let on_inspect_job = &mut *ctx.on_inspect_job;
    let on_open_log = &mut *ctx.on_open_log;
    let auto_shutdown = &mut *ctx.auto_shutdown;
    let on_add_media = &mut *ctx.on_add_media;
    let on_add_folder = &mut *ctx.on_add_folder;
    let selected_renderer = &mut *ctx.selected_renderer;
    let on_show_source_properties = &mut *ctx.on_show_source_properties;

    let mut jobs = queue_mgr.list_jobs();

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

    let cols = QueueColumnWidths::calculate(ui.available_width(), ui.spacing().item_spacing.x);

    ui.vertical(|ui| {
        if let Some(message) = &state.status_message {
            ui.label(RichText::new(message).small().color(tokens.status_failed));
        }
        let worker_running = queue_mgr.is_worker_running();
        state.is_running = worker_running && !queue_mgr.is_paused();

        // --- 1. Top Batch Queue Toolbar (AME Reference Style) ---
        let has_selection = !state.selected_job_ids.is_empty() || state.selected_job_id.is_some();
        let command_state = crate::commands::CommandState { jobs: jobs.len(), worker_running, worker_active: state.is_running, ..Default::default() };
        let can_start = crate::commands::AppCommand::StartQueue.enabled(&command_state);

        ui.horizontal(|ui| {
            // Left icon buttons
            let add_btn = icons::button(ui, icons::Icon::Plus, 20.0, false, tokens, "Add Media Files... (Cmd+I)");
            if add_btn.clicked() {
                *on_add_media = true;
            }

            let add_folder_btn = icons::button(ui, icons::Icon::Folder, 20.0, false, tokens, "Add Entire Folder to Queue... (Cmd+Shift+I)");
            if add_folder_btn.clicked() {
                *on_add_folder = true;
            }

            ui.add_enabled_ui(has_selection, |ui| {
                let remove_btn = icons::button(ui, icons::Icon::Trash, 20.0, false, tokens, "Remove selected job(s) (Delete)");
                if remove_btn.clicked() {
                    let ids = state.selected_ids_list();
                    for id in ids {
                        execute_app_queue_command_with_params(
                            state,
                            commands,
                            queue_mgr,
                            crate::commands::AppCommand::RemoveSelected,
                            serde_json::json!({ "jobId": id }),
                        );
                    }
                    state.clear_selection();
                }

                let dup_btn = icons::button(ui, icons::Icon::Duplicate, 20.0, false, tokens, "Duplicate selected job(s) (Cmd+D)");
                if dup_btn.clicked() {
                    let ids = state.selected_ids_list();
                    for id in ids {
                        if let Some(spec) = prepare_app_job_spec(
                            state,
                            commands,
                            queue_mgr,
                            crate::commands::AppCommand::DuplicateSelected,
                            serde_json::json!({ "jobId": id }),
                        ) {
                            state.pending_specs.push(spec);
                        }
                    }
                }

                let settings_btn = icons::button(ui, icons::Icon::Gear, 20.0, false, tokens, "Export Settings... (Cmd+E)");
                if settings_btn.clicked()
                    && let Some(ref id) = state.selected_job_id
                {
                    *on_inspect_job = prepare_app_job_spec(
                        state,
                        commands,
                        queue_mgr,
                        crate::commands::AppCommand::InspectSettings,
                        serde_json::json!({ "jobId": id }),
                    );
                }

                let add_target_btn = icons::button(ui, icons::Icon::Sliders, 20.0, false, tokens, "Add output target to selected job(s)");
                if add_target_btn.clicked() {
                    for id in state.selected_ids_list() {
                        if let Some(updated_spec) = prepare_app_job_spec(
                            state,
                            commands,
                            queue_mgr,
                            crate::commands::AppCommand::AddOutputTarget,
                            serde_json::json!({ "jobId": id }),
                        ) {
                            state.pending_updates.push((id.clone(), updated_spec));
                            state.expanded_jobs.insert(id);
                        }
                    }
                }

                let reset_btn = icons::button(ui, icons::Icon::Reset, 20.0, false, tokens, "Reset selected job(s) back to Ready");
                if reset_btn.clicked() {
                    for id in state.selected_ids_list() {
                        execute_app_queue_command_with_params(
                            state,
                            commands,
                            queue_mgr,
                            crate::commands::AppCommand::ResetSelected,
                            serde_json::json!({ "jobId": id }),
                        );
                    }
                }
            });

            let has_completed = jobs.iter().any(|j| j.status == JobStatus::Completed);
            ui.add_enabled_ui(has_completed, |ui| {
                if ui.button("Clear Finished").on_hover_text("Remove finished jobs from queue").clicked() {
                    execute_app_queue_command(state, commands, queue_mgr, crate::commands::AppCommand::ClearFinished);
                }
            });

            let log_btn = icons::button(ui, icons::Icon::Clock, 20.0, false, tokens, "Open Encoding Activity Log (Cmd+L)");
            if log_btn.clicked() && execute_app_queue_command(state, commands, queue_mgr, crate::commands::AppCommand::ShowLog).is_some() {
                *on_open_log = true;
            }

            // Right Toolbar: Processed Counter, Stop, Start Queue (prominent green play glyph)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(can_start, |ui| {
                    let start_btn = icons::button_tinted(ui, icons::Icon::Play, 22.0, false, tokens.status_completed, tokens, "Start Queue (Enter)");
                    if start_btn.clicked() {
                        execute_app_queue_command(state, commands, queue_mgr, crate::commands::AppCommand::StartQueue);
                    }
                });

                ui.add_enabled_ui(crate::commands::AppCommand::StopQueue.enabled(&command_state), |ui| {
                    let stop_btn = icons::button(ui, icons::Icon::Stop, 20.0, false, tokens, "Stop batch queue (Esc)");
                    if stop_btn.clicked() {
                        execute_app_queue_command(state, commands, queue_mgr, crate::commands::AppCommand::StopQueue);
                    }
                });

                ui.add_enabled_ui(crate::commands::AppCommand::PauseQueue.enabled(&command_state), |ui| {
                    let pause_btn = icons::button(ui, icons::Icon::Pause, 20.0, false, tokens, "Pause active encode");
                    if pause_btn.clicked() {
                        execute_app_queue_command(state, commands, queue_mgr, crate::commands::AppCommand::PauseQueue);
                    }
                });

                ui.separator();

                ui.label(RichText::new(crate::queue_footer::processed_counter(&jobs)).small().color(tokens.text_dim));

                if !jobs.is_empty() && (worker_running || jobs.iter().any(|j| matches!(j.status, JobStatus::Processing | JobStatus::Completed))) {
                    let pbar = egui::ProgressBar::new(overall_fraction(&jobs)).desired_width(70.0);
                    ui.add(pbar);
                }
            });
        });

        ui.add_space(2.0);

        // --- 2. Hierarchical Job Table with Columns ---
        if jobs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("The transcode queue is empty.").color(tokens.text_dim));
                ui.label(
                    RichText::new("Drag and drop media files, use File > Add Media Files..., or pick from Presets to begin.").color(tokens.text_dim),
                );
            });
        } else {
            // Table Header
            render_table_header(ui, state, tokens, cols);

            ui.separator();

            // Table Rows Scroll Area
            egui::ScrollArea::both().id_salt("queue_job_rows_scroll_area").auto_shrink([false; 2]).show(ui, |ui| {
                for (idx, job) in jobs.iter().enumerate() {
                    let item = GroupedJobItem { job, job_idx: idx, all_jobs: &jobs, cols, on_inspect_job, on_show_source_properties };
                    render_grouped_job_row(ui, item, state, queue_mgr, commands, tokens);
                }
            });
        }

        // --- 3. Panel Footer (AME Parity: Auto Computer Shutdown & Renderer) ---
        ui.add_space(tokens.toolbar_gap);
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(auto_shutdown, "Auto Computer Shutdown")
                .on_hover_text("Shut the computer down 60 seconds after the queue finishes; you can cancel the countdown");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let renderers = crate::queue_footer::available_renderers();
                if selected_renderer.is_empty() || !renderers.iter().any(|r| r == selected_renderer) {
                    *selected_renderer = renderers.first().cloned().unwrap_or_else(|| "Pure-Rust Software (CPU)".into());
                }
                egui::ComboBox::from_id_salt("queue_footer_renderer").selected_text(RichText::new(&*selected_renderer).small()).show_ui(ui, |ui| {
                    for name in &renderers {
                        let is_selected = *selected_renderer == *name;
                        if ui.selectable_label(is_selected, name).clicked() {
                            *selected_renderer = name.clone();
                        }
                    }
                });
                ui.label(RichText::new("Renderer:").small().color(tokens.text_dim));
            });
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

fn render_column_cell<R>(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let mut cell_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    cell_ui.set_clip_rect(rect);
    add_contents(&mut cell_ui)
}

fn render_table_header(ui: &mut Ui, state: &mut QueuePanelState, tokens: &Tokens, cols: QueueColumnWidths) {
    egui::Frame::new().fill(tokens.bg_panel_darker).inner_margin(egui::vec2(tokens.panel_inset, 3.0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            // Source Column (expands)
            render_column_cell(ui, cols.src, 18.0, |ui| {
                let src_label = format!("Source Name{}", sort_indicator(state, QueueSortColumn::Source));
                let src_resp = ui.add_sized([cols.src, 18.0], egui::Button::new(RichText::new(src_label).strong().small()).frame(false));
                if src_resp.clicked() {
                    state.toggle_sort(QueueSortColumn::Source);
                }
            });

            // Format Column
            render_column_cell(ui, cols.format, 18.0, |ui| {
                let fmt_label = format!("Format{}", sort_indicator(state, QueueSortColumn::Format));
                let fmt_resp = ui.add_sized([cols.format, 18.0], egui::Button::new(RichText::new(fmt_label).strong().small()).frame(false));
                if fmt_resp.clicked() {
                    state.toggle_sort(QueueSortColumn::Format);
                }
            });

            // Preset Column
            render_column_cell(ui, cols.preset, 18.0, |ui| {
                let pre_label = format!("Preset{}", sort_indicator(state, QueueSortColumn::Preset));
                let pre_resp = ui.add_sized([cols.preset, 18.0], egui::Button::new(RichText::new(pre_label).strong().small()).frame(false));
                if pre_resp.clicked() {
                    state.toggle_sort(QueueSortColumn::Preset);
                }
            });

            // Output File Column
            render_column_cell(ui, cols.output, 18.0, |ui| {
                let out_label = format!("Output File{}", sort_indicator(state, QueueSortColumn::Output));
                let out_resp = ui.add_sized([cols.output, 18.0], egui::Button::new(RichText::new(out_label).strong().small()).frame(false));
                if out_resp.clicked() {
                    state.toggle_sort(QueueSortColumn::Output);
                }
            });

            // Status Column
            render_column_cell(ui, cols.status, 18.0, |ui| {
                let st_label = format!("Status{}", sort_indicator(state, QueueSortColumn::Status));
                let st_resp = ui.add_sized([cols.status, 18.0], egui::Button::new(RichText::new(st_label).strong().small()).frame(false));
                if st_resp.clicked() {
                    state.toggle_sort(QueueSortColumn::Status);
                }
            });
        });
    });
}

struct GroupedJobItem<'a> {
    pub job: &'a QueuedJob,
    pub job_idx: usize,
    pub all_jobs: &'a [QueuedJob],
    pub cols: QueueColumnWidths,
    pub on_inspect_job: &'a mut Option<CraftJobSpec>,
    pub on_show_source_properties: &'a mut Option<std::path::PathBuf>,
}

fn render_grouped_job_row(
    ui: &mut Ui,
    item: GroupedJobItem<'_>,
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    commands: &CommandRegistry,
    tokens: &Tokens,
) {
    let job = item.job;
    let job_idx = item.job_idx;
    let all_jobs = item.all_jobs;
    let cols = item.cols;
    let on_inspect_job = item.on_inspect_job;
    let on_show_source_properties = item.on_show_source_properties;
    let job_id = job.spec.job_id.clone();
    let is_selected = state.selected_job_ids.contains(&job_id) || state.selected_job_id.as_deref() == Some(&job_id);
    let is_expanded = state.expanded_jobs.contains(&job_id);

    let row_bg = if is_selected {
        tokens.bg_surface_active
    } else if job_idx % 2 == 1 {
        tokens.bg_surface
    } else {
        tokens.bg_panel
    };

    egui::Frame::new().fill(row_bg).inner_margin(egui::vec2(tokens.panel_inset, 1.0)).show(ui, |ui| {
        // --- 1. Parent Source Row ---
        ui.horizontal(|ui| {
            render_column_cell(ui, cols.src, tokens.row_height, |ui| {
                // Expand / Collapse Chevron
                let arrow_icon = if is_expanded { icons::Icon::ChevronDown } else { icons::Icon::ChevronRight };
                if icons::button(ui, arrow_icon, 16.0, false, tokens, "Toggle output targets").clicked() {
                    if is_expanded {
                        state.expanded_jobs.remove(&job_id);
                    } else {
                        state.expanded_jobs.insert(job_id.clone());
                    }
                }

                // Media Kind Chip (VID, AUD, IMG, DOC, 3D, SEQ)
                render_media_kind_chip(ui, job, tokens);

                // Origin Chip (ArtCraft siblings)
                render_client_origin_chip(ui, &job.spec.client_info.app_name, tokens);

                // Priority Badge
                if job.spec.priority != JobPriority::Normal {
                    let prio_label = format!("{:?}", job.spec.priority);
                    ui.label(RichText::new(prio_label).small().color(tokens.accent_primary));
                }

                // Source Name
                let source_name = job_source_name(job);
                let approx_chars = ((ui.available_width() - 8.0) / 7.2).clamp(8.0, 48.0) as usize;
                let display_source_name = middle_truncate(&source_name, approx_chars);

                let job_select_btn = ui
                    .add(egui::Button::new(RichText::new(display_source_name).strong()).selected(is_selected).truncate())
                    .on_hover_text(&source_name);
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
                    render_job_context_menu(ui, job, all_jobs, state, queue_mgr, commands, on_show_source_properties);
                });
            });

            // Format Summary Column (Inline dropdown selector)
            render_column_cell(ui, cols.format, tokens.row_height, |ui| {
                if job.spec.targets.len() == 1 {
                    if let Some(t) = job.spec.targets.first() {
                        let candidate_containers = candidate_containers_for_job(job);
                        let mut current_container = t.container;
                        let combo_text =
                            candidate_containers.iter().find(|(c, _, _)| *c == current_container).map(|(_, name, _)| *name).unwrap_or("Custom");
                        let _combo = egui::ComboBox::from_id_salt(format!("summary_fmt_{job_id}"))
                            .selected_text(RichText::new(combo_text).small())
                            .width(cols.format - 12.0)
                            .show_ui(ui, |ui| {
                                for (c, name, _) in candidate_containers {
                                    if ui.selectable_label(current_container == *c, *name).clicked() {
                                        current_container = *c;
                                    }
                                }
                            });

                        if current_container != t.container {
                            let jobs_to_update: Vec<String> = if state.selected_job_ids.contains(&job_id) {
                                state.selected_job_ids.iter().cloned().collect()
                            } else {
                                vec![job_id.clone()]
                            };

                            for target_job_id in jobs_to_update {
                                if let Some(target_job) = all_jobs.iter().find(|j| j.spec.job_id == target_job_id)
                                    && let Some(target_t) = target_job.spec.targets.first()
                                {
                                    let mut updated = target_t.clone();
                                    updated.container = current_container;
                                    let ext = current_container.file_extension();
                                    let mut p = updated.output_path.clone();
                                    p.set_extension(ext);
                                    updated.output_path = p;
                                    updated.preset_name = default_preset_for_container(current_container).to_string();
                                    execute_queue_command(
                                        state,
                                        commands,
                                        queue_mgr,
                                        "queue.update_output_target",
                                        serde_json::json!({ "jobId": target_job_id, "targetIndex": 0, "target": updated }),
                                    );
                                }
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                } else {
                    ui.label(RichText::new(format!("({} outputs)", job.spec.targets.len())).small().color(tokens.text_dim));
                }
            });

            // Preset Summary Column (Inline dropdown selector + Custom Inspector link)
            render_column_cell(ui, cols.preset, tokens.row_height, |ui| {
                if job.spec.targets.len() == 1 {
                    if let Some(t) = job.spec.targets.first() {
                        let display_name = middle_truncate(&t.preset_name, 22);
                        let mut chosen_preset = t.preset_name.clone();
                        let candidate_presets = presets_for_container(t.container);
                        let _combo = egui::ComboBox::from_id_salt(format!("summary_pre_{job_id}"))
                            .selected_text(RichText::new(display_name).color(tokens.accent_primary).small())
                            .width(cols.preset - 12.0)
                            .show_ui(ui, |ui| {
                                for preset_name in &candidate_presets {
                                    if ui.selectable_label(t.preset_name == *preset_name, *preset_name).clicked() {
                                        chosen_preset = (*preset_name).to_string();
                                    }
                                }
                                ui.separator();
                                if ui.button("Custom Export Settings... (Cmd+E)").clicked() {
                                    *on_inspect_job = Some(job.spec.clone());
                                }
                            });
                        if chosen_preset != t.preset_name {
                            let jobs_to_update: Vec<String> = if state.selected_job_ids.contains(&job_id) {
                                state.selected_job_ids.iter().cloned().collect()
                            } else {
                                vec![job_id.clone()]
                            };
                            for target_job_id in jobs_to_update {
                                if let Some(target_job) = all_jobs.iter().find(|j| j.spec.job_id == target_job_id)
                                    && let Some(target_t) = target_job.spec.targets.first()
                                {
                                    let mut updated = target_t.clone();
                                    updated.preset_name = chosen_preset.clone();
                                    execute_queue_command(
                                        state,
                                        commands,
                                        queue_mgr,
                                        "queue.update_output_target",
                                        serde_json::json!({ "jobId": target_job_id, "targetIndex": 0, "target": updated }),
                                    );
                                }
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                } else {
                    ui.label(RichText::new("(multiple presets)").small().color(tokens.text_dim));
                }
            });

            // Output File Summary Column
            render_column_cell(ui, cols.output, tokens.row_height, |ui| {
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
                            execute_queue_command(
                                state,
                                commands,
                                queue_mgr,
                                "queue.update_output_target",
                                serde_json::json!({ "jobId": job_id, "targetIndex": 0, "target": updated }),
                            );
                        }
                    }
                } else {
                    ui.label(RichText::new(format!("({} destination files)", job.spec.targets.len())).small().color(tokens.text_dim));
                }
            });

            // Status Column
            render_column_cell(ui, cols.status, tokens.row_height, |ui| {
                render_status_chip(ui, &job.status, tokens);
                if job.status == JobStatus::Processing {
                    let progress = (job.progress_percent / 100.0).clamp(0.0, 1.0) as f32;
                    let pbar = egui::ProgressBar::new(progress).desired_width(45.0);
                    ui.add(pbar);
                }
            });
        });

        // --- 2. Indented Target Child Rows ---
        if is_expanded || job.spec.targets.len() > 1 {
            for (idx, target) in job.spec.targets.iter().enumerate() {
                ui.horizontal(|ui| {
                    // Child Indent / Bullet Column
                    render_column_cell(ui, cols.src, tokens.row_height, |ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new(format!("↳ Target #{}", idx + 1)).small().color(tokens.text_dim));

                        if job.spec.targets.len() > 1 && icons::button(ui, icons::Icon::Trash, 14.0, false, tokens, "Remove output target").clicked()
                        {
                            execute_queue_command(
                                state,
                                commands,
                                queue_mgr,
                                "queue.remove_output_target",
                                serde_json::json!({ "jobId": job_id, "targetIndex": idx }),
                            );
                        }
                        if icons::button(ui, icons::Icon::Duplicate, 14.0, false, tokens, "Duplicate output target").clicked() {
                            execute_queue_command(
                                state,
                                commands,
                                queue_mgr,
                                "queue.duplicate_output_target",
                                serde_json::json!({ "jobId": job_id, "targetIndex": idx }),
                            );
                        }
                    });

                    // Format Dropdown Column
                    render_column_cell(ui, cols.format, tokens.row_height, |ui| {
                        let candidate_containers = candidate_containers_for_job(job);
                        let mut current_container = target.container;
                        let combo_text =
                            candidate_containers.iter().find(|(c, _, _)| *c == current_container).map(|(_, name, _)| *name).unwrap_or("Custom");
                        let _combo = egui::ComboBox::from_id_salt(format!("target_fmt_{job_id}_{idx}"))
                            .selected_text(RichText::new(combo_text).small())
                            .width(cols.format - 12.0)
                            .show_ui(ui, |ui| {
                                for (c, name, _) in candidate_containers {
                                    if ui.selectable_label(current_container == *c, *name).clicked() {
                                        current_container = *c;
                                    }
                                }
                            });

                        if current_container != target.container {
                            let jobs_to_update: Vec<String> = if state.selected_job_ids.contains(&job_id) {
                                state.selected_job_ids.iter().cloned().collect()
                            } else {
                                vec![job_id.clone()]
                            };

                            for sel_id in jobs_to_update {
                                if let Some(sel_job) = all_jobs.iter().find(|j| j.spec.job_id == sel_id) {
                                    let target_idx = if sel_id == job_id { idx } else { 0 };
                                    if let Some(target_t) = sel_job.spec.targets.get(target_idx) {
                                        let mut updated = target_t.clone();
                                        updated.container = current_container;
                                        let ext = current_container.file_extension();
                                        let mut p = updated.output_path.clone();
                                        p.set_extension(ext);
                                        updated.output_path = p;
                                        updated.preset_name = default_preset_for_container(current_container).to_string();
                                        execute_queue_command(
                                            state,
                                            commands,
                                            queue_mgr,
                                            "queue.update_output_target",
                                            serde_json::json!({ "jobId": sel_id, "targetIndex": target_idx, "target": updated }),
                                        );
                                    }
                                }
                            }
                            ui.ctx().request_repaint();
                        }
                    });

                    // Preset Selector Column (Blue Link style)
                    render_column_cell(ui, cols.preset, tokens.row_height, |ui| {
                        let is_menu_open = state.active_preset_menu == Some((job_id.clone(), idx));
                        let pre_btn = ui
                            .link(RichText::new(middle_truncate(&target.preset_name, 22)).color(tokens.accent_primary))
                            .on_hover_text("Click to choose preset; double-click for settings");
                        if pre_btn.clicked() {
                            state.active_preset_menu = if is_menu_open { None } else { Some((job_id.clone(), idx)) };
                        }
                        if pre_btn.double_clicked()
                            && let Some(spec) = prepare_app_job_spec(
                                state,
                                commands,
                                queue_mgr,
                                crate::commands::AppCommand::InspectSettings,
                                serde_json::json!({ "jobId": job_id }),
                            )
                        {
                            *on_inspect_job = Some(spec);
                        }

                        if is_menu_open {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                for preset_name in presets_for_container(target.container) {
                                    if ui.button(preset_name).clicked() {
                                        let jobs_to_update: Vec<String> = if state.selected_job_ids.contains(&job_id) {
                                            state.selected_job_ids.iter().cloned().collect()
                                        } else {
                                            vec![job_id.clone()]
                                        };
                                        for sel_id in jobs_to_update {
                                            if let Some(sel_job) = all_jobs.iter().find(|j| j.spec.job_id == sel_id) {
                                                let target_idx = if sel_id == job_id { idx } else { 0 };
                                                if let Some(target_t) = sel_job.spec.targets.get(target_idx) {
                                                    let mut updated = target_t.clone();
                                                    updated.preset_name = preset_name.to_string();
                                                    execute_queue_command(
                                                        state,
                                                        commands,
                                                        queue_mgr,
                                                        "queue.update_output_target",
                                                        serde_json::json!({ "jobId": sel_id, "targetIndex": target_idx, "target": updated }),
                                                    );
                                                }
                                            }
                                        }
                                        ui.ctx().request_repaint();
                                        state.active_preset_menu = None;
                                    }
                                }
                                ui.separator();
                                if ui.button("Custom Export Settings...").clicked() {
                                    if let Some(spec) = prepare_app_job_spec(
                                        state,
                                        commands,
                                        queue_mgr,
                                        crate::commands::AppCommand::InspectSettings,
                                        serde_json::json!({ "jobId": job_id }),
                                    ) {
                                        *on_inspect_job = Some(spec);
                                    }
                                    state.active_preset_menu = None;
                                }
                            });
                        }
                    });

                    // Output File Path Column (Blue Link style)
                    render_column_cell(ui, cols.output, tokens.row_height, |ui| {
                        let path_str = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
                        let out_link = ui
                            .link(RichText::new(middle_truncate(path_str, 26)).color(tokens.accent_primary))
                            .on_hover_text(target.output_path.display().to_string());
                        if out_link.clicked()
                            && let Some(new_path) = rfd::FileDialog::new().set_file_name(path_str).save_file()
                        {
                            let mut updated = target.clone();
                            updated.output_path = new_path;
                            execute_queue_command(
                                state,
                                commands,
                                queue_mgr,
                                "queue.update_output_target",
                                serde_json::json!({ "jobId": job_id, "targetIndex": idx, "target": updated }),
                            );
                        }

                        out_link.context_menu(|ui| {
                            if ui.button("Reveal in Finder / Explorer").clicked() {
                                if let Some(value) = execute_queue_command(
                                    state,
                                    commands,
                                    queue_mgr,
                                    "queue.reveal_output",
                                    serde_json::json!({ "jobId": job_id, "targetIndex": idx }),
                                ) && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                                {
                                    reveal_in_file_manager(std::path::Path::new(path));
                                }
                                ui.close();
                            }
                            if ui.button("Copy Output Path").clicked() {
                                if let Some(value) = execute_queue_command(
                                    state,
                                    commands,
                                    queue_mgr,
                                    "queue.copy_output_path",
                                    serde_json::json!({ "jobId": job_id, "targetIndex": idx }),
                                ) && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                                {
                                    ui.ctx().copy_text(path.to_owned());
                                }
                                ui.close();
                            }
                            if ui.button("Inspect Settings...").clicked() {
                                if let Some(spec) = prepare_app_job_spec(
                                    state,
                                    commands,
                                    queue_mgr,
                                    crate::commands::AppCommand::InspectSettings,
                                    serde_json::json!({ "jobId": job_id }),
                                ) {
                                    *on_inspect_job = Some(spec);
                                }
                                ui.close();
                            }
                        });
                    });

                    // Status Column
                    render_column_cell(ui, cols.status, tokens.row_height, |ui| {
                        ui.label(RichText::new(target.container.display_name()).small().color(tokens.text_dim));
                    });
                });
            }
        }

        if is_expanded && let SourceDescriptor::StitchedClips { clips } = &job.spec.source {
            for (c_idx, clip) in clips.iter().enumerate() {
                ui.horizontal(|ui| {
                    render_column_cell(ui, cols.src, tokens.row_height, |ui| {
                        ui.add_space(24.0);
                        let clip_name = clip.path.file_name().and_then(|s| s.to_str()).unwrap_or("Clip");
                        ui.label(RichText::new(format!("↳ Clip #{}: {}", c_idx + 1, clip_name)).small().color(tokens.text_dim));
                    });
                    render_column_cell(ui, cols.format, tokens.row_height, |ui| {
                        ui.label(RichText::new("Segment").small().color(tokens.text_dim));
                    });
                    render_column_cell(ui, cols.preset, tokens.row_height, |_| {});
                    render_column_cell(ui, cols.output, tokens.row_height, |_| {});
                    render_column_cell(ui, cols.status, tokens.row_height, |_| {});
                });
            }
        }
    });

    ui.add_space(1.0);
}

fn render_job_context_menu(
    ui: &mut Ui,
    job: &QueuedJob,
    all_jobs: &[QueuedJob],
    state: &mut QueuePanelState,
    queue_mgr: &Arc<QueueManager>,
    commands: &CommandRegistry,
    on_show_source_properties: &mut Option<std::path::PathBuf>,
) {
    let job_id = &job.spec.job_id;
    if !state.selected_job_ids.contains(job_id) {
        let Some(index) = all_jobs.iter().position(|candidate| candidate.spec.job_id == *job_id) else {
            state.status_message = Some(format!("Queue item '{job_id}' is no longer available."));
            return;
        };
        state.select_single(job_id.clone(), index);
    }
    let selected_ids: Vec<_> = all_jobs
        .iter()
        .filter(|candidate| state.selected_job_ids.contains(&candidate.spec.job_id))
        .map(|candidate| candidate.spec.job_id.clone())
        .collect();
    let out_path = job.spec.targets.first().map(|t| t.output_path.clone());
    let src_path = match &job.spec.source {
        SourceDescriptor::File { path, .. } => Some(path.clone()),
        _ => None,
    };

    for info in crate::commands::context_menu_commands(crate::commands::CommandContextMenu::QueueRow) {
        if info.command == crate::commands::AppCommand::ResetSelected {
            ui.separator();
        }
        let command_state = crate::commands::CommandState {
            selected: selected_ids.len(),
            jobs: all_jobs.len(),
            has_source: src_path.is_some(),
            has_output: out_path.is_some(),
            ..Default::default()
        };
        if !info.command.enabled(&command_state) {
            continue;
        }
        if !ui.button(info.label).clicked() {
            continue;
        }
        match info.command {
            crate::commands::AppCommand::RevealOutput => {
                if let Some(value) = execute_command_info(state, commands, queue_mgr, info, serde_json::json!({ "jobId": job.spec.job_id }))
                    && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                {
                    reveal_in_file_manager(std::path::Path::new(path));
                }
            }
            crate::commands::AppCommand::CopyOutputPath => {
                if let Some(value) = execute_command_info(state, commands, queue_mgr, info, serde_json::json!({ "jobId": job.spec.job_id }))
                    && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                {
                    ui.ctx().copy_text(path.to_owned());
                }
            }
            crate::commands::AppCommand::RevealSource => {
                if let Some(value) = execute_command_info(state, commands, queue_mgr, info, serde_json::json!({ "jobId": job.spec.job_id }))
                    && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                {
                    reveal_in_file_manager(std::path::Path::new(path));
                }
            }
            crate::commands::AppCommand::CopySourcePath => {
                if let Some(value) = execute_command_info(state, commands, queue_mgr, info, serde_json::json!({ "jobId": job.spec.job_id }))
                    && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                {
                    ui.ctx().copy_text(path.to_owned());
                }
            }
            crate::commands::AppCommand::ExportSettings => {
                if let Some(value) = execute_command_info(state, commands, queue_mgr, info, serde_json::json!({ "jobId": job.spec.job_id }))
                    && let Some(path) = rfd::FileDialog::new().set_title("Export Job Settings").set_file_name("encodercraft-job.json").save_file()
                {
                    let result = serde_json::to_vec_pretty(&value)
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|error| error.to_string()));
                    if let Err(error) = result {
                        state.status_message = Some(format!("Could not export job settings: {error}"));
                    } else {
                        state.status_message = Some(format!("Exported job settings to {}", path.display()));
                    }
                }
            }
            crate::commands::AppCommand::ResetSelected | crate::commands::AppCommand::RemoveSelected => {
                let Some(command_id) = info.engine_id else {
                    state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
                    ui.close();
                    continue;
                };
                for id in &selected_ids {
                    execute_queue_command(state, commands, queue_mgr, command_id, serde_json::json!({ "jobId": id }));
                }
            }
            crate::commands::AppCommand::DuplicateSelected => {
                let Some(command_id) = info.engine_id else {
                    state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
                    ui.close();
                    continue;
                };
                for id in &selected_ids {
                    if let Some(spec) = prepare_job_spec(state, commands, queue_mgr, command_id, serde_json::json!({ "jobId": id })) {
                        state.pending_specs.push(spec);
                    }
                }
            }
            crate::commands::AppCommand::MoveSelectedUp | crate::commands::AppCommand::MoveSelectedDown => {
                let Some(command_id) = info.engine_id else {
                    state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
                    ui.close();
                    continue;
                };
                let mut ids = selected_ids.clone();
                if info.command == crate::commands::AppCommand::MoveSelectedDown {
                    ids.reverse();
                }
                for id in ids {
                    execute_queue_command(state, commands, queue_mgr, command_id, serde_json::json!({ "jobId": id }));
                }
            }
            crate::commands::AppCommand::StitchSelected => {
                let Some(command_id) = info.engine_id else {
                    state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
                    ui.close();
                    continue;
                };
                let output_path = out_path
                    .as_ref()
                    .map_or_else(|| std::env::temp_dir().join("stitched_batch.mp4"), |path| path.with_file_name("stitched_batch.mp4"));
                if let Some(spec) = prepare_job_spec(
                    state,
                    commands,
                    queue_mgr,
                    command_id,
                    serde_json::json!({
                        "jobIds": selected_ids,
                        "outputPath": output_path,
                        "presetName": "Match Source - High Bitrate",
                    }),
                ) {
                    state.pending_specs.push(spec);
                }
            }
            crate::commands::AppCommand::SetDestination => {
                let Some(command_id) = info.engine_id else {
                    state.status_message = Some(format!("Command '{}' has no engine binding.", info.id));
                    ui.close();
                    continue;
                };
                if let Some(folder) = rfd::FileDialog::new().set_title("Select Output Directory").pick_folder() {
                    execute_queue_command(state, commands, queue_mgr, command_id, serde_json::json!({ "jobIds": selected_ids, "outputDir": folder }));
                }
            }
            crate::commands::AppCommand::SourceProperties => {
                if let Some(ref path) = src_path {
                    *on_show_source_properties = Some(path.clone());
                }
            }
            _ => {}
        }
        ui.close();
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
    fn queue_panel_engine_command_ids_are_registered() {
        let registry = CommandRegistry::new();
        let registered: std::collections::HashSet<_> = registry.list().into_iter().map(|(id, _, _)| id).collect();
        for id in [
            "queue.start",
            "queue.pause_worker",
            "queue.stop_worker",
            "queue.move_up",
            "queue.move_down",
            "queue.duplicate_spec",
            "queue.reset",
            "queue.remove",
            "queue.clear_finished",
            "queue.prepare_stitch",
            "queue.set_output_dir",
            "queue.prepare_add_output_target",
            "queue.update_output_target",
            "queue.remove_output_target",
            "queue.duplicate_output_target",
        ] {
            assert!(registered.contains(id), "queue panel references unregistered command {id}");
        }
    }

    #[test]
    fn test_default_presets_coverage() {
        for (c, _, _) in INLINE_CONTAINERS {
            let p = default_preset_for_container(*c);
            assert!(!p.is_empty());
        }
    }

    #[test]
    fn test_render_column_cell_strict_bounds() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let width = 120.0;
                let height = 22.0;
                let start_x = ui.cursor().min.x;
                render_column_cell(ui, width, height, |cell_ui| {
                    cell_ui.add_sized([300.0, height], egui::Label::new("Ultra long text that exceeds cell bounds"));
                });
                let advanced_x = ui.cursor().min.x - start_x;
                assert!((advanced_x - (width + ui.spacing().item_spacing.x)).abs() < 1.0);
            });
        });
        output.textures_delta.clear();
    }

    #[test]
    fn column_widths_maintain_fixed_columns_and_expand_source() {
        let cols_narrow = QueueColumnWidths::calculate(500.0, 4.0);
        assert_eq!(cols_narrow.format, QueueColumnWidths::FORMAT);
        assert_eq!(cols_narrow.preset, QueueColumnWidths::PRESET);
        assert_eq!(cols_narrow.output, QueueColumnWidths::OUTPUT);
        assert_eq!(cols_narrow.status, QueueColumnWidths::STATUS);
        assert!(cols_narrow.src >= 160.0);

        let cols_wide = QueueColumnWidths::calculate(1200.0, 4.0);
        assert_eq!(cols_wide.format, QueueColumnWidths::FORMAT);
        assert_eq!(cols_wide.preset, QueueColumnWidths::PRESET);
        assert_eq!(cols_wide.output, QueueColumnWidths::OUTPUT);
        assert_eq!(cols_wide.status, QueueColumnWidths::STATUS);
        assert!(cols_wide.src > cols_narrow.src);
    }

    #[test]
    fn test_queue_panel_renderer_selection() {
        let queue_mgr = Arc::new(QueueManager::new_in_memory());
        let commands = CommandRegistry::new();
        let tokens = Tokens::dark();
        let mut queue_state = QueuePanelState::default();
        let mut on_inspect = None;
        let mut on_open_log = false;
        let mut auto_shutdown = false;
        let mut on_add_media = false;
        let mut on_add_folder = false;
        let mut selected_renderer = String::new();
        let mut on_show_source_properties = None;

        egui::__run_test_ui(|ui| {
            render_queue_panel(
                ui,
                &mut queue_state,
                QueuePanelContext {
                    queue_mgr: &queue_mgr,
                    commands: &commands,
                    tokens: &tokens,
                    on_inspect_job: &mut on_inspect,
                    on_open_log: &mut on_open_log,
                    auto_shutdown: &mut auto_shutdown,
                    on_add_media: &mut on_add_media,
                    on_add_folder: &mut on_add_folder,
                    selected_renderer: &mut selected_renderer,
                    on_show_source_properties: &mut on_show_source_properties,
                },
            );
        });

        assert!(!selected_renderer.is_empty());
        assert!(selected_renderer.contains("Hardware") || selected_renderer.contains("Software"));
    }

    #[test]
    fn test_candidate_containers_by_source_kind() {
        // Audio job
        let audio_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "audio-1".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "podcast.mp3".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget::default()],
        };
        let audio_job = QueuedJob::new(audio_spec);
        let audio_containers = candidate_containers_for_job(&audio_job);
        assert!(audio_containers.iter().any(|(c, _, _)| *c == ContainerType::Wave));
        assert!(audio_containers.iter().any(|(c, _, _)| *c == ContainerType::Mp3));
        assert!(!audio_containers.iter().any(|(c, _, _)| *c == ContainerType::Png));
        assert!(!audio_containers.iter().any(|(c, _, _)| *c == ContainerType::IsoBmffMp4));

        // Image job
        let img_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "img-1".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "photo.png".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget::default()],
        };
        let img_job = QueuedJob::new(img_spec);
        let img_containers = candidate_containers_for_job(&img_job);
        assert!(img_containers.iter().any(|(c, _, _)| *c == ContainerType::Png));
        assert!(img_containers.iter().any(|(c, _, _)| *c == ContainerType::Jpeg));
        assert!(!img_containers.iter().any(|(c, _, _)| *c == ContainerType::IsoBmffMp4));

        // Video job
        let vid_spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "vid-1".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "clip.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget::default()],
        };
        let vid_job = QueuedJob::new(vid_spec);
        let vid_containers = candidate_containers_for_job(&vid_job);
        assert!(vid_containers.iter().any(|(c, _, _)| *c == ContainerType::IsoBmffMp4));
        assert!(vid_containers.iter().any(|(c, _, _)| *c == ContainerType::QuickTimeMov));
    }
}
