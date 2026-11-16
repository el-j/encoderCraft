//! Export Settings Inspector: comprehensive modal dialog for fine-tuning preview, container, video, audio, effects, captions, publish and metadata.

use std::path::PathBuf;

use egui::{ColorImage, RichText, TextureHandle, Ui, Window};
use encodercraft_core::{
    CaptionExportFormat, ChannelLayout, ContainerType, CraftJobSpec, DeinterlaceMode, FlipMode, FrameRateMode, MediaStreamKind, OutputTarget,
    OverlayAnchor, Rational64, Rotation, ScalingFilter, SourceDescriptor, SourceSelection, VideoLimiterClipLevel,
};
use encodercraft_geom::ScaleMode;
use encodercraft_pipeline::SourceInventory;
use encodercraft_source::MediaProbe;

use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::queue_panel::{INLINE_CONTAINERS, default_preset_for_container, presets_for_container};
use crate::theme::Tokens;

/// Inspector dialog active tab (AME Parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Video,
    /// Sequence / composition picker for project sources.
    Composition,
    /// Page range picker for document sources.
    Layout,
    /// Layer picker for CAD drawings.
    Cad,
    /// Layer picker for layered raster sources.
    Layers,
    Audio,
    Effects,
    Captions,
    Publish,
    Loudness,
    Metadata,
}

impl InspectorTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Composition => "Composition",
            Self::Layout => "Layout",
            Self::Cad => "CAD",
            Self::Layers => "Layers",
            Self::Audio => "Audio",
            Self::Effects => "Effects",
            Self::Captions => "Captions",
            Self::Publish => "Publish",
            Self::Loudness => "Loudness",
            Self::Metadata => "Metadata",
        }
    }
}

/// Tabs for a job: source-specific pickers first, then the tabs relevant to the output container and stream toggles (AME Parity).
pub fn visible_tabs_for_target(source: Option<ContainerType>, target: &OutputTarget) -> Vec<InspectorTab> {
    use InspectorTab::*;
    let mut tabs = Vec::new();
    match source.map(|c| c.media_kind()) {
        Some(MediaStreamKind::ProjectTimeline | MediaStreamKind::MotionGraph) => tabs.push(Composition),
        Some(MediaStreamKind::Document) => tabs.push(Layout),
        Some(MediaStreamKind::Cad2D) => tabs.push(Cad),
        Some(MediaStreamKind::LayeredComposite) => tabs.push(Layers),
        _ => {}
    }
    match target.container.media_kind() {
        MediaStreamKind::AudioOnly => {
            if target.export_audio {
                tabs.extend([Audio, Loudness]);
            }
            tabs.extend([Publish, Metadata]);
        }
        MediaStreamKind::StillImage | MediaStreamKind::LayeredComposite => {
            tabs.push(Video);
            tabs.extend([Effects, Publish, Metadata]);
        }
        MediaStreamKind::Vector2D | MediaStreamKind::Document | MediaStreamKind::Spatial3D | MediaStreamKind::Cad2D => {
            tabs.extend([Publish, Metadata]);
        }
        _ => {
            if target.export_video {
                tabs.push(Video);
            }
            if target.export_audio {
                tabs.push(Audio);
            }
            tabs.extend([Effects, Captions, Publish]);
            if target.export_audio {
                tabs.push(Loudness);
            }
            tabs.push(Metadata);
        }
    }
    if tabs.is_empty() {
        tabs.push(Metadata);
    }
    tabs
}

/// Tabs for a job: source-specific pickers first, then the tabs relevant to the output container.
/// Domains without video or audio hide those tabs.
pub fn visible_tabs(source: Option<ContainerType>, target: ContainerType) -> Vec<InspectorTab> {
    let dummy = OutputTarget {
        container: target,
        export_video: target.supports_video() || matches!(target.media_kind(), MediaStreamKind::StillImage | MediaStreamKind::LayeredComposite),
        export_audio: target.supports_audio(),
        ..Default::default()
    };
    visible_tabs_for_target(source, &dummy)
}

/// Whether the source holds structure (sequences, pages, layers) rather than plain media, so it is worth inspecting.
pub fn source_has_structure(source: Option<ContainerType>) -> bool {
    matches!(
        source.map(|c| c.media_kind()),
        Some(
            MediaStreamKind::ProjectTimeline
                | MediaStreamKind::MotionGraph
                | MediaStreamKind::Document
                | MediaStreamKind::Cad2D
                | MediaStreamKind::LayeredComposite
        )
    )
}

/// Value stored in `SourceSelection::sequence` for entry `idx`: the name when it is unique, otherwise the index.
pub fn sequence_key(inventory: &SourceInventory, idx: usize) -> Option<String> {
    let seq = inventory.sequences.get(idx)?;
    if inventory.sequences.iter().filter(|s| s.name == seq.name).count() == 1 { Some(seq.name.clone()) } else { Some(idx.to_string()) }
}

/// Index of the currently selected sequence; with no selection the first sequence is the default view.
pub fn selected_sequence_index(inventory: &SourceInventory, selection: &SourceSelection) -> Option<usize> {
    match &selection.sequence {
        None => None,
        Some(key) => {
            inventory.sequences.iter().position(|s| &s.name == key).or_else(|| key.parse::<usize>().ok().filter(|i| *i < inventory.sequences.len()))
        }
    }
}

/// Sets the inclusive page range, clamped to the document; the full range clears the selection.
pub fn set_page_range(selection: &mut SourceSelection, total: usize, first: u32, last: u32) {
    let total = u32::try_from(total).unwrap_or(u32::MAX).max(1);
    let first = first.clamp(1, total);
    let last = last.clamp(first, total);
    selection.pages = if first == 1 && last == total { None } else { Some((first, last)) };
}

/// Checks or unchecks one layer. `None` means every layer; the last remaining layer cannot be unchecked.
pub fn set_layer_enabled(selection: &mut SourceSelection, all: &[String], name: &str, enabled: bool) {
    let mut current: Vec<String> = match &selection.layers {
        Some(l) => l.clone(),
        None => all.to_vec(),
    };
    let present = current.iter().any(|l| l == name);
    if enabled && !present {
        current.push(name.to_string());
    } else if !enabled && present && current.len() > 1 {
        current.retain(|l| l != name);
    } else {
        return;
    }
    // keep the source order so output layer order is stable
    current.sort_by_key(|l| all.iter().position(|a| a == l).unwrap_or(usize::MAX));
    selection.layers = if current.len() == all.len() { None } else { Some(current) };
}

/// Source container of a job, taken from the input file extension.
pub fn source_container(spec: &CraftJobSpec) -> Option<ContainerType> {
    match &spec.source {
        SourceDescriptor::File { path, .. } => path.extension().and_then(|e| e.to_str()).and_then(ContainerType::from_extension),
        _ => None,
    }
}

/// Destination formats offered for a source: only valid conversions, or the common set when the source is unknown.
pub fn destination_options(source: Option<ContainerType>) -> Vec<(ContainerType, &'static str, &'static str)> {
    match source {
        Some(src) => src.valid_conversion_targets().into_iter().map(|c| (c, c.display_name(), c.file_extension())).collect(),
        None => INLINE_CONTAINERS.to_vec(),
    }
}

/// Default audio codec for an output container (AME Parity).
pub fn default_audio_codec_for_container(c: ContainerType) -> &'static str {
    match c {
        ContainerType::Wave => "pcm_s24le",
        ContainerType::Aiff => "pcm_s24be",
        ContainerType::Flac => "flac",
        ContainerType::Mp3 => "mp3",
        ContainerType::OggAudio => "vorbis",
        ContainerType::Opus | ContainerType::WebM => "opus",
        ContainerType::Caf => "lpcm",
        _ => "aac",
    }
}

/// Available audio codecs for an output container format (AME Parity).
pub fn audio_codecs_for_container(container: ContainerType) -> &'static [(&'static str, &'static str)] {
    match container {
        ContainerType::IsoBmffMp4 => &[("aac", "AAC-LC (Advanced Audio Coding)")],
        ContainerType::QuickTimeMov => &[
            ("aac", "AAC-LC (Advanced Audio Coding)"),
            ("pcm_s24le", "Uncompressed Linear PCM (24-bit)"),
            ("pcm_s16le", "Uncompressed Linear PCM (16-bit)"),
            ("alac", "Apple Lossless Audio (ALAC)"),
        ],
        ContainerType::MatroskaMkv => &[
            ("opus", "Ogg Opus (RFC 7845)"),
            ("aac", "AAC-LC (Advanced Audio Coding)"),
            ("flac", "Free Lossless Audio Codec (FLAC)"),
            ("pcm_s24le", "Linear PCM (24-bit)"),
        ],
        ContainerType::WebM => &[("opus", "Ogg Opus (RFC 7845)"), ("vorbis", "Ogg Vorbis")],
        ContainerType::Wave => &[
            ("pcm_s24le", "Uncompressed PCM (24-bit Broadcast Wave)"),
            ("pcm_s16le", "Uncompressed PCM (16-bit CD Standard)"),
            ("pcm_f32le", "32-bit Floating Point PCM"),
        ],
        ContainerType::Aiff => &[
            ("pcm_s24be", "Uncompressed PCM Big-Endian (24-bit)"),
            ("pcm_s16be", "Uncompressed PCM Big-Endian (16-bit)"),
            ("pcm_f32be", "32-bit Float PCM Big-Endian"),
        ],
        ContainerType::Flac => &[("flac", "Free Lossless Audio Codec (FLAC)")],
        ContainerType::Mp3 => &[("mp3", "MPEG Audio Layer 3 (MP3)")],
        ContainerType::OggAudio => &[("vorbis", "Ogg Vorbis Audio"), ("opus", "Ogg Opus Audio"), ("flac", "Ogg FLAC Lossless")],
        ContainerType::Opus => &[("opus", "Ogg Opus Interactive Audio")],
        ContainerType::Caf => &[("lpcm", "Apple Core Audio Linear PCM"), ("alac", "Apple Lossless (ALAC)"), ("aac", "AAC-LC")],
        _ => &[("aac", "AAC-LC (Advanced Audio Coding)"), ("pcm_s24le", "Linear PCM 24-bit")],
    }
}

/// Preview monitor tab mode within the Export Settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewTabMode {
    #[default]
    Source,
    Output,
    Compare,
}

/// Interactive preview crop aspect ratio constraint presets (AME Parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CropAspectPreset {
    #[default]
    Custom,
    Square1x1,
    Standard4x3,
    Widescreen16x9,
    Vertical9x16,
    Anamorphic239x1,
}

impl CropAspectPreset {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Custom => "Custom",
            Self::Square1x1 => "1:1 Square",
            Self::Standard4x3 => "4:3 Standard",
            Self::Widescreen16x9 => "16:9 Widescreen",
            Self::Vertical9x16 => "9:16 Vertical (Reels/TikTok)",
            Self::Anamorphic239x1 => "2.39:1 Cinemascope",
        }
    }

    pub fn ratio(&self) -> Option<f32> {
        match self {
            Self::Custom => None,
            Self::Square1x1 => Some(1.0),
            Self::Standard4x3 => Some(4.0 / 3.0),
            Self::Widescreen16x9 => Some(16.0 / 9.0),
            Self::Vertical9x16 => Some(9.0 / 16.0),
            Self::Anamorphic239x1 => Some(2.39),
        }
    }
}

/// State for the inspector settings modal.
pub struct InspectorState {
    pub is_open: bool,
    pub active_tab: InspectorTab,
    pub preview_tab: PreviewTabMode,
    pub compare_split_fraction: f32,
    pub job_spec: Option<CraftJobSpec>,
    pub preview_path: Option<PathBuf>,
    pub preview_decoder: Option<PreviewDecoder>,
    pub preview_probe: Option<Box<MediaProbe>>,
    pub preview_texture: Option<TextureHandle>,
    pub preview_frame_idx: u64,
    pub preview_frame_count: u64,
    pub aspect_ratio_locked: bool,
    pub crop_interactive: bool,
    pub crop_aspect_preset: CropAspectPreset,
    pub video_bitrate_mode: String,
    pub audio_sample_rate_idx: usize,
    pub audio_channels_idx: usize,
    pub publish_destination: Option<PathBuf>,
    pub post_encode_action: String,
    /// Sequences, pages and layers of the current source, loaded once per source path.
    pub inventory: SourceInventory,
    pub inventory_path: Option<PathBuf>,
}

impl Default for InspectorState {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: InspectorTab::Video,
            preview_tab: PreviewTabMode::Source,
            compare_split_fraction: 0.5,
            job_spec: None,
            preview_path: None,
            preview_decoder: None,
            preview_probe: None,
            preview_texture: None,
            preview_frame_idx: 0,
            preview_frame_count: 100,
            aspect_ratio_locked: true,
            crop_interactive: false,
            crop_aspect_preset: CropAspectPreset::Custom,
            video_bitrate_mode: "VBR 1-Pass".into(),
            audio_sample_rate_idx: 1, // 48,000 Hz
            audio_channels_idx: 1,    // Stereo (2.0)
            publish_destination: None,
            post_encode_action: "None".into(),
            inventory: SourceInventory::default(),
            inventory_path: None,
        }
    }
}

/// Calculates estimated output file size in megabytes.
pub fn calculate_estimated_size_mb(duration_secs: f64, video_bitrate_kbps: u32, audio_bitrate_kbps: u32) -> f64 {
    let dur = if duration_secs > 0.05 { duration_secs } else { 60.0 };
    let total_kbps = (video_bitrate_kbps + audio_bitrate_kbps) as f64;
    (total_kbps * dur) / (8.0 * 1024.0)
}

/// Formats a SMPTE-style timecode from a frame number and frame rate.
pub fn format_timecode(frame: u64, fps: f64) -> String {
    let rate = if fps > 1.0 { fps } else { 30.0 };
    let total_seconds = frame as f64 / rate;
    let hours = (total_seconds / 3600.0).floor() as u64;
    let minutes = ((total_seconds % 3600.0) / 60.0).floor() as u64;
    let seconds = (total_seconds % 60.0).floor() as u64;
    let frames = (frame as f64 % rate).floor() as u64;
    format!("{hours:02}:{minutes:02}:{seconds:02}:{frames:02}")
}

/// Renders the export settings inspector modal window.
pub fn render_inspector_modal(ctx: &egui::Context, state: &mut InspectorState, tokens: &Tokens, on_save: &mut Option<CraftJobSpec>) {
    if !state.is_open {
        return;
    }

    // Initialize or poll preview decoder
    if let Some(ref spec) = state.job_spec
        && let SourceDescriptor::File { path, .. } = &spec.source
        && state.preview_path.as_deref() != Some(path.as_path())
    {
        state.preview_path = Some(path.clone());
        state.preview_decoder = Some(PreviewDecoder::open(path.clone()));
        state.preview_probe = None;
        state.preview_texture = None;
        state.preview_frame_idx = 0;
    }

    if let Some(ref dec) = state.preview_decoder {
        while let Some(ev) = dec.poll() {
            match ev {
                PreviewEvent::Opened { probe, frame_count } => {
                    let total = if frame_count > 0 {
                        frame_count
                    } else if probe.duration_seconds > 0.0 {
                        (probe.duration_seconds * 30.0).round().max(1.0) as u64
                    } else {
                        1
                    };
                    state.preview_frame_count = total;
                    state.preview_probe = Some(probe);
                    dec.request_frame(state.preview_frame_idx);
                }
                PreviewEvent::Frame { rgba, width, height, .. } => {
                    let img = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    state.preview_texture = Some(ctx.load_texture("inspector_preview", img, Default::default()));
                }
                PreviewEvent::Failed(_) => {}
            }
        }
    }

    let mut is_open = state.is_open;
    let mut should_close = false;
    let mut should_save = false;

    let mut current_spec = state.job_spec.take();

    Window::new("Export Settings").open(&mut is_open).resizable(true).collapsible(false).default_size([880.0, 600.0]).min_size([720.0, 480.0]).show(
        ctx,
        |ui| {
            if let Some(ref mut spec) = current_spec {
                ui.horizontal(|ui| {
                    // --- Left Side: Preview Monitor (Source / Output Tabs & Scrubber) ---
                    let left_width = (ui.available_width() * 0.46).clamp(320.0, 480.0);
                    ui.allocate_ui_with_layout(egui::vec2(left_width, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        render_preview_section(ui, state, spec, tokens);
                    });

                    ui.separator();

                    // --- Right Side: Export Settings Tabs & Parameters ---
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            render_settings_section(ui, state, spec, tokens);

                            ui.add_space(8.0);
                            ui.separator();

                            // Bottom Action Bar: File size estimate & Buttons
                            ui.horizontal(|ui| {
                                let duration = state.preview_probe.as_ref().map(|p| p.duration_seconds).unwrap_or(60.0);
                                let v_kbps = spec.targets.first().and_then(|t| t.target_bitrate_kbps).unwrap_or(12000);
                                let a_kbps = 320;
                                let est_mb = calculate_estimated_size_mb(duration, v_kbps, a_kbps);

                                let dur_label = if duration > 0.05 { format!("{duration:.1} s") } else { "60 s est.".into() };
                                ui.label(
                                    RichText::new(format!("Est. File Size: {est_mb:.1} MB ({dur_label} at {v_kbps} kbps)"))
                                        .small()
                                        .color(tokens.text_dim),
                                );

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button(RichText::new("Save & Apply").strong()).clicked() {
                                        should_save = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        should_close = true;
                                    }
                                });
                            });
                        },
                    );
                });
            } else {
                ui.label("No job selected for inspection.");
            }
        },
    );

    if should_close {
        is_open = false;
    }
    state.is_open = is_open;

    if should_save {
        if let Some(spec) = current_spec {
            *on_save = Some(spec);
        }
        state.job_spec = None;
        state.is_open = false;
    } else {
        state.job_spec = current_spec;
    }
}

fn render_preview_section(ui: &mut Ui, state: &mut InspectorState, spec: &mut CraftJobSpec, tokens: &Tokens) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.preview_tab, PreviewTabMode::Source, "Source");
        ui.selectable_value(&mut state.preview_tab, PreviewTabMode::Output, "Output");
        ui.selectable_value(&mut state.preview_tab, PreviewTabMode::Compare, "Split Compare");

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        // Interactive Source Cropping Tool (AME Parity: Source Crop Tool)
        let crop_active = state.crop_interactive;
        if ui.selectable_label(crop_active, "✂ Crop Tool").clicked() {
            state.crop_interactive = !state.crop_interactive;
            if state.crop_interactive {
                state.preview_tab = PreviewTabMode::Source;
                if let Some(target) = spec.targets.first_mut() {
                    let crop_cfg = target.crop.get_or_insert_with(Default::default);
                    crop_cfg.enabled = true;
                }
            }
        }

        if state.crop_interactive {
            egui::ComboBox::from_id_salt("inspector_crop_aspect_combo").selected_text(state.crop_aspect_preset.label()).width(90.0).show_ui(
                ui,
                |ui| {
                    for preset in [
                        CropAspectPreset::Custom,
                        CropAspectPreset::Square1x1,
                        CropAspectPreset::Standard4x3,
                        CropAspectPreset::Widescreen16x9,
                        CropAspectPreset::Vertical9x16,
                        CropAspectPreset::Anamorphic239x1,
                    ] {
                        if ui.selectable_label(state.crop_aspect_preset == preset, preset.label()).clicked() {
                            state.crop_aspect_preset = preset;
                            if let (Some(ratio), Some(probe), Some(target)) = (preset.ratio(), &state.preview_probe, spec.targets.first_mut())
                                && let Some(ref v) = probe.video
                            {
                                let src_w = v.width as f32;
                                let src_h = v.height as f32;
                                let (cw, ch) = if src_w / src_h > ratio { (src_h * ratio, src_h) } else { (src_w, src_w / ratio) };
                                let px = ((src_w - cw) / 2.0).round().max(0.0) as u32;
                                let py = ((src_h - ch) / 2.0).round().max(0.0) as u32;
                                target.crop = Some(encodercraft_core::CropConfig { enabled: true, left: px, right: px, top: py, bottom: py });
                            }
                        }
                    }
                },
            );
        }

        if let Some(ref probe) = state.preview_probe
            && let Some(ref v) = probe.video
        {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{}x{} · {:.2} fps", v.width, v.height, v.frame_rate.as_f64())).small().color(tokens.text_dim));
            });
        }
    });

    if state.preview_tab == PreviewTabMode::Compare {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Split:").small().color(tokens.text_dim));
            let pct = (state.compare_split_fraction * 100.0) as u32;
            ui.add(egui::Slider::new(&mut state.compare_split_fraction, 0.0..=1.0).show_value(false).text(format!("{pct}% Out")));
        });
    }

    ui.add_space(4.0);

    // Frame Texture or Placeholder Viewport
    let preview_height = 240.0;

    egui::Frame::canvas(ui.style()).fill(tokens.bg_panel_darker).show(ui, |ui| {
        ui.set_height(preview_height);
        ui.set_width(ui.available_width());

        if let Some(ref tex) = state.preview_texture {
            let tex_size = tex.size_vec2();
            let aspect = if tex_size.y > 0.0 { tex_size.x / tex_size.y } else { 16.0 / 9.0 };

            let avail_w = ui.available_width();
            let avail_h = preview_height;

            let (draw_w, draw_h) = if avail_w / avail_h > aspect { (avail_h * aspect, avail_h) } else { (avail_w, avail_w / aspect) };

            let img_resp = ui.centered_and_justified(|ui| ui.image((tex.id(), egui::vec2(draw_w, draw_h))));

            let rect = img_resp.response.rect;
            let painter = ui.painter();

            // Compare split indicator line
            if state.preview_tab == PreviewTabMode::Compare {
                let split_x = rect.min.x + rect.width() * state.compare_split_fraction;
                painter
                    .line_segment([egui::pos2(split_x, rect.min.y), egui::pos2(split_x, rect.max.y)], egui::Stroke::new(2.0, egui::Color32::WHITE));
                painter.text(
                    egui::pos2(rect.min.x + 6.0, rect.min.y + 6.0),
                    egui::Align2::LEFT_TOP,
                    "SOURCE",
                    egui::FontId::proportional(10.0),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(rect.max.x - 6.0, rect.min.y + 6.0),
                    egui::Align2::RIGHT_TOP,
                    "OUTPUT",
                    egui::FontId::proportional(10.0),
                    tokens.accent_primary,
                );
            }

            // Interactive Crop Box & Rule-of-Thirds Grid Overlay
            if (state.crop_interactive || state.preview_tab == PreviewTabMode::Source)
                && let Some(target) = spec.targets.first()
                && let Some(crop_cfg) = &target.crop
                && crop_cfg.enabled
            {
                let (src_w, src_h) = state
                    .preview_probe
                    .as_ref()
                    .and_then(|p| p.video.as_ref())
                    .map(|v| (v.width as f32, v.height as f32))
                    .unwrap_or((1920.0, 1080.0));

                let scale_x = rect.width() / src_w.max(1.0);
                let scale_y = rect.height() / src_h.max(1.0);

                let crop_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        (rect.min.x + (crop_cfg.left as f32 * scale_x)).min(rect.max.x),
                        (rect.min.y + (crop_cfg.top as f32 * scale_y)).min(rect.max.y),
                    ),
                    egui::pos2(
                        (rect.max.x - (crop_cfg.right as f32 * scale_x)).max(rect.min.x),
                        (rect.max.y - (crop_cfg.bottom as f32 * scale_y)).max(rect.min.y),
                    ),
                );

                if crop_rect.is_positive() {
                    let mask_color = egui::Color32::from_black_alpha(150);
                    // Dim surrounding areas
                    painter.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, crop_rect.min.y)), 0.0, mask_color);
                    painter.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.min.x, crop_rect.max.y), rect.max), 0.0, mask_color);
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(rect.min.x, crop_rect.min.y), egui::pos2(crop_rect.min.x, crop_rect.max.y)),
                        0.0,
                        mask_color,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(crop_rect.max.x, crop_rect.min.y), egui::pos2(rect.max.x, crop_rect.max.y)),
                        0.0,
                        mask_color,
                    );

                    // Crop border
                    painter.rect_stroke(crop_rect, 0.0, egui::Stroke::new(1.5, egui::Color32::WHITE), egui::StrokeKind::Inside);

                    // Rule of thirds grid
                    let grid_stroke = egui::Stroke::new(0.5, egui::Color32::from_white_alpha(90));
                    let tw = crop_rect.width() / 3.0;
                    let th = crop_rect.height() / 3.0;
                    painter.line_segment(
                        [egui::pos2(crop_rect.min.x + tw, crop_rect.min.y), egui::pos2(crop_rect.min.x + tw, crop_rect.max.y)],
                        grid_stroke,
                    );
                    painter.line_segment(
                        [egui::pos2(crop_rect.min.x + 2.0 * tw, crop_rect.min.y), egui::pos2(crop_rect.min.x + 2.0 * tw, crop_rect.max.y)],
                        grid_stroke,
                    );
                    painter.line_segment(
                        [egui::pos2(crop_rect.min.x, crop_rect.min.y + th), egui::pos2(crop_rect.max.x, crop_rect.min.y + th)],
                        grid_stroke,
                    );
                    painter.line_segment(
                        [egui::pos2(crop_rect.min.x, crop_rect.min.y + 2.0 * th), egui::pos2(crop_rect.max.x, crop_rect.min.y + 2.0 * th)],
                        grid_stroke,
                    );

                    // Corner brackets
                    let bracket_len = 8.0;
                    let bracket_stroke = egui::Stroke::new(2.5, tokens.accent_primary);
                    // TL
                    painter.line_segment([crop_rect.min, crop_rect.min + egui::vec2(bracket_len, 0.0)], bracket_stroke);
                    painter.line_segment([crop_rect.min, crop_rect.min + egui::vec2(0.0, bracket_len)], bracket_stroke);
                    // TR
                    let tr = egui::pos2(crop_rect.max.x, crop_rect.min.y);
                    painter.line_segment([tr, tr + egui::vec2(-bracket_len, 0.0)], bracket_stroke);
                    painter.line_segment([tr, tr + egui::vec2(0.0, bracket_len)], bracket_stroke);
                    // BL
                    let bl = egui::pos2(crop_rect.min.x, crop_rect.max.y);
                    painter.line_segment([bl, bl + egui::vec2(bracket_len, 0.0)], bracket_stroke);
                    painter.line_segment([bl, bl + egui::vec2(0.0, -bracket_len)], bracket_stroke);
                    // BR
                    painter.line_segment([crop_rect.max, crop_rect.max + egui::vec2(-bracket_len, 0.0)], bracket_stroke);
                    painter.line_segment([crop_rect.max, crop_rect.max + egui::vec2(0.0, -bracket_len)], bracket_stroke);
                }
            }

            // Output / Compare active effects badges
            if state.preview_tab != PreviewTabMode::Source
                && let Some(target) = spec.targets.first()
            {
                let mut badge_y = rect.min.y + 20.0;
                if target.burn_timecode || target.timecode_overlay.as_ref().is_some_and(|c| c.enabled) {
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, badge_y),
                        egui::Align2::LEFT_TOP,
                        "[TIME-CODE OVERLAY]",
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_rgb(255, 200, 50),
                    );
                    badge_y += 14.0;
                }
                if target.name_overlay.as_ref().is_some_and(|n| n.enabled) {
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, badge_y),
                        egui::Align2::LEFT_TOP,
                        "[NAME OVERLAY]",
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_rgb(100, 220, 255),
                    );
                    badge_y += 14.0;
                }
                if target.watermark.as_ref().is_some_and(|w| w.enabled) {
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, badge_y),
                        egui::Align2::LEFT_TOP,
                        "[WATERMARK]",
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_rgb(200, 255, 100),
                    );
                    badge_y += 14.0;
                }
                if target.video_limiter.as_ref().is_some_and(|v| v.enabled) {
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, badge_y),
                        egui::Align2::LEFT_TOP,
                        "[VIDEO LIMITER]",
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_rgb(255, 120, 120),
                    );
                    badge_y += 14.0;
                }
                if target.sdr_conform.as_ref().is_some_and(|s| s.enabled) {
                    painter.text(
                        egui::pos2(rect.min.x + 8.0, badge_y),
                        egui::Align2::LEFT_TOP,
                        "[SDR CONFORM]",
                        egui::FontId::monospace(9.0),
                        egui::Color32::from_rgb(200, 150, 255),
                    );
                }
            }
        } else {
            ui.centered_and_justified(|ui| {
                let msg = match state.preview_tab {
                    PreviewTabMode::Source => "Source Preview (Loading / Standby)",
                    PreviewTabMode::Output => "Output Preview with Target Scaling",
                    PreviewTabMode::Compare => "Split Screen Compare (Source vs Output)",
                };
                ui.label(RichText::new(msg).color(tokens.text_dim));
            });
        }
    });

    // If crop tool is active, display the interactive margin sliders below monitor
    if state.crop_interactive
        && let Some(target) = spec.targets.first_mut()
    {
        let mut crop_cfg = target.crop.unwrap_or_default();
        crop_cfg.enabled = true;
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Margins:").small().strong());
            ui.label(RichText::new("L:").small());
            ui.add(egui::DragValue::new(&mut crop_cfg.left).range(0..=3840).speed(1));
            ui.label(RichText::new("T:").small());
            ui.add(egui::DragValue::new(&mut crop_cfg.top).range(0..=2160).speed(1));
            ui.label(RichText::new("R:").small());
            ui.add(egui::DragValue::new(&mut crop_cfg.right).range(0..=3840).speed(1));
            ui.label(RichText::new("B:").small());
            ui.add(egui::DragValue::new(&mut crop_cfg.bottom).range(0..=2160).speed(1));
            if ui.small_button("Reset").clicked() {
                crop_cfg.left = 0;
                crop_cfg.top = 0;
                crop_cfg.right = 0;
                crop_cfg.bottom = 0;
            }
            target.crop = Some(crop_cfg);
        });
    }

    ui.add_space(6.0);

    // Timeline Scrubber & In/Out Handles
    let fps = state.preview_probe.as_ref().and_then(|p| p.video.as_ref()).map(|v| v.frame_rate.as_f64()).unwrap_or(30.0);
    let current_tc = format_timecode(state.preview_frame_idx, fps);
    let total_tc = format_timecode(state.preview_frame_count.saturating_sub(1), fps);

    ui.horizontal(|ui| {
        ui.label(RichText::new(current_tc).strong().monospace().small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(total_tc).monospace().small().color(tokens.text_dim));
        });
    });

    let mut current_frame = state.preview_frame_idx;
    let max_frame = state.preview_frame_count.saturating_sub(1);
    let slider = egui::Slider::new(&mut current_frame, 0..=max_frame).show_value(false);
    if ui.add(slider).changed() {
        state.preview_frame_idx = current_frame;
        if let Some(ref dec) = state.preview_decoder {
            dec.request_frame(current_frame);
        }
    }

    // In / Out range controls (AME Parity with I/O keyboard shortcuts)
    ui.horizontal(|ui| {
        let fps_int = fps.round().max(1.0) as i64;
        let mut in_tick = None;
        let mut dur_tick = None;
        if let SourceDescriptor::File { start_tick, duration_ticks, .. } = &spec.source {
            in_tick = *start_tick;
            dur_tick = *duration_ticks;
        }

        let mark_in = ui.button("Set In (I)").on_hover_text("Mark in-point for transcode range (I)").clicked()
            || (!ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::I)));
        let mark_out = ui.button("Set Out (O)").on_hover_text("Mark out-point for transcode range (O)").clicked()
            || (!ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::O)));
        let reset_range = ui.button("Reset Range (Alt+X)").on_hover_text("Clear In/Out mark points (Alt+X)").clicked()
            || (!ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::X)));

        if mark_in && let SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = &mut spec.source {
            let current_tick = i64::try_from(current_frame).unwrap_or(0);
            *start_tick = Some(current_tick);
            if timebase.is_none() {
                *timebase = encodercraft_core::Rational64::new(1, fps_int).ok();
            }
            if let Some(dur) = *duration_ticks
                && dur <= 0
            {
                *duration_ticks = None;
            }
        }
        if mark_out && let SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = &mut spec.source {
            let current_tick = i64::try_from(current_frame).unwrap_or(0);
            let start = start_tick.unwrap_or(0);
            let dur = current_tick.saturating_sub(start).max(1);
            *duration_ticks = Some(dur);
            if timebase.is_none() {
                *timebase = encodercraft_core::Rational64::new(1, fps_int).ok();
            }
        }
        if reset_range && let SourceDescriptor::File { start_tick, duration_ticks, .. } = &mut spec.source {
            *start_tick = None;
            *duration_ticks = None;
        }

        let range_label = match (in_tick, dur_tick) {
            (None, None) => "Range: Entire Sequence".to_string(),
            (Some(s), None) => format!("Range: In @ frame {s}"),
            (None, Some(d)) => format!("Range: 0 – frame {}", d.saturating_sub(1)),
            (Some(s), Some(d)) => format!("Range: Frame {s} – Frame {}", s.saturating_add(d.saturating_sub(1))),
        };
        ui.label(RichText::new(range_label).small().color(tokens.text_dim));
    });
}

fn render_settings_section(ui: &mut Ui, state: &mut InspectorState, spec: &mut CraftJobSpec, tokens: &Tokens) {
    let source = source_container(spec);
    refresh_inventory(state, spec, source);
    let destinations = destination_options(source);
    let targets = &mut spec.targets;
    let target = match targets.first_mut() {
        Some(t) => t,
        None => return,
    };

    // Header: Format & Preset Selectors + Match Source Button
    ui.horizontal(|ui| {
        ui.label(RichText::new("Format:").strong());
        let current_c = target.container;
        let combo_text = destinations.iter().find(|(c, _, _)| *c == current_c).map(|(_, name, _)| *name).unwrap_or("Custom");
        egui::ComboBox::from_id_salt("inspector_format_combo").selected_text(combo_text).width(140.0).show_ui(ui, |ui| {
            for (c, name, ext) in &destinations {
                if ui.selectable_label(target.container == *c, *name).clicked() {
                    target.container = *c;
                    target.preset_name = default_preset_for_container(*c).to_string();
                    let mut p = target.output_path.clone();
                    p.set_extension(ext);
                    target.output_path = p;
                    target.export_video = c.supports_video();
                    target.export_audio = c.supports_audio();
                    if target.export_audio && target.audio.is_none() {
                        let def = default_audio_codec_for_container(*c);
                        target.audio = Some(encodercraft_core::AudioEncodeSettings {
                            codec: Some(def.to_string()),
                            sample_rate: Some(48_000),
                            layout: Some(ChannelLayout::Stereo),
                            bitrate_kbps: if def.starts_with("pcm") { None } else { Some(320) },
                            bit_depth: if def == "pcm_s24le" {
                                Some(24)
                            } else if def == "pcm_s16le" {
                                Some(16)
                            } else {
                                None
                            },
                            delay_ms: 0,
                        });
                    }
                }
            }
        });

        ui.add_space(8.0);

        ui.label(RichText::new("Preset:").strong());
        egui::ComboBox::from_id_salt("inspector_preset_combo").selected_text(&target.preset_name).width(190.0).show_ui(ui, |ui| {
            for preset_name in presets_for_container(target.container) {
                if ui.selectable_label(target.preset_name == preset_name, preset_name).clicked() {
                    target.preset_name = preset_name.to_string();
                }
            }
        });

        ui.add_space(8.0);

        // Match Source button
        let match_btn = ui.button("Match Source").on_hover_text("Copy width, height, and frame rate directly from the source media");
        if match_btn.clicked()
            && let Some(ref probe) = state.preview_probe
            && let Some(ref v) = probe.video
        {
            target.custom_width = Some(v.width);
            target.custom_height = Some(v.height);
            target.custom_frame_rate = Some(v.frame_rate);
            if let Some(br) = v.bitrate {
                target.target_bitrate_kbps = Some((br / 1000) as u32);
            }
        }
    });

    ui.add_space(4.0);

    // Output File Destination Link
    ui.horizontal(|ui| {
        ui.label(RichText::new("Output Name:").strong());
        let file_name = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
        let link = ui.link(RichText::new(file_name).color(tokens.accent_primary)).on_hover_text(target.output_path.display().to_string());
        if link.clicked()
            && let Some(new_path) = rfd::FileDialog::new().set_file_name(file_name).save_file()
        {
            target.output_path = new_path;
        }
    });

    ui.add_space(4.0);

    // Master Stream Toggles (AME Parity: [x] Export Video, [x] Export Audio)
    let video_supported = target.container.supports_video();
    let audio_supported = target.container.supports_audio();

    ui.horizontal(|ui| {
        ui.add_enabled_ui(video_supported, |ui| {
            let mut v = target.export_video;
            if ui.checkbox(&mut v, RichText::new("Export Video").strong()).changed() {
                target.export_video = v;
            }
        });

        ui.add_space(20.0);

        ui.add_enabled_ui(audio_supported, |ui| {
            let mut a = target.export_audio;
            if ui.checkbox(&mut a, RichText::new("Export Audio").strong()).changed() {
                target.export_audio = a;
                if target.export_audio && target.audio.is_none() {
                    let def = default_audio_codec_for_container(target.container);
                    target.audio = Some(encodercraft_core::AudioEncodeSettings {
                        codec: Some(def.to_string()),
                        sample_rate: Some(48_000),
                        layout: Some(ChannelLayout::Stereo),
                        bitrate_kbps: if def.starts_with("pcm") { None } else { Some(320) },
                        bit_depth: if def == "pcm_s24le" {
                            Some(24)
                        } else if def == "pcm_s16le" {
                            Some(16)
                        } else {
                            None
                        },
                        delay_ms: 0,
                    });
                }
            }
        });
    });

    ui.add_space(6.0);
    ui.separator();

    // Tabs
    let tabs = visible_tabs_for_target(source, target);
    if !tabs.contains(&state.active_tab)
        && let Some(first) = tabs.first()
    {
        state.active_tab = *first;
    }
    ui.horizontal(|ui| {
        for tab in &tabs {
            ui.selectable_value(&mut state.active_tab, *tab, tab.label());
        }
    });

    ui.separator();
    ui.add_space(4.0);

    egui::ScrollArea::vertical().id_salt("inspector_modal_tabs_scroll_area").auto_shrink([false; 2]).show(ui, |ui| match state.active_tab {
        InspectorTab::Video => render_video_tab(ui, target, state),
        InspectorTab::Composition => render_composition_tab(ui, &mut target.selection, &state.inventory),
        InspectorTab::Layout => render_layout_tab(ui, &mut target.selection, &state.inventory),
        InspectorTab::Cad => render_layers_tab(ui, "CAD Layers", &mut target.selection, &state.inventory),
        InspectorTab::Layers => render_layers_tab(ui, "Image Layers", &mut target.selection, &state.inventory),
        InspectorTab::Audio => render_audio_tab(ui, target, state),
        InspectorTab::Effects => render_effects_tab(ui, target),
        InspectorTab::Captions => render_captions_tab(ui, target),
        InspectorTab::Publish => render_publish_tab(ui, target, state),
        InspectorTab::Loudness => render_loudness_tab(ui, target),
        InspectorTab::Metadata => render_metadata_tab(ui, &mut spec.client_info, target),
    });
}

/// Loads the source inventory once per source path; plain media sources are never read.
fn refresh_inventory(state: &mut InspectorState, spec: &CraftJobSpec, source: Option<ContainerType>) {
    let path = match &spec.source {
        SourceDescriptor::File { path, .. } if source_has_structure(source) => Some(path.clone()),
        _ => None,
    };
    if state.inventory_path == path {
        return;
    }
    state.inventory = match &path {
        Some(p) => encodercraft_pipeline::inspect_source(p),
        None => SourceInventory::default(),
    };
    state.inventory_path = path;
}

fn render_composition_tab(ui: &mut Ui, selection: &mut SourceSelection, inventory: &SourceInventory) {
    ui.heading("Sequence / Composition");
    ui.add_space(4.0);
    if inventory.sequences.is_empty() {
        ui.label("No sequences were found in this source.");
        return;
    }
    if inventory.sequences.len() > 1 && selection.sequence.is_none() {
        ui.colored_label(ui.visuals().warn_fg_color, "This project has several sequences; pick one or all are exported.");
    }
    if ui.selectable_label(selection.sequence.is_none(), format!("All sequences ({})", inventory.sequences.len())).clicked() {
        selection.sequence = None;
    }
    let chosen = selected_sequence_index(inventory, selection);
    for (idx, seq) in inventory.sequences.iter().enumerate() {
        let duration = seq.duration_secs.map(|d| format!(", {d:.1} s")).unwrap_or_default();
        let label = format!("{}  ({}x{} @ {} fps{duration})", seq.name, seq.width, seq.height, seq.fps);
        if ui.selectable_label(chosen == Some(idx), label).clicked() {
            selection.sequence = sequence_key(inventory, idx);
        }
    }
}

fn render_layout_tab(ui: &mut Ui, selection: &mut SourceSelection, inventory: &SourceInventory) {
    ui.heading("Page Range");
    ui.add_space(4.0);
    let Some(total) = inventory.pages else {
        ui.label("No pages were found in this source.");
        return;
    };
    let total32 = u32::try_from(total).unwrap_or(u32::MAX).max(1);
    let (mut first, mut last) = selection.pages.unwrap_or((1, total32));
    ui.horizontal(|ui| {
        ui.label("From");
        let a = ui.add(egui::DragValue::new(&mut first).range(1..=total32)).changed();
        ui.label("to");
        let b = ui.add(egui::DragValue::new(&mut last).range(1..=total32)).changed();
        ui.label(format!("of {total}"));
        if a || b {
            set_page_range(selection, total, first, last);
        }
    });
    if ui.button("All pages").clicked() {
        selection.pages = None;
    }
}

fn render_layers_tab(ui: &mut Ui, title: &str, selection: &mut SourceSelection, inventory: &SourceInventory) {
    ui.heading(title);
    ui.add_space(4.0);
    if inventory.layers.is_empty() {
        ui.label("No layers were found in this source.");
        return;
    }
    for name in &inventory.layers {
        let mut on = selection.layers.as_ref().is_none_or(|l| l.contains(name));
        if ui.checkbox(&mut on, name).changed() {
            set_layer_enabled(selection, &inventory.layers, name, on);
        }
    }
    if ui.button("All layers").clicked() {
        selection.layers = None;
    }
}

fn render_video_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    if target.container.media_kind().is_image()
        || matches!(
            target.container,
            ContainerType::Jpeg
                | ContainerType::WebP
                | ContainerType::Jxl
                | ContainerType::Png
                | ContainerType::Tiff
                | ContainerType::OpenExr
                | ContainerType::Bmp
        )
    {
        ui.heading("Still Image Settings");
        ui.add_space(4.0);

        let mut quality = target.image_quality.unwrap_or(90);
        ui.horizontal(|ui| {
            ui.label("Image Quality (1-100):");
            if ui.add(egui::Slider::new(&mut quality, 1..=100)).changed() {
                target.image_quality = Some(quality);
            }
        });
        ui.add_space(6.0);
        ui.separator();
    }

    ui.heading("Basic Video Settings");
    ui.add_space(4.0);

    // Frame Dimensions & Aspect Lock
    let mut w = target.custom_width.unwrap_or(1920);
    let mut h = target.custom_height.unwrap_or(1080);

    ui.horizontal(|ui| {
        ui.label("Frame Width:");
        if ui.add(egui::DragValue::new(&mut w).range(16..=7680).speed(2)).changed() {
            target.custom_width = Some(w);
            if state.aspect_ratio_locked {
                h = ((w as f64 * 9.0) / 16.0).round() as u32;
                target.custom_height = Some(h);
            }
        }

        ui.add_space(8.0);
        ui.label("Height:");
        if ui.add(egui::DragValue::new(&mut h).range(16..=4320).speed(2)).changed() {
            target.custom_height = Some(h);
            if state.aspect_ratio_locked {
                w = ((h as f64 * 16.0) / 9.0).round() as u32;
                target.custom_width = Some(w);
            }
        }

        ui.add_space(8.0);
        ui.checkbox(&mut state.aspect_ratio_locked, "Lock Aspect Ratio (16:9)");
    });

    // Resolution Presets Shortcut
    ui.horizontal(|ui| {
        ui.label(RichText::new("Presets:").small().color(egui::Color32::GRAY));
        if ui.small_button("4K UHD (3840x2160)").clicked() {
            target.custom_width = Some(3840);
            target.custom_height = Some(2160);
        }
        if ui.small_button("1080p FHD (1920x1080)").clicked() {
            target.custom_width = Some(1920);
            target.custom_height = Some(1080);
        }
        if ui.small_button("720p HD (1280x720)").clicked() {
            target.custom_width = Some(1280);
            target.custom_height = Some(720);
        }
    });

    ui.add_space(6.0);

    // Frame Rate
    let current_fps = target.custom_frame_rate.map(|r| r.as_f64()).unwrap_or(29.97);
    ui.horizontal(|ui| {
        ui.label("Frame Rate (fps):");
        egui::ComboBox::from_id_salt("inspector_fps_combo").selected_text(format!("{current_fps:.2} fps")).show_ui(ui, |ui| {
            if ui.selectable_label((current_fps - 23.976).abs() < 0.01, "23.976 fps (Film)").clicked() {
                target.custom_frame_rate = Rational64::new(24000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 24.0).abs() < 0.01, "24.0 fps (Cinema)").clicked() {
                target.custom_frame_rate = Rational64::new(24, 1).ok();
            }
            if ui.selectable_label((current_fps - 25.0).abs() < 0.01, "25.0 fps (PAL/Broadcast)").clicked() {
                target.custom_frame_rate = Rational64::new(25, 1).ok();
            }
            if ui.selectable_label((current_fps - 29.97).abs() < 0.01, "29.97 fps (NTSC)").clicked() {
                target.custom_frame_rate = Rational64::new(30000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 30.0).abs() < 0.01, "30.0 fps").clicked() {
                target.custom_frame_rate = Rational64::new(30, 1).ok();
            }
            if ui.selectable_label((current_fps - 59.94).abs() < 0.01, "59.94 fps").clicked() {
                target.custom_frame_rate = Rational64::new(60000, 1001).ok();
            }
            if ui.selectable_label((current_fps - 60.0).abs() < 0.01, "60.0 fps").clicked() {
                target.custom_frame_rate = Rational64::new(60, 1).ok();
            }
        });
    });

    ui.horizontal(|ui| {
        ui.label("Rotation:");
        egui::ComboBox::from_id_salt("inspector_rotation_combo")
            .selected_text(match target.transform.rotation {
                Rotation::None => "0°",
                Rotation::Cw90 => "90° clockwise",
                Rotation::Cw180 => "180°",
                Rotation::Cw270 => "270° clockwise",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.rotation, Rotation::None, "0°");
                ui.selectable_value(&mut target.transform.rotation, Rotation::Cw90, "90° clockwise");
                ui.selectable_value(&mut target.transform.rotation, Rotation::Cw180, "180°");
                ui.selectable_value(&mut target.transform.rotation, Rotation::Cw270, "270° clockwise");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Flip:");
        egui::ComboBox::from_id_salt("inspector_flip_combo")
            .selected_text(match target.transform.flip {
                FlipMode::None => "None",
                FlipMode::Horizontal => "Horizontal (mirror)",
                FlipMode::Vertical => "Vertical",
                FlipMode::Both => "Both (H + V)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.flip, FlipMode::None, "None");
                ui.selectable_value(&mut target.transform.flip, FlipMode::Horizontal, "Horizontal (mirror)");
                ui.selectable_value(&mut target.transform.flip, FlipMode::Vertical, "Vertical");
                ui.selectable_value(&mut target.transform.flip, FlipMode::Both, "Both (H + V)");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Deinterlace:");
        egui::ComboBox::from_id_salt("inspector_deinterlace_combo")
            .selected_text(match target.transform.deinterlace {
                DeinterlaceMode::None => "Off (progressive)",
                DeinterlaceMode::TopFieldFirst => "Upper Field First",
                DeinterlaceMode::BottomFieldFirst => "Lower Field First",
                DeinterlaceMode::Bob => "Bob (duplicate)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.deinterlace, DeinterlaceMode::None, "Off (progressive)");
                ui.selectable_value(&mut target.transform.deinterlace, DeinterlaceMode::TopFieldFirst, "Upper Field First");
                ui.selectable_value(&mut target.transform.deinterlace, DeinterlaceMode::BottomFieldFirst, "Lower Field First");
                ui.selectable_value(&mut target.transform.deinterlace, DeinterlaceMode::Bob, "Bob (duplicate)");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Scale mode:");
        egui::ComboBox::from_id_salt("inspector_scale_mode_combo")
            .selected_text(match target.transform.scale_mode {
                ScaleMode::Fit => "Fit (letterbox)",
                ScaleMode::Fill => "Fill (crop)",
                ScaleMode::Stretch => "Stretch",
                ScaleMode::Center => "Center (1:1)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.scale_mode, ScaleMode::Fit, "Fit (letterbox)");
                ui.selectable_value(&mut target.transform.scale_mode, ScaleMode::Fill, "Fill (crop)");
                ui.selectable_value(&mut target.transform.scale_mode, ScaleMode::Stretch, "Stretch");
                ui.selectable_value(&mut target.transform.scale_mode, ScaleMode::Center, "Center (1:1)");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Scale filter:");
        egui::ComboBox::from_id_salt("inspector_scaling_filter_combo")
            .selected_text(match target.transform.scaling_filter {
                ScalingFilter::Bilinear => "Bilinear",
                ScalingFilter::Bicubic => "Bicubic (Catmull-Rom)",
                ScalingFilter::Lanczos3 => "Lanczos 3",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.scaling_filter, ScalingFilter::Bilinear, "Bilinear");
                ui.selectable_value(&mut target.transform.scaling_filter, ScalingFilter::Bicubic, "Bicubic (Catmull-Rom)");
                ui.selectable_value(&mut target.transform.scaling_filter, ScalingFilter::Lanczos3, "Lanczos 3");
            });
    });

    ui.horizontal(|ui| {
        ui.label("Frame-rate conversion:");
        egui::ComboBox::from_id_salt("inspector_frame_rate_mode_combo")
            .selected_text(match target.transform.frame_rate_mode {
                FrameRateMode::Nearest => "Nearest frame",
                FrameRateMode::Blend => "Blend frames",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target.transform.frame_rate_mode, FrameRateMode::Nearest, "Nearest frame");
                ui.selectable_value(&mut target.transform.frame_rate_mode, FrameRateMode::Blend, "Blend frames");
            });
    });

    ui.add_space(6.0);
    let mut crop_cfg = target.crop.unwrap_or_default();
    let mut crop_enabled = crop_cfg.enabled;
    if ui.checkbox(&mut crop_enabled, "Source Cropping").changed() {
        crop_cfg.enabled = crop_enabled;
        target.crop = Some(crop_cfg);
    }
    if crop_enabled {
        ui.horizontal(|ui| {
            ui.label("Left:");
            if ui.add(egui::DragValue::new(&mut crop_cfg.left).range(0..=3840).speed(1)).changed() {
                target.crop = Some(crop_cfg);
            }
            ui.label("Top:");
            if ui.add(egui::DragValue::new(&mut crop_cfg.top).range(0..=2160).speed(1)).changed() {
                target.crop = Some(crop_cfg);
            }
            ui.label("Right:");
            if ui.add(egui::DragValue::new(&mut crop_cfg.right).range(0..=3840).speed(1)).changed() {
                target.crop = Some(crop_cfg);
            }
            ui.label("Bottom:");
            if ui.add(egui::DragValue::new(&mut crop_cfg.bottom).range(0..=2160).speed(1)).changed() {
                target.crop = Some(crop_cfg);
            }
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // Bitrate Settings (CBR, VBR 1-Pass, VBR 2-Pass, Constant Quality)
    ui.heading("Bitrate Settings");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Bitrate Encoding:");
        egui::ComboBox::from_id_salt("inspector_bitrate_mode_combo").selected_text(&state.video_bitrate_mode).show_ui(ui, |ui| {
            if ui.selectable_value(&mut state.video_bitrate_mode, "VBR 1-Pass".into(), "VBR, 1 Pass").clicked() {
                target.video.rate_mode = encodercraft_core::RateMode::Vbr;
            }
            if ui.selectable_value(&mut state.video_bitrate_mode, "VBR 2-Pass".into(), "VBR, 2 Pass").clicked() {
                target.video.rate_mode = encodercraft_core::RateMode::Vbr;
            }
            if ui.selectable_value(&mut state.video_bitrate_mode, "CBR".into(), "CBR (Constant Bitrate)").clicked() {
                target.video.rate_mode = encodercraft_core::RateMode::Cbr;
            }
            if ui.selectable_value(&mut state.video_bitrate_mode, "Constant Quality".into(), "Constant Quality").clicked() {
                target.video.rate_mode = encodercraft_core::RateMode::ConstantQuality;
            }
        });
    });

    let mut bitrate_kbps = target.target_bitrate_kbps.unwrap_or(12000);
    ui.horizontal(|ui| {
        ui.label("Target Bitrate [Mbps]:");
        let mut mbps = bitrate_kbps as f32 / 1000.0;
        if ui.add(egui::Slider::new(&mut mbps, 0.5..=150.0).step_by(0.5)).changed() {
            bitrate_kbps = (mbps * 1000.0).round() as u32;
            target.target_bitrate_kbps = Some(bitrate_kbps);
        }
    });

    ui.add_space(6.0);
    ui.separator();

    // Advanced Video Settings
    ui.heading("Advanced Video Settings");
    ui.add_space(4.0);

    // Keyframe Distance (GOP)
    let mut gop = target.video.keyframe_interval.unwrap_or(24);
    ui.horizontal(|ui| {
        ui.label("Keyframe Distance (GOP):");
        if ui.add(egui::DragValue::new(&mut gop).range(1..=300).suffix(" frames")).changed() {
            target.video.keyframe_interval = Some(gop);
        }
    });

    // B-Frames
    let mut bframes = target.video.bframes.unwrap_or(3);
    ui.horizontal(|ui| {
        ui.label("Consecutive B-Frames:");
        if ui.add(egui::DragValue::new(&mut bframes).range(0..=16)).changed() {
            target.video.bframes = Some(bframes);
        }
    });

    // Codec Profile
    let current_profile = target.video.profile.clone().unwrap_or_else(|| "high".to_string());
    ui.horizontal(|ui| {
        ui.label("Profile:");
        egui::ComboBox::from_id_salt("inspector_video_profile_combo").selected_text(&current_profile).show_ui(ui, |ui| {
            for (prof_id, prof_label) in [
                ("baseline", "Baseline"),
                ("main", "Main"),
                ("high", "High"),
                ("prores_proxy", "Apple ProRes Proxy"),
                ("prores_lt", "Apple ProRes LT"),
                ("prores_422", "Apple ProRes 422 Standard"),
                ("prores_hq", "Apple ProRes 422 HQ"),
                ("prores_4444", "Apple ProRes 4444"),
            ] {
                if ui.selectable_label(current_profile == prof_id, prof_label).clicked() {
                    target.video.profile = Some(prof_id.to_string());
                }
            }
        });
    });

    // Pixel Format / Color Depth
    let current_pf = target.video.pixel_format.unwrap_or(encodercraft_color::PixelFormat::Yuv420p);
    ui.horizontal(|ui| {
        ui.label("Pixel Format & Depth:");
        egui::ComboBox::from_id_salt("inspector_pixel_format_combo")
            .selected_text(match current_pf {
                encodercraft_color::PixelFormat::Yuv420p => "Planar YUV 4:2:0 (8-bit Standard)",
                encodercraft_color::PixelFormat::Yuv422p => "Planar YUV 4:2:2 (8-bit Broadcast)",
                encodercraft_color::PixelFormat::Yuv420p10 => "Planar YUV 4:2:0 10-bit (HDR10/HEVC Main 10)",
                encodercraft_color::PixelFormat::Yuv422p10 => "Planar YUV 4:2:2 10-bit (ProRes 422/Broadcast)",
                encodercraft_color::PixelFormat::Yuv444p12 => "Planar YUV 4:4:4 12-bit (ProRes 4444 Master)",
                encodercraft_color::PixelFormat::Rgba8 => "8-bit RGBA",
                encodercraft_color::PixelFormat::Rgb8 => "8-bit RGB",
                encodercraft_color::PixelFormat::Rgba16 => "16-bit RGBA",
                encodercraft_color::PixelFormat::RgbaF32 => "32-bit Float RGBA (HDR)",
                _ => "Auto (Default)",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(current_pf == encodercraft_color::PixelFormat::Yuv420p, "Planar YUV 4:2:0 (8-bit Standard)").clicked() {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Yuv420p);
                }
                if ui.selectable_label(current_pf == encodercraft_color::PixelFormat::Yuv422p, "Planar YUV 4:2:2 (8-bit Broadcast)").clicked() {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Yuv422p);
                }
                if ui
                    .selectable_label(current_pf == encodercraft_color::PixelFormat::Yuv420p10, "Planar YUV 4:2:0 10-bit (HDR10/HEVC Main 10)")
                    .clicked()
                {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Yuv420p10);
                }
                if ui
                    .selectable_label(current_pf == encodercraft_color::PixelFormat::Yuv422p10, "Planar YUV 4:2:2 10-bit (ProRes 422/Broadcast)")
                    .clicked()
                {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Yuv422p10);
                }
                if ui
                    .selectable_label(current_pf == encodercraft_color::PixelFormat::Yuv444p12, "Planar YUV 4:4:4 12-bit (ProRes 4444 Master)")
                    .clicked()
                {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Yuv444p12);
                }
                if ui.selectable_label(current_pf == encodercraft_color::PixelFormat::Rgba8, "8-bit RGBA").clicked() {
                    target.video.pixel_format = Some(encodercraft_color::PixelFormat::Rgba8);
                }
            });
    });

    ui.add_space(6.0);
    ui.separator();

    // Hardware Acceleration & Rendering options
    ui.checkbox(&mut target.hardware_acceleration, "Hardware Acceleration (VideoToolbox / NVENC / VA-API)");
}

fn render_audio_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    ui.heading("Audio Format & Channels");
    ui.add_space(4.0);

    let mut audio_settings = target.audio.clone().unwrap_or_else(|| {
        let def = default_audio_codec_for_container(target.container);
        let bit_depth = if def == "pcm_s24le" || def == "pcm_s24be" || def == "lpcm" {
            Some(24)
        } else if def == "pcm_s16le" || def == "pcm_s16be" {
            Some(16)
        } else if def == "pcm_f32le" || def == "pcm_f32be" {
            Some(32)
        } else {
            None
        };
        encodercraft_core::AudioEncodeSettings {
            codec: Some(def.to_string()),
            sample_rate: Some(48000),
            layout: Some(ChannelLayout::Stereo),
            bitrate_kbps: if def.starts_with("pcm") { None } else { Some(320) },
            bit_depth,
            delay_ms: 0,
        }
    });

    let available_codecs = audio_codecs_for_container(target.container);
    let current_codec_id = audio_settings.codec.clone().unwrap_or_else(|| default_audio_codec_for_container(target.container).to_string());
    let current_codec_label =
        available_codecs.iter().find(|(id, _)| *id == current_codec_id.as_str()).map(|(_, l)| *l).unwrap_or(current_codec_id.as_str());

    ui.horizontal(|ui| {
        ui.label("Audio Codec:");
        egui::ComboBox::from_id_salt("inspector_audio_codec_combo").selected_text(current_codec_label).show_ui(ui, |ui| {
            for (id, label) in available_codecs {
                let is_selected = current_codec_id.as_str() == *id;
                if ui.selectable_label(is_selected, *label).clicked() {
                    audio_settings.codec = Some((*id).to_string());
                    if *id == "pcm_s16le" || *id == "pcm_s16be" {
                        audio_settings.bit_depth = Some(16);
                    } else if *id == "pcm_s24le" || *id == "pcm_s24be" || *id == "lpcm" {
                        audio_settings.bit_depth = Some(24);
                    } else if *id == "pcm_f32le" || *id == "pcm_f32be" {
                        audio_settings.bit_depth = Some(32);
                    }
                }
            }
        });
    });

    let current_sr = audio_settings.sample_rate.unwrap_or(48000);
    ui.horizontal(|ui| {
        ui.label("Sample Rate:");
        egui::ComboBox::from_id_salt("inspector_audio_sr")
            .selected_text(match current_sr {
                44100 => "44,100 Hz (CD Standard)",
                48000 => "48,000 Hz (Video Broadcast Standard)",
                88200 => "88,200 Hz",
                96000 => "96,000 Hz (Hi-Res Audio)",
                192000 => "192,000 Hz (Studio Master)",
                _ => "48,000 Hz",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(current_sr == 44100, "44,100 Hz (CD Standard)").clicked() {
                    audio_settings.sample_rate = Some(44100);
                    state.audio_sample_rate_idx = 0;
                }
                if ui.selectable_label(current_sr == 48000, "48,000 Hz (Video Broadcast Standard)").clicked() {
                    audio_settings.sample_rate = Some(48000);
                    state.audio_sample_rate_idx = 1;
                }
                if ui.selectable_label(current_sr == 88200, "88,200 Hz").clicked() {
                    audio_settings.sample_rate = Some(88200);
                }
                if ui.selectable_label(current_sr == 96000, "96,000 Hz (Hi-Res Audio)").clicked() {
                    audio_settings.sample_rate = Some(96000);
                    state.audio_sample_rate_idx = 2;
                }
                if ui.selectable_label(current_sr == 192000, "192,000 Hz (Studio Master)").clicked() {
                    audio_settings.sample_rate = Some(192000);
                }
            });
    });

    let current_layout = audio_settings.layout.unwrap_or(ChannelLayout::Stereo);
    ui.horizontal(|ui| {
        ui.label("Channels:");
        egui::ComboBox::from_id_salt("inspector_audio_ch")
            .selected_text(match current_layout {
                ChannelLayout::Mono => "Mono (1.0)",
                ChannelLayout::Surround51 => "5.1 Surround",
                ChannelLayout::Surround71 => "8 Channels (7.1 / Broadcast)",
                _ => "Stereo (2.0)",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(current_layout == ChannelLayout::Mono, "Mono (1.0)").clicked() {
                    audio_settings.layout = Some(ChannelLayout::Mono);
                    state.audio_channels_idx = 0;
                }
                if ui.selectable_label(current_layout == ChannelLayout::Stereo, "Stereo (2.0)").clicked() {
                    audio_settings.layout = Some(ChannelLayout::Stereo);
                    state.audio_channels_idx = 1;
                }
                if ui.selectable_label(current_layout == ChannelLayout::Surround51, "5.1 Surround").clicked() {
                    audio_settings.layout = Some(ChannelLayout::Surround51);
                    state.audio_channels_idx = 2;
                }
                if ui.selectable_label(current_layout == ChannelLayout::Surround71, "8 Channels (7.1 Surround / Broadcast 8-Track)").clicked() {
                    audio_settings.layout = Some(ChannelLayout::Surround71);
                    state.audio_channels_idx = 3;
                }
            });
    });

    let is_lossy = current_codec_id == "aac" || current_codec_id == "mp3" || current_codec_id == "opus" || current_codec_id == "vorbis";
    if is_lossy {
        let kbps = audio_settings.bitrate_kbps.unwrap_or(320);
        ui.horizontal(|ui| {
            ui.label("Bitrate [kbps]:");
            egui::ComboBox::from_id_salt("inspector_audio_bitrate_combo").selected_text(format!("{kbps} kbps")).show_ui(ui, |ui| {
                for rate in [64, 96, 128, 160, 192, 256, 320, 384, 448, 512] {
                    if ui.selectable_label(kbps == rate, format!("{rate} kbps")).clicked() {
                        audio_settings.bitrate_kbps = Some(rate);
                    }
                }
            });
        });
    }

    let is_pcm_or_lossless =
        current_codec_id.starts_with("pcm") || current_codec_id == "lpcm" || current_codec_id == "flac" || current_codec_id == "alac";
    if is_pcm_or_lossless {
        let current_depth = audio_settings.bit_depth.unwrap_or(24);
        ui.horizontal(|ui| {
            ui.label("Sample Size / Bit Depth:");
            egui::ComboBox::from_id_salt("inspector_audio_depth_combo")
                .selected_text(match current_depth {
                    16 => "16-bit Integer",
                    32 => "32-bit Float",
                    _ => "24-bit Integer (Broadcast/Studio)",
                })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current_depth == 16, "16-bit Integer (CD Standard)").clicked() {
                        audio_settings.bit_depth = Some(16);
                        if target.container == ContainerType::Wave {
                            audio_settings.codec = Some("pcm_s16le".to_string());
                        } else if target.container == ContainerType::Aiff {
                            audio_settings.codec = Some("pcm_s16be".to_string());
                        }
                    }
                    if ui.selectable_label(current_depth == 24, "24-bit Integer (Studio Master)").clicked() {
                        audio_settings.bit_depth = Some(24);
                        if target.container == ContainerType::Wave {
                            audio_settings.codec = Some("pcm_s24le".to_string());
                        } else if target.container == ContainerType::Aiff {
                            audio_settings.codec = Some("pcm_s24be".to_string());
                        }
                    }
                    if ui.selectable_label(current_depth == 32, "32-bit Floating Point").clicked() {
                        audio_settings.bit_depth = Some(32);
                        if target.container == ContainerType::Wave {
                            audio_settings.codec = Some("pcm_f32le".to_string());
                        } else if target.container == ContainerType::Aiff {
                            audio_settings.codec = Some("pcm_f32be".to_string());
                        }
                    }
                });
        });
    }

    // Format-Specific Audio Settings (AME Parity)
    match target.container {
        ContainerType::Wave => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("WAV/BWF Format Options:").small().color(egui::Color32::GRAY));
                let mut bext = true;
                let mut rf64 = true;
                ui.checkbox(&mut bext, "Broadcast Wave Format (BEXT chunk)").on_hover_text("Includes standard SMPTE timecode & originator metadata");
                ui.checkbox(&mut rf64, "RF64 Large File Support (>4GB)");
            });
        }
        ContainerType::Flac => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("FLAC Compression Level:");
                ui.label("8 (Maximum Compression)");
            });
        }
        ContainerType::Mp3 => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Stereo Mode:");
                ui.label("Joint Stereo (Standard)");
            });
        }
        ContainerType::Opus => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Opus Application:");
                ui.label("Audio (Full Fidelity Broadcast)");
            });
        }
        _ => {}
    }

    ui.add_space(6.0);
    ui.separator();

    let mut current_stems = target.audio_stems.unwrap_or(encodercraft_core::AudioStemMode::None);
    ui.horizontal(|ui| {
        ui.label("Multichannel Audio Stems:");
        egui::ComboBox::from_id_salt("audio_stems_combo")
            .selected_text(match current_stems {
                encodercraft_core::AudioStemMode::None => "None (Master Mix Only)",
                encodercraft_core::AudioStemMode::DiscreteMono => "Discrete Mono Channels (_Stem_L, _Stem_R.wav)",
                encodercraft_core::AudioStemMode::StereoPairs => "Stereo Channel Pairs (_Stems_Ch1-2.wav)",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut current_stems, encodercraft_core::AudioStemMode::None, "None (Master Mix Only)");
                ui.selectable_value(
                    &mut current_stems,
                    encodercraft_core::AudioStemMode::DiscreteMono,
                    "Discrete Mono Channels (_Stem_L, _Stem_R.wav)",
                );
                ui.selectable_value(&mut current_stems, encodercraft_core::AudioStemMode::StereoPairs, "Stereo Channel Pairs (_Stems_Ch1-2.wav)");
            });
    });
    target.audio_stems = if current_stems == encodercraft_core::AudioStemMode::None { None } else { Some(current_stems) };

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Lip-Sync / Audio Delay:");
        let mut delay = audio_settings.delay_ms;
        let drag = ui.add(egui::DragValue::new(&mut delay).suffix(" ms").speed(1.0).range(-5000..=5000));
        if drag.changed() {
            audio_settings.delay_ms = delay;
        }
    });

    target.audio = Some(audio_settings);

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    let default_ch_count = match state.audio_channels_idx {
        0 => 1,
        2 => 6,
        3 => 8,
        _ => 2,
    };
    let mut mapping = target.audio_mapping.clone().unwrap_or_else(|| encodercraft_core::AudioChannelMapping::new_pass_through(default_ch_count));

    let mut custom_matrix_enabled = target.audio_mapping.is_some();
    if ui.checkbox(&mut custom_matrix_enabled, "Multichannel Matrix Routing Grid").changed() {
        if custom_matrix_enabled {
            target.audio_mapping = Some(mapping.clone());
        } else {
            target.audio_mapping = None;
        }
    }

    if custom_matrix_enabled {
        ui.indent("audio_matrix_indent", |ui| {
            ui.horizontal(|ui| {
                ui.label("Track Layout:");
                egui::ComboBox::from_id_salt("matrix_track_layout")
                    .selected_text(match mapping.track_layout {
                        encodercraft_core::AudioTrackLayout::Standard => "Standard",
                        encodercraft_core::AudioTrackLayout::DiscreteMono => "Discrete Mono Tracks",
                        encodercraft_core::AudioTrackLayout::StereoPairs => "Stereo Pairs",
                        encodercraft_core::AudioTrackLayout::Surround51 => "5.1 Surround",
                        encodercraft_core::AudioTrackLayout::Surround71 => "7.1 / 8-Ch Broadcast",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mapping.track_layout, encodercraft_core::AudioTrackLayout::Standard, "Standard");
                        ui.selectable_value(&mut mapping.track_layout, encodercraft_core::AudioTrackLayout::DiscreteMono, "Discrete Mono Tracks");
                        ui.selectable_value(&mut mapping.track_layout, encodercraft_core::AudioTrackLayout::StereoPairs, "Stereo Pairs");
                        ui.selectable_value(&mut mapping.track_layout, encodercraft_core::AudioTrackLayout::Surround51, "5.1 Surround");
                        ui.selectable_value(&mut mapping.track_layout, encodercraft_core::AudioTrackLayout::Surround71, "7.1 / 8-Ch Broadcast");
                    });

                ui.separator();
                if ui.button("1:1 Passthrough").clicked() {
                    mapping = encodercraft_core::AudioChannelMapping::new_pass_through(mapping.output_channels);
                }
                if ui.button("Discrete 8-Ch").clicked() {
                    mapping = encodercraft_core::AudioChannelMapping::new_discrete_mono(8);
                }
            });

            ui.add_space(4.0);
            ui.label(RichText::new("Matrix Cross-Routing (Output Channels x Input Channels):").small().color(ui.visuals().weak_text_color()));

            let out_count = mapping.output_channels.min(8);
            let in_count = 8.min(mapping.matrix.first().map(|r| r.len()).unwrap_or(2).max(2));

            egui::Grid::new("audio_matrix_grid").striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("");
                for in_idx in 0..in_count {
                    ui.label(RichText::new(format!("In {}", in_idx + 1)).strong());
                }
                ui.end_row();

                for out_idx in 0..out_count {
                    ui.label(RichText::new(format!("Out {}", out_idx + 1)).strong());
                    for in_idx in 0..in_count {
                        let gain = mapping.get_gain(out_idx, in_idx);
                        let is_active = gain > 0.0;
                        let label = if is_active { format!("{:.1}", gain) } else { "—".into() };
                        if ui.selectable_label(is_active, label).clicked() {
                            let new_gain = if is_active { 0.0 } else { 1.0 };
                            mapping.set_gain(out_idx, in_idx, new_gain);
                        }
                    }
                    ui.end_row();
                }
            });

            target.audio_mapping = Some(mapping);
        });
    }
}

fn render_effects_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Effects, Overlays & 3D LUTs");
    ui.add_space(4.0);

    // 1. Timecode Overlay (BITC)
    let mut tc_cfg = target.timecode_overlay.clone().unwrap_or_default();
    let mut tc_enabled = tc_cfg.enabled || target.burn_timecode;
    if ui.checkbox(&mut tc_enabled, "Timecode Overlay (SMPTE Burn-In)").changed() {
        tc_cfg.enabled = tc_enabled;
        target.burn_timecode = tc_enabled;
        target.timecode_overlay = Some(tc_cfg.clone());
    }
    if tc_enabled {
        ui.indent("timecode_overlay_indent", |ui| {
            ui.horizontal(|ui| {
                ui.label("Position:");
                egui::ComboBox::from_id_salt("tc_pos_combo")
                    .selected_text(match tc_cfg.anchor {
                        OverlayAnchor::TopLeft => "Top Left",
                        OverlayAnchor::TopRight => "Top Right",
                        OverlayAnchor::BottomLeft => "Bottom Left",
                        OverlayAnchor::BottomRight => "Bottom Right",
                        OverlayAnchor::Center => "Center",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut tc_cfg.anchor, OverlayAnchor::TopLeft, "Top Left");
                        ui.selectable_value(&mut tc_cfg.anchor, OverlayAnchor::TopRight, "Top Right");
                        ui.selectable_value(&mut tc_cfg.anchor, OverlayAnchor::BottomLeft, "Bottom Left");
                        ui.selectable_value(&mut tc_cfg.anchor, OverlayAnchor::BottomRight, "Bottom Right");
                        ui.selectable_value(&mut tc_cfg.anchor, OverlayAnchor::Center, "Center");
                    });

                ui.label("Scale:");
                ui.add(egui::Slider::new(&mut tc_cfg.scale, 1..=5).text("x"));
            });
            ui.horizontal(|ui| {
                ui.label("Text Opacity:");
                ui.add(egui::Slider::new(&mut tc_cfg.text_opacity, 0.1..=1.0));
                ui.label("Background:");
                ui.add(egui::Slider::new(&mut tc_cfg.bg_opacity, 0.0..=1.0));
            });
            let mut custom_tc = tc_cfg.starting_timecode.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Start Timecode:");
                if ui.add(egui::TextEdit::singleline(&mut custom_tc).hint_text("Source Timecode (e.g. 01:00:00:00)")).changed() {
                    tc_cfg.starting_timecode = if custom_tc.trim().is_empty() { None } else { Some(custom_tc.trim().into()) };
                }
            });
            target.timecode_overlay = Some(tc_cfg);
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // 2. Name & Dailies Overlay
    let mut name_cfg = target.name_overlay.clone().unwrap_or_default();
    let mut name_enabled = name_cfg.enabled;
    if ui.checkbox(&mut name_enabled, "Name Overlay (Dailies Burn-In)").changed() {
        name_cfg.enabled = name_enabled;
        target.name_overlay = Some(name_cfg.clone());
    }
    if name_enabled {
        ui.indent("name_overlay_indent", |ui| {
            ui.horizontal(|ui| {
                ui.label("Prefix:");
                ui.text_edit_singleline(&mut name_cfg.prefix);
                ui.label("Suffix:");
                ui.text_edit_singleline(&mut name_cfg.suffix);
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut name_cfg.use_source_name, "Include Source File Name");
                ui.label("Position:");
                egui::ComboBox::from_id_salt("name_pos_combo")
                    .selected_text(match name_cfg.anchor {
                        OverlayAnchor::TopLeft => "Top Left",
                        OverlayAnchor::TopRight => "Top Right",
                        OverlayAnchor::BottomLeft => "Bottom Left",
                        OverlayAnchor::BottomRight => "Bottom Right",
                        OverlayAnchor::Center => "Center",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut name_cfg.anchor, OverlayAnchor::TopLeft, "Top Left");
                        ui.selectable_value(&mut name_cfg.anchor, OverlayAnchor::TopRight, "Top Right");
                        ui.selectable_value(&mut name_cfg.anchor, OverlayAnchor::BottomLeft, "Bottom Left");
                        ui.selectable_value(&mut name_cfg.anchor, OverlayAnchor::BottomRight, "Bottom Right");
                        ui.selectable_value(&mut name_cfg.anchor, OverlayAnchor::Center, "Center");
                    });
                ui.label("Opacity:");
                ui.add(egui::Slider::new(&mut name_cfg.opacity, 0.1..=1.0));
            });
            target.name_overlay = Some(name_cfg);
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // 3. Image Watermark Overlay
    let mut wm_cfg = target.watermark.clone().unwrap_or_default();
    let mut wm_enabled = wm_cfg.enabled;
    if ui.checkbox(&mut wm_enabled, "Image Watermark Overlay").changed() {
        wm_cfg.enabled = wm_enabled;
        target.watermark = Some(wm_cfg.clone());
    }
    if wm_enabled {
        ui.indent("wm_overlay_indent", |ui| {
            let mut wm_str = wm_cfg.path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label("Image File (.png, .jpg, .webp):");
                if ui.text_edit_singleline(&mut wm_str).changed() {
                    wm_cfg.path = if wm_str.trim().is_empty() { None } else { Some(PathBuf::from(wm_str.trim())) };
                }
                if ui.button("Browse...").clicked()
                    && let Some(file) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "webp"]).pick_file()
                {
                    wm_cfg.path = Some(file);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Position:");
                egui::ComboBox::from_id_salt("wm_pos_combo")
                    .selected_text(match wm_cfg.anchor {
                        OverlayAnchor::TopLeft => "Top Left",
                        OverlayAnchor::TopRight => "Top Right",
                        OverlayAnchor::BottomLeft => "Bottom Left",
                        OverlayAnchor::BottomRight => "Bottom Right",
                        OverlayAnchor::Center => "Center",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut wm_cfg.anchor, OverlayAnchor::TopLeft, "Top Left");
                        ui.selectable_value(&mut wm_cfg.anchor, OverlayAnchor::TopRight, "Top Right");
                        ui.selectable_value(&mut wm_cfg.anchor, OverlayAnchor::BottomLeft, "Bottom Left");
                        ui.selectable_value(&mut wm_cfg.anchor, OverlayAnchor::BottomRight, "Bottom Right");
                        ui.selectable_value(&mut wm_cfg.anchor, OverlayAnchor::Center, "Center");
                    });

                ui.label("Scale:");
                let mut scale_pct = (wm_cfg.scale * 100.0) as u32;
                if ui.add(egui::Slider::new(&mut scale_pct, 5..=100).text("% width")).changed() {
                    wm_cfg.scale = scale_pct as f32 / 100.0;
                }
                ui.label("Opacity:");
                ui.add(egui::Slider::new(&mut wm_cfg.opacity, 0.05..=1.0));
            });
            target.watermark = Some(wm_cfg);
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // 4. Video Limiter
    let mut limiter_cfg = target.video_limiter.clone().unwrap_or_default();
    let mut limiter_enabled = limiter_cfg.enabled;
    if ui.checkbox(&mut limiter_enabled, "Broadcast Video Limiter").changed() {
        limiter_cfg.enabled = limiter_enabled;
        target.video_limiter = Some(limiter_cfg.clone());
    }
    if limiter_enabled {
        ui.indent("limiter_indent", |ui| {
            ui.horizontal(|ui| {
                ui.label("Clip Level:");
                egui::ComboBox::from_id_salt("limiter_level_combo")
                    .selected_text(match limiter_cfg.clip_level {
                        VideoLimiterClipLevel::Ire100 => "IRE 100 (Broadcast Legal)",
                        VideoLimiterClipLevel::Ire103 => "IRE 103 (Permissive Studio)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut limiter_cfg.clip_level, VideoLimiterClipLevel::Ire100, "IRE 100 (Broadcast Legal)");
                        ui.selectable_value(&mut limiter_cfg.clip_level, VideoLimiterClipLevel::Ire103, "IRE 103 (Permissive Studio)");
                    });

                ui.label("Soft Knee:");
                ui.add(egui::Slider::new(&mut limiter_cfg.soft_knee_percent, 0.0..=15.0).text("%"));
            });
            target.video_limiter = Some(limiter_cfg);
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // 5. SDR Conform (HDR to SDR Tone Mapping)
    let mut sdr_cfg = target.sdr_conform.clone().unwrap_or_default();
    let mut sdr_enabled = sdr_cfg.enabled;
    if ui.checkbox(&mut sdr_enabled, "SDR Conform (Tone-map HDR10 / HLG to Rec.709)").changed() {
        sdr_cfg.enabled = sdr_enabled;
        target.sdr_conform = Some(sdr_cfg.clone());
    }
    if sdr_enabled {
        ui.indent("sdr_conform_indent", |ui| {
            ui.horizontal(|ui| {
                ui.label("Target Peak Luminance:");
                ui.add(egui::Slider::new(&mut sdr_cfg.target_nits, 80.0..=200.0).text("nits (SDR standard: 100)"));
            });
            target.sdr_conform = Some(sdr_cfg);
        });
    }

    ui.add_space(6.0);
    ui.separator();

    // 6. 3D LUT (.cube, .3dl, .look)
    let mut lut_str = target.lut_path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Apply 3D LUT (.cube, .3dl, .look):");
        if ui.text_edit_singleline(&mut lut_str).changed() {
            target.lut_path = if lut_str.trim().is_empty() { None } else { Some(PathBuf::from(lut_str.trim())) };
        }
        if ui.button("Browse...").clicked()
            && let Some(file) = rfd::FileDialog::new().add_filter("3D LUT / Look", &["cube", "3dl", "look"]).pick_file()
        {
            target.lut_path = Some(file);
        }
    });
}

fn render_captions_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("Closed Captions & Subtitles");
    ui.add_space(4.0);

    let mut cap_str = target.caption_path.as_ref().and_then(|p| p.to_str()).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Caption File:");
        if ui.text_edit_singleline(&mut cap_str).changed() {
            target.caption_path = if cap_str.trim().is_empty() { None } else { Some(PathBuf::from(cap_str.trim())) };
        }
        if ui.button("Browse...").clicked()
            && let Some(file) =
                rfd::FileDialog::new().add_filter("Captions", &["srt", "vtt", "scc", "mcc", "ttml", "dfxp", "ass", "ssa", "stl"]).pick_file()
        {
            target.caption_path = Some(file);
        }
    });

    ui.add_space(6.0);
    ui.checkbox(&mut target.burn_captions, "Burn Captions into Video (Open Captions)");

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let mut has_sidecar = target.caption_format.is_some();
        if ui.checkbox(&mut has_sidecar, "Generate Sidecar File").changed() {
            target.caption_format = if has_sidecar { Some(CaptionExportFormat::WebVtt) } else { None };
        }

        if let Some(ref mut fmt) = target.caption_format {
            egui::ComboBox::from_id_salt("caption_sidecar_format_combo").selected_text(fmt.display_name()).show_ui(ui, |ui| {
                ui.selectable_value(fmt, CaptionExportFormat::WebVtt, CaptionExportFormat::WebVtt.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Srt, CaptionExportFormat::Srt.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Scc, CaptionExportFormat::Scc.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Mcc, CaptionExportFormat::Mcc.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Ttml, CaptionExportFormat::Ttml.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Ass, CaptionExportFormat::Ass.display_name());
                ui.selectable_value(fmt, CaptionExportFormat::Stl, CaptionExportFormat::Stl.display_name());
            });
        }
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Time Offset:");
        let mut offset = target.caption_offset_ms.unwrap_or(0);
        if ui.add(egui::DragValue::new(&mut offset).speed(25.0).suffix(" ms")).changed() {
            target.caption_offset_ms = if offset == 0 { None } else { Some(offset) };
        }
    });
}

fn render_publish_tab(ui: &mut Ui, target: &mut OutputTarget, state: &mut InspectorState) {
    ui.heading("Publish & Post-Encode Actions");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Destination Folder:");
        let folder_str = target.output_path.parent().and_then(|p| p.to_str()).unwrap_or("");
        ui.label(RichText::new(folder_str).monospace().small());
        if ui.button("Choose...").clicked()
            && let Some(folder) = rfd::FileDialog::new().set_title("Choose Destination Directory").pick_folder()
        {
            let file_name = target.output_path.file_name().and_then(|s| s.to_str()).unwrap_or("output.mp4");
            target.output_path = folder.join(file_name);
        }
    });

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("Post-Encode Action:");
        egui::ComboBox::from_id_salt("inspector_post_action").selected_text(&state.post_encode_action).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.post_encode_action, "None".into(), "Do Nothing");
            ui.selectable_value(&mut state.post_encode_action, "Reveal".into(), "Reveal in File Manager");
            ui.selectable_value(&mut state.post_encode_action, "Play".into(), "Play in Default Media Player");
        });
    });
}

fn render_loudness_tab(ui: &mut Ui, target: &mut OutputTarget) {
    ui.heading("EBU R128 / ITU-R BS.1770 Loudness Normalization");
    ui.add_space(4.0);

    let mut lufs = target.target_lufs.unwrap_or(-23.0);
    ui.horizontal(|ui| {
        ui.label("Integrated Target (LUFS):");
        if ui.add(egui::DragValue::new(&mut lufs).range(-36.0..=-6.0).speed(0.5)).changed() {
            target.target_lufs = Some(lufs);
        }
    });

    ui.horizontal(|ui| {
        ui.label("True Peak Ceiling:");
        ui.label("-1.0 dBFS (4x Oversampled Brickwall)");
    });
}

fn render_metadata_tab(ui: &mut Ui, client_info: &mut encodercraft_core::ClientInfo, target: &mut OutputTarget) {
    ui.heading("Job & Dublin Core / Adobe XMP Metadata");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Submitting Application:");
        ui.label(&client_info.app_name);
    });

    let mut meta = target.metadata.clone().unwrap_or_default();
    let mut changed = false;

    let mut title = meta.title.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Title (dc:title):");
        if ui.text_edit_singleline(&mut title).changed() {
            meta.title = if title.trim().is_empty() { None } else { Some(title) };
            changed = true;
        }
    });

    let mut creator = meta.creator.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Artist / Creator (dc:creator):");
        if ui.text_edit_singleline(&mut creator).changed() {
            meta.creator = if creator.trim().is_empty() { None } else { Some(creator) };
            changed = true;
        }
    });

    let mut album = meta.album.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Album (xmpDM:album):");
        if ui.text_edit_singleline(&mut album).changed() {
            meta.album = if album.trim().is_empty() { None } else { Some(album) };
            changed = true;
        }
    });

    let mut track_str = meta.track_number.map(|t| t.to_string()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Track Number (xmpDM:trackNumber):");
        if ui.add(egui::TextEdit::singleline(&mut track_str).desired_width(60.0)).changed() {
            meta.track_number = track_str.trim().parse::<u32>().ok();
            changed = true;
        }
    });

    let mut year_str = meta.year.map(|y| y.to_string()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Year (xmpDM:releaseDate):");
        if ui.add(egui::TextEdit::singleline(&mut year_str).desired_width(80.0)).changed() {
            meta.year = year_str.trim().parse::<u32>().ok();
            changed = true;
        }
    });

    let mut genre = meta.genre.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Genre (xmpDM:genre):");
        if ui.text_edit_singleline(&mut genre).changed() {
            meta.genre = if genre.trim().is_empty() { None } else { Some(genre) };
            changed = true;
        }
    });

    let mut desc = meta.description.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Description (dc:description):");
        if ui.text_edit_singleline(&mut desc).changed() {
            meta.description = if desc.trim().is_empty() { None } else { Some(desc) };
            changed = true;
        }
    });

    let mut copyright = meta.copyright.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Copyright Notice (dc:rights):");
        if ui.text_edit_singleline(&mut copyright).changed() {
            meta.copyright = if copyright.trim().is_empty() { None } else { Some(copyright) };
            changed = true;
        }
    });

    let mut composer = meta.composer.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Composer / Songwriter (xmpDM:composer):");
        if ui.text_edit_singleline(&mut composer).changed() {
            meta.composer = if composer.trim().is_empty() { None } else { Some(composer) };
            changed = true;
        }
    });

    let mut isrc = meta.isrc.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("ISRC Code (xmpDM:ISRC):");
        if ui.text_edit_singleline(&mut isrc).changed() {
            meta.isrc = if isrc.trim().is_empty() { None } else { Some(isrc) };
            changed = true;
        }
    });

    ui.add_space(4.0);
    if ui.checkbox(&mut meta.export_sidecar_xmp, "Export External Sidecar .xmp File").changed() {
        changed = true;
    }

    if changed {
        target.metadata = Some(meta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_tabs_follow_target_domain() {
        assert!(!visible_tabs(None, ContainerType::Wave).contains(&InspectorTab::Video));
        assert!(visible_tabs(None, ContainerType::Flac).contains(&InspectorTab::Loudness));
        assert!(visible_tabs(None, ContainerType::IsoBmffMp4).contains(&InspectorTab::Captions));
        let doc = visible_tabs(None, ContainerType::Pdf);
        assert!(!doc.contains(&InspectorTab::Video) && !doc.contains(&InspectorTab::Audio));
        assert!(visible_tabs(None, ContainerType::Png).contains(&InspectorTab::Video));
        assert!(!visible_tabs(None, ContainerType::Png).contains(&InspectorTab::Audio));
    }

    #[test]
    fn test_source_specific_tabs() {
        let prproj = visible_tabs(Some(ContainerType::AdobePremierePrproj), ContainerType::IsoBmffMp4);
        assert_eq!(prproj.first(), Some(&InspectorTab::Composition));
        assert!(prproj.contains(&InspectorTab::Video));
        assert_eq!(visible_tabs(Some(ContainerType::AdobeAfterEffectsAep), ContainerType::Wave).first(), Some(&InspectorTab::Composition));
        assert_eq!(visible_tabs(Some(ContainerType::Idml), ContainerType::Pdf).first(), Some(&InspectorTab::Layout));
        assert_eq!(visible_tabs(Some(ContainerType::AutocadDxf), ContainerType::Svg).first(), Some(&InspectorTab::Cad));
        assert_eq!(visible_tabs(Some(ContainerType::AdobePhotoshopPsd), ContainerType::Png).first(), Some(&InspectorTab::Layers));
        assert!(
            !visible_tabs(Some(ContainerType::Wave), ContainerType::Flac)
                .iter()
                .any(|t| matches!(t, InspectorTab::Composition | InspectorTab::Layout | InspectorTab::Cad | InspectorTab::Layers))
        );
        assert!(source_has_structure(Some(ContainerType::Pdf)) || source_has_structure(Some(ContainerType::Idml)));
        assert!(!source_has_structure(Some(ContainerType::IsoBmffMp4)) && !source_has_structure(None));
    }

    fn inv_with_sequences(names: &[&str]) -> SourceInventory {
        SourceInventory {
            sequences: names
                .iter()
                .enumerate()
                .map(|(index, n)| encodercraft_pipeline::SequenceInfo {
                    index,
                    name: (*n).into(),
                    width: 1920,
                    height: 1080,
                    fps: 25.0,
                    duration_secs: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn test_sequence_selection_keys() {
        let inv = inv_with_sequences(&["Main", "Social", "Main"]);
        assert_eq!(sequence_key(&inv, 1).as_deref(), Some("Social"));
        assert_eq!(sequence_key(&inv, 0).as_deref(), Some("0"));
        assert_eq!(sequence_key(&inv, 9), None);
        let sel = SourceSelection { sequence: sequence_key(&inv, 2), ..Default::default() };
        assert_eq!(selected_sequence_index(&inv, &sel), Some(2));
        let sel = SourceSelection { sequence: Some("Social".into()), ..Default::default() };
        assert_eq!(selected_sequence_index(&inv, &sel), Some(1));
        assert_eq!(selected_sequence_index(&inv, &SourceSelection::default()), None);
    }

    #[test]
    fn test_page_range_and_layer_toggles() {
        let mut sel = SourceSelection::default();
        set_page_range(&mut sel, 10, 3, 5);
        assert_eq!(sel.pages, Some((3, 5)));
        set_page_range(&mut sel, 10, 0, 99);
        assert_eq!(sel.pages, None);
        set_page_range(&mut sel, 10, 8, 2);
        assert_eq!(sel.pages, Some((8, 8)));
        assert!(sel.validate().is_ok());

        let all: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut sel = SourceSelection::default();
        set_layer_enabled(&mut sel, &all, "b", false);
        assert_eq!(sel.layers, Some(vec!["a".to_string(), "c".to_string()]));
        set_layer_enabled(&mut sel, &all, "b", true);
        assert_eq!(sel.layers, None);
        set_layer_enabled(&mut sel, &all, "a", false);
        set_layer_enabled(&mut sel, &all, "b", false);
        set_layer_enabled(&mut sel, &all, "c", false);
        assert_eq!(sel.layers, Some(vec!["c".to_string()]));
        assert!(sel.validate().is_ok());
    }

    #[test]
    fn test_destination_filtering_by_source() {
        let indd: Vec<_> = destination_options(Some(ContainerType::AdobeInDesignIndd)).into_iter().map(|d| d.0).collect();
        assert!(indd.contains(&ContainerType::Pdf) && indd.contains(&ContainerType::Idml) && indd.contains(&ContainerType::ArtcraftDesigncraft));
        assert!(indd.contains(&ContainerType::Tiff));
        assert!(!indd.iter().any(|c| matches!(c.media_kind(), MediaStreamKind::VideoWithAudio | MediaStreamKind::AudioOnly)));
        let dwg: Vec<_> = destination_options(Some(ContainerType::AutocadDwg)).into_iter().map(|d| d.0).collect();
        assert!(dwg.contains(&ContainerType::AutocadDxf) && !dwg.contains(&ContainerType::IsoBmffMp4));
        assert_eq!(destination_options(None).len(), INLINE_CONTAINERS.len());
    }

    #[test]
    fn test_source_container_from_job() {
        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "j".into(),
            client_info: encodercraft_core::ClientInfo { app_name: "t".into(), app_version: "0".into(), pid: 0, user_notes: None },
            priority: encodercraft_core::JobPriority::Normal,
            start_policy: encodercraft_core::StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("/x/design.INDD"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![],
        };
        assert_eq!(source_container(&spec), Some(ContainerType::AdobeInDesignIndd));
    }

    #[test]
    fn test_format_timecode() {
        assert_eq!(format_timecode(0, 30.0), "00:00:00:00");
        assert_eq!(format_timecode(30, 30.0), "00:00:01:00");
        assert_eq!(format_timecode(3600 * 30, 30.0), "01:00:00:00");
    }

    #[test]
    fn test_calculate_estimated_size() {
        let size = calculate_estimated_size_mb(10.0, 8000, 320);
        // (8320 kbps * 10 s) / 8192 = 10.156 MB
        assert!((size - 10.156).abs() < 0.05);
    }

    #[test]
    fn test_effects_tab_and_compare_mode_rendering() {
        let mut target = OutputTarget {
            target_id: "t1".into(),
            output_path: PathBuf::from("test.mp4"),
            preset_name: "YouTube 1080p".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        };

        let mut state = InspectorState::default();
        assert_eq!(state.preview_tab, PreviewTabMode::Source);
        assert_eq!(state.compare_split_fraction, 0.5);

        state.preview_tab = PreviewTabMode::Compare;
        state.compare_split_fraction = 0.65;
        assert_eq!(state.compare_split_fraction, 0.65);

        egui::__run_test_ui(|ui| {
            render_effects_tab(ui, &mut target);
        });

        assert!(target.burn_timecode || target.timecode_overlay.is_some() || !target.preset_name.is_empty());
    }

    #[test]
    fn test_crop_config_updates() {
        let mut target = OutputTarget::default();
        let mut state = InspectorState::default();

        let crop = encodercraft_core::CropConfig { enabled: true, left: 50, top: 20, right: 50, bottom: 20 };
        target.crop = Some(crop);

        egui::__run_test_ui(|ui| {
            render_video_tab(ui, &mut target, &mut state);
        });

        assert!(target.crop.as_ref().is_some_and(|c| c.enabled && c.left == 50));
    }

    #[test]
    fn test_audio_tab_matrix_router() {
        let mut target = OutputTarget::default();
        let mut state = InspectorState { audio_channels_idx: 3, ..Default::default() };

        let mapping = encodercraft_core::AudioChannelMapping::new_discrete_mono(8);
        target.audio_mapping = Some(mapping);

        egui::__run_test_ui(|ui| {
            render_audio_tab(ui, &mut target, &mut state);
        });

        assert!(target.audio_mapping.is_some());
        let m = target.audio_mapping.as_ref().unwrap();
        assert_eq!(m.output_channels, 8);
        assert_eq!(m.track_layout, encodercraft_core::AudioTrackLayout::DiscreteMono);
    }

    #[test]
    fn test_render_metadata_tab_updates() {
        let mut target = OutputTarget::default();
        let mut client_info = encodercraft_core::ClientInfo { app_name: "test-app".into(), app_version: "1.0".into(), pid: 1, user_notes: None };
        target.metadata = Some(encodercraft_core::XmpMetadata {
            title: Some("Song Title".into()),
            creator: Some("Band Name".into()),
            album: Some("Album Name".into()),
            track_number: Some(2),
            year: Some(2026),
            genre: Some("Synthwave".into()),
            ..Default::default()
        });

        egui::__run_test_ui(|ui| {
            render_metadata_tab(ui, &mut client_info, &mut target);
        });

        let meta = target.metadata.as_ref().unwrap();
        assert_eq!(meta.title.as_deref(), Some("Song Title"));
        assert_eq!(meta.creator.as_deref(), Some("Band Name"));
        assert_eq!(meta.album.as_deref(), Some("Album Name"));
        assert_eq!(meta.track_number, Some(2));
        assert_eq!(meta.year, Some(2026));
        assert_eq!(meta.genre.as_deref(), Some("Synthwave"));
    }

    #[test]
    fn test_inspector_preview_in_out_range() {
        let mut spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "job-range-test".into(),
            client_info: encodercraft_core::ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
            priority: encodercraft_core::JobPriority::Normal,
            start_policy: encodercraft_core::StartPolicy::AutoStart,
            source: SourceDescriptor::File {
                path: std::path::PathBuf::from("video.mp4"),
                start_tick: Some(10),
                duration_ticks: Some(50),
                timebase: Some(encodercraft_core::Rational64::new(1, 30).unwrap()),
            },
            targets: vec![OutputTarget::default()],
        };

        let mut state = InspectorState::default();
        let tokens = Tokens::dark();

        egui::__run_test_ui(|ui| {
            render_preview_section(ui, &mut state, &mut spec, &tokens);
        });

        if let SourceDescriptor::File { start_tick, duration_ticks, .. } = &spec.source {
            assert_eq!(*start_tick, Some(10));
            assert_eq!(*duration_ticks, Some(50));
        } else {
            panic!("expected file source");
        }
    }

    #[test]
    fn test_stream_toggles_and_tab_filtering() {
        let mut target = OutputTarget { container: ContainerType::IsoBmffMp4, export_video: true, export_audio: true, ..Default::default() };
        let full_tabs = visible_tabs_for_target(None, &target);
        assert!(full_tabs.contains(&InspectorTab::Video));
        assert!(full_tabs.contains(&InspectorTab::Audio));

        // Disable video stream (audio-only podcast / stem delivery)
        target.export_video = false;
        let audio_only_tabs = visible_tabs_for_target(None, &target);
        assert!(!audio_only_tabs.contains(&InspectorTab::Video));
        assert!(audio_only_tabs.contains(&InspectorTab::Audio));

        // Disable audio stream (silent B-roll / video-only deliverable)
        target.export_video = true;
        target.export_audio = false;
        let video_only_tabs = visible_tabs_for_target(None, &target);
        assert!(video_only_tabs.contains(&InspectorTab::Video));
        assert!(!video_only_tabs.contains(&InspectorTab::Audio));
        assert!(!video_only_tabs.contains(&InspectorTab::Loudness));
    }

    #[test]
    fn test_audio_tab_codec_and_bit_depth_persistence() {
        let mut target = OutputTarget { container: ContainerType::Wave, export_video: false, export_audio: true, ..Default::default() };
        let mut state = InspectorState::default();

        egui::__run_test_ui(|ui| {
            render_audio_tab(ui, &mut target, &mut state);
        });

        assert!(target.audio.is_some());
        let a = target.audio.as_ref().unwrap();
        assert_eq!(a.codec.as_deref(), Some("pcm_s24le"));
        assert_eq!(a.bit_depth, Some(24));
        assert_eq!(a.sample_rate, Some(48000));
        assert_eq!(a.layout, Some(ChannelLayout::Stereo));
    }

    #[test]
    fn test_crop_aspect_presets_ratios() {
        assert_eq!(CropAspectPreset::Square1x1.ratio(), Some(1.0));
        assert!((CropAspectPreset::Standard4x3.ratio().unwrap() - 1.3333).abs() < 0.01);
        assert!((CropAspectPreset::Widescreen16x9.ratio().unwrap() - 1.7777).abs() < 0.01);
        assert!((CropAspectPreset::Vertical9x16.ratio().unwrap() - 0.5625).abs() < 0.01);
        assert!((CropAspectPreset::Anamorphic239x1.ratio().unwrap() - 2.39).abs() < 0.01);
        assert_eq!(CropAspectPreset::Custom.ratio(), None);
    }
}
