//! Preview monitor: source, rendered output and A/B views with transport, In/Out range,
//! zoom, safe margins, aspect guides and video scopes.

use egui::{Color32, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;
use std::time::Instant;

use encodercraft_core::{CraftJobSpec, SourceDescriptor};

use crate::icons;
use crate::monitor_geom::{self, ACTION_SAFE, TITLE_SAFE, ZoomMode};
use crate::output_render::OutputRender;
use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::scope_view::{self, ScopeMode};
use crate::theme::Tokens;
use crate::transport::{Transport, timecode};

/// Preview display modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Source,
    Output,
    SideBySideSplit,
}

/// Aspect ratios offered as crop guides.
const GUIDES: [(&str, f32); 5] = [("16:9", 16.0 / 9.0), ("4:3", 4.0 / 3.0), ("1:1", 1.0), ("9:16", 9.0 / 16.0), ("2.39:1", 2.39)];

/// One decoded view (source or output) with its GPU texture.
#[derive(Default)]
struct Slot {
    decoder: Option<PreviewDecoder>,
    texture: Option<egui::TextureHandle>,
    shown_frame: Option<u64>,
    requested_frame: Option<u64>,
    frame_count: u64,
    fps: f64,
    size: Option<(u32, u32)>,
    summary: Option<String>,
    message: Option<String>,
    /// The frame on screen as RGBA8, kept for the scopes.
    pixels: Option<(usize, usize, Vec<u8>)>,
}

impl Slot {
    fn open(&mut self, path: Option<PathBuf>, missing: &str) {
        *self = Slot::default();
        match path {
            Some(p) if p.is_file() => self.decoder = Some(PreviewDecoder::open(p)),
            Some(_) | None => self.message = Some(missing.to_string()),
        }
    }

    fn path(&self) -> Option<&std::path::Path> {
        self.decoder.as_ref().map(PreviewDecoder::path)
    }

    fn pump(&mut self, ctx: &egui::Context, name: &str, frame: u64) {
        let Some(decoder) = &self.decoder else { return };
        while let Some(ev) = decoder.poll() {
            ctx.request_repaint();
            match ev {
                PreviewEvent::Opened { probe, frame_count } => {
                    self.frame_count = frame_count;
                    if let Some(v) = &probe.video {
                        self.fps = v.frame_rate.num as f64 / v.frame_rate.den.max(1) as f64;
                        self.size = Some((v.width, v.height));
                        self.summary = Some(format!("{}x{} · {} · {:.3} fps", v.width, v.height, v.codec, self.fps));
                    } else {
                        self.message = Some("Audio-only source: no picture to preview".into());
                    }
                    self.requested_frame = None;
                }
                PreviewEvent::Frame { index, width, height, rgba } => {
                    if width > 0 && height > 0 {
                        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                        match &mut self.texture {
                            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                            None => self.texture = Some(ctx.load_texture(format!("preview-{name}"), image, egui::TextureOptions::LINEAR)),
                        }
                        self.shown_frame = Some(index);
                        self.pixels = Some((width, height, rgba));
                        self.message = None;
                    }
                }
                PreviewEvent::Failed(msg) => self.message = Some(msg),
            }
        }
        let wanted = frame.min(self.frame_count.saturating_sub(1));
        if self.frame_count > 0 && self.requested_frame != Some(wanted) {
            decoder.request_frame(wanted);
            self.requested_frame = Some(wanted);
            ctx.request_repaint();
        } else if self.decoder.is_some() && self.texture.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

/// Active media domain loaded into the universal preview monitor.
#[derive(Clone, Debug)]
pub enum UniversalMediaState {
    Audio(AudioPreview),
    Document(DocumentPreview),
    Vector(VectorPreview),
    Spatial3D(Spatial3DPreview),
}

#[derive(Clone, Debug)]
pub struct AudioPreview {
    pub path: PathBuf,
    pub format_name: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub waveform_peaks: Vec<f32>,
    pub summary: String,
}

#[derive(Clone)]
pub struct DocumentPreview {
    pub path: PathBuf,
    pub title: String,
    pub current_page: usize,
    pub total_pages: usize,
    pub pages_text: Vec<Vec<String>>,
    pub summary: String,
    pub cached_texture: Option<(usize, egui::TextureHandle)>,
}

impl std::fmt::Debug for DocumentPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentPreview")
            .field("path", &self.path)
            .field("title", &self.title)
            .field("current_page", &self.current_page)
            .field("total_pages", &self.total_pages)
            .field("summary", &self.summary)
            .finish()
    }
}

impl DocumentPreview {
    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
        }
    }

    pub fn next_page(&mut self) {
        if self.current_page + 1 < self.total_pages {
            self.current_page += 1;
        }
    }
}

#[derive(Clone)]
pub struct VectorPreview {
    pub path: PathBuf,
    pub format_name: String,
    pub width: f32,
    pub height: f32,
    pub element_count: usize,
    pub texture: Option<egui::TextureHandle>,
    pub summary: String,
}

impl std::fmt::Debug for VectorPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorPreview")
            .field("path", &self.path)
            .field("format_name", &self.format_name)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("element_count", &self.element_count)
            .field("summary", &self.summary)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct Spatial3DPreview {
    pub path: PathBuf,
    pub format_name: String,
    pub vertex_count: usize,
    pub face_count: usize,
    pub vertices: Vec<[f32; 3]>,
    pub faces: Vec<[u32; 3]>,
    pub aabb: ([f32; 3], [f32; 3]),
    pub pitch: f32,
    pub yaw: f32,
    pub distance: f32,
    pub pan: Vec2,
    pub wireframe: bool,
    pub summary: String,
}

impl Spatial3DPreview {
    pub fn reset_camera(&mut self) {
        let dx = self.aabb.1[0] - self.aabb.0[0];
        let dy = self.aabb.1[1] - self.aabb.0[1];
        let dz = self.aabb.1[2] - self.aabb.0[2];
        let max_extent = dx.max(dy).max(dz).max(0.001);
        self.pitch = 0.35;
        self.yaw = 0.55;
        self.distance = max_extent * 2.5;
        self.pan = Vec2::ZERO;
    }
}

/// State for the preview canvas and scrub controls.
pub struct PreviewState {
    pub active_job_id: Option<String>,
    pub mode: PreviewMode,
    pub transport: Transport,
    pub split_fraction: f32,
    pub zoom: ZoomMode,
    pub pan: Vec2,
    pub safe_margins: bool,
    pub guide: Option<usize>,
    pub scope: ScopeMode,
    pub universal_media: Option<UniversalMediaState>,
    source: Slot,
    output: Slot,
    /// Source frame at which the open output file starts.
    output_base: u64,
    render: Option<OutputRender>,
    render_error: Option<String>,
    scope_texture: Option<egui::TextureHandle>,
    scope_key: Option<(ScopeMode, PreviewMode, Option<u64>)>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            active_job_id: None,
            mode: PreviewMode::Source,
            transport: Transport::default(),
            split_fraction: 0.5,
            zoom: ZoomMode::Fit,
            pan: Vec2::ZERO,
            safe_margins: false,
            guide: None,
            scope: ScopeMode::Off,
            universal_media: None,
            source: Slot::default(),
            output: Slot::default(),
            output_base: 0,
            render: None,
            render_error: None,
            scope_texture: None,
            scope_key: None,
        }
    }
}

fn source_path(job: &CraftJobSpec) -> Option<PathBuf> {
    match &job.source {
        SourceDescriptor::File { path, .. } => Some(path.clone()),
        _ => None,
    }
}

fn paint_slot(painter: &egui::Painter, rect: Rect, slot: &Slot, clip: Option<Rect>, tokens: &Tokens) {
    let painter = match clip {
        Some(c) => painter.with_clip_rect(c),
        None => painter.clone(),
    };
    if let Some(tex) = &slot.texture {
        painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    } else {
        let text = slot.message.clone().unwrap_or_else(|| "Loading…".to_string());
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.0), tokens.text_dim);
    }
}

impl PreviewState {
    fn select(&mut self, j: &CraftJobSpec) {
        self.active_job_id = Some(j.job_id.clone());
        self.transport = Transport::default();
        self.render = None;
        self.render_error = None;
        self.output_base = 0;
        self.pan = Vec2::ZERO;
        self.scope_key = None;
        self.universal_media = None;
        let missing = match &j.source {
            SourceDescriptor::File { .. } => "Source file is offline",
            _ => "Preview is not available for this source type",
        };
        let sp = source_path(j);
        self.source.open(sp.clone(), missing);
        self.output.open(None, "Output has not been rendered yet");
        if let Some(path) = sp {
            self.load_universal(&path);
        }
        if let SourceDescriptor::File { start_tick, duration_ticks, .. } = &j.source {
            let start = start_tick.and_then(|s| u64::try_from(s).ok());
            self.transport.in_point = start;
            self.transport.out_point = match (start, duration_ticks) {
                (Some(s), Some(d)) if *d > 0 => Some(s.saturating_add((*d as u64).saturating_sub(1))),
                (None, Some(d)) if *d > 0 => Some((*d as u64).saturating_sub(1)),
                _ => None,
            };
        }
    }

    fn clear(&mut self) {
        self.active_job_id = None;
        self.source = Slot::default();
        self.output = Slot::default();
        self.source.message = Some("No Active Clip Selected".into());
        self.transport = Transport::default();
        self.render = None;
        self.universal_media = None;
    }

    fn load_universal(&mut self, path: &std::path::Path) {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        match ext.as_str() {
            "wav" | "mp3" | "flac" | "aiff" | "aif" | "ogg" | "oga" | "m4a" | "aac" | "opus" | "caf" => {
                if let Ok(bytes) = std::fs::read(path)
                    && let Ok(buffer) = encodercraft_audio_dsp::io::decode_audio(&bytes)
                {
                    let sample_rate = buffer.sample_rate;
                    let channels = buffer.channel_count() as u16;
                    let dur = buffer.samples_per_channel() as f64 / buffer.sample_rate.max(1) as f64;
                    let planar = buffer.into_planar_channels();
                    let samples = planar.first().cloned().unwrap_or_default();
                    let peaks_overview = craft_audio::peaks::Peaks::build(&samples);
                    let num_points = 256;
                    let mut waveform = Vec::with_capacity(num_points);
                    let total_samples = samples.len();
                    if total_samples > 0 {
                        let step = total_samples as f32 / num_points as f32;
                        for i in 0..num_points {
                            let start = (i as f32 * step) as usize;
                            let end = (((i + 1) as f32 * step) as usize).min(total_samples);
                            let (min, max) = peaks_overview.minmax(&samples, start, end);
                            waveform.push(max.abs().max(min.abs()).clamp(0.0, 1.0));
                        }
                    }
                    let summary = format!("Audio: {} · {}Hz · {} ch · {:.2}s", ext.to_ascii_uppercase(), sample_rate, channels, dur);
                    self.source.summary = Some(summary.clone());
                    let frame_count = (dur * 30.0).max(1.0).ceil() as u64;
                    self.transport.set_media(30.0, frame_count);
                    self.source.frame_count = frame_count;
                    self.source.fps = 30.0;
                    self.source.size = Some((800, 200));
                    self.universal_media = Some(UniversalMediaState::Audio(AudioPreview {
                        path: path.to_path_buf(),
                        format_name: ext.to_ascii_uppercase(),
                        duration_seconds: dur,
                        sample_rate,
                        channels,
                        waveform_peaks: waveform,
                        summary,
                    }));
                }
            }
            "docx" | "pdf" | "md" | "markdown" | "html" | "htm" | "txt" | "idml" | "indd" | "epub" | "pptx" => {
                let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Document".to_string());
                let bytes = std::fs::read(path).unwrap_or_default();
                let text_res = std::fs::read_to_string(path);

                let canvas_res = match ext.as_str() {
                    "docx" => encodercraft_pipeline::document::DocumentCanvas::from_docx_bytes(&bytes),
                    "pdf" => encodercraft_pipeline::document::DocumentCanvas::from_pdf_bytes(&bytes),
                    "md" | "markdown" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_pipeline::document::DocumentCanvas::from_text(&file_name, text, true)
                    }
                    "html" | "htm" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_pipeline::document::DocumentCanvas::from_html(&file_name, text)
                    }
                    "idml" => encodercraft_pipeline::document::DocumentCanvas::from_idml_bytes(&bytes),
                    "epub" => encodercraft_pipeline::document::DocumentCanvas::from_epub_bytes(&bytes),
                    "pptx" => encodercraft_pipeline::document::DocumentCanvas::from_pptx_bytes(&bytes),
                    _ => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_pipeline::document::DocumentCanvas::from_text(&file_name, text, false)
                    }
                };

                let mut pages_text = Vec::new();
                if let Ok(canvas) = canvas_res {
                    for page in &canvas.pages {
                        let mut page_lines = Vec::new();
                        for item in &page.items {
                            if let encodercraft_pipeline::document::DocumentItem::Text { text, .. } = item {
                                for line in text.lines() {
                                    if !line.trim().is_empty() {
                                        page_lines.push(line.trim().to_string());
                                    }
                                }
                            }
                        }
                        if page_lines.is_empty() {
                            page_lines.push(format!("Page {}", page.page_number));
                        }
                        pages_text.push(page_lines);
                    }
                } else if let Ok(text) = text_res {
                    let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
                    if !lines.is_empty() {
                        pages_text.push(lines);
                    }
                }

                if pages_text.is_empty() {
                    pages_text.push(vec![file_name.clone(), format!("Document format: {}", ext.to_ascii_uppercase())]);
                }

                let mut total_pages = pages_text.len();
                if ext.as_str() == "pdf"
                    && let Ok(count) = encodercraft_pipeline::pdf_page_count(&bytes)
                {
                    total_pages = total_pages.max(count);
                }
                let summary = format!("Document: {} · {} page(s)", ext.to_ascii_uppercase(), total_pages);
                self.source.summary = Some(summary.clone());
                self.source.size = Some((612, 792));
                self.source.frame_count = total_pages as u64;
                self.source.fps = 1.0;
                self.transport.set_media(1.0, total_pages as u64);
                self.universal_media = Some(UniversalMediaState::Document(DocumentPreview {
                    path: path.to_path_buf(),
                    title: file_name,
                    current_page: 0,
                    total_pages,
                    pages_text,
                    summary,
                    cached_texture: None,
                }));
            }
            "svg" | "eps" | "ai" | "wmf" | "vectorcraft" => {
                let bytes = std::fs::read(path).unwrap_or_default();
                let text_res = std::fs::read_to_string(path);

                let canvas_res = match ext.as_str() {
                    "svg" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_image_codecs::vector::VectorCanvas::from_svg(text)
                    }
                    "eps" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_image_codecs::vector::VectorCanvas::from_eps(text)
                    }
                    "wmf" => encodercraft_image_codecs::vector::VectorCanvas::from_wmf_bytes(&bytes),
                    "ai" => encodercraft_image_codecs::vector::VectorCanvas::from_ai_or_pdf(&bytes),
                    "vectorcraft" => encodercraft_image_codecs::vector::VectorCanvas::from_vectorcraft(&bytes),
                    _ => encodercraft_image_codecs::vector::VectorCanvas::from_svg(text_res.as_deref().unwrap_or("")),
                };

                let (w, h, elem_count, pixels) = match canvas_res {
                    Ok(canvas) => {
                        let rgba = canvas.render_to_rgba8();
                        let w = canvas.width as f32;
                        let h = canvas.height as f32;
                        let count = canvas.paths.len() + canvas.texts.len();
                        (w, h, count, Some(rgba))
                    }
                    Err(_) => (800.0, 600.0, 0, None),
                };

                let summary = format!("Vector: {} · {:.0}x{:.0} pt · {} elements", ext.to_ascii_uppercase(), w, h, elem_count);
                self.source.summary = Some(summary.clone());
                self.source.size = Some((w.max(1.0) as u32, h.max(1.0) as u32));
                self.source.frame_count = 1;
                self.source.fps = 1.0;
                self.transport.set_media(1.0, 1);
                if let Some(px) = pixels {
                    let (uw, uh) = ((w.max(1.0).round() as usize).min(2048), (h.max(1.0).round() as usize).min(2048));
                    self.source.pixels = Some((uw, uh, px));
                }
                self.universal_media = Some(UniversalMediaState::Vector(VectorPreview {
                    path: path.to_path_buf(),
                    format_name: ext.to_ascii_uppercase(),
                    width: w,
                    height: h,
                    element_count: elem_count,
                    texture: None,
                    summary,
                }));
            }
            "stl" | "obj" | "step" | "stp" | "glb" | "gltf" | "ply" | "usdz" | "usdc" => {
                let bytes = std::fs::read(path).unwrap_or_default();
                let text_res = std::fs::read_to_string(path);

                let mesh_res = match ext.as_str() {
                    "stl" => encodercraft_spatial_io::stl::parse_stl(&bytes),
                    "obj" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_spatial_io::obj::parse_obj(text)
                    }
                    "glb" => encodercraft_spatial_io::gltf::parse_glb(&bytes),
                    "ply" => encodercraft_spatial_io::ply::parse_ply_binary(&bytes),
                    "step" | "stp" => {
                        let text = text_res.as_deref().unwrap_or("");
                        encodercraft_spatial_io::step::parse_step(text)
                    }
                    _ => encodercraft_spatial_io::stl::parse_stl(&bytes),
                };

                let mesh = mesh_res.unwrap_or_else(|_| encodercraft_spatial_io::TriangleMesh::new("ImportedMesh"));
                let aabb = mesh.bounding_box().unwrap_or(([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]));
                let dx = aabb.1[0] - aabb.0[0];
                let dy = aabb.1[1] - aabb.0[1];
                let dz = aabb.1[2] - aabb.0[2];
                let max_extent = dx.max(dy).max(dz).max(0.001);
                let distance = max_extent * 2.5;

                let vertex_count = mesh.vertices.len();
                let face_count = mesh.indices.len();
                let summary = format!("3D Model: {} · {} vertices · {} triangles", ext.to_ascii_uppercase(), vertex_count, face_count);
                self.source.summary = Some(summary.clone());
                self.source.size = Some((800, 600));
                self.source.frame_count = 1;
                self.source.fps = 1.0;
                self.transport.set_media(1.0, 1);

                self.universal_media = Some(UniversalMediaState::Spatial3D(Spatial3DPreview {
                    path: path.to_path_buf(),
                    format_name: ext.to_ascii_uppercase(),
                    vertex_count,
                    face_count,
                    vertices: mesh.vertices,
                    faces: mesh.indices,
                    aabb,
                    pitch: 0.35,
                    yaw: 0.55,
                    distance,
                    pan: Vec2::ZERO,
                    wireframe: false,
                    summary,
                }));
            }
            _ => {}
        }
    }

    fn start_render(&mut self, job: &CraftJobSpec) {
        self.render_error = None;
        match OutputRender::start(job, self.transport.position(), self.transport.fps(), self.source.frame_count) {
            Ok(r) => self.render = Some(r),
            Err(e) => self.render_error = Some(e.to_string()),
        }
    }

    fn poll_render(&mut self) {
        let Some(render) = &self.render else { return };
        match render.poll() {
            Some(Ok(())) => {
                self.output_base = render.base_frame;
                let path = render.path.clone();
                self.output.open(Some(path), "Output has not been rendered yet");
                self.render = None;
            }
            Some(Err(e)) => {
                self.render_error = Some(e);
                self.render = None;
            }
            None => {}
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let now = Instant::now();
        ctx.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                return;
            }
            let big = if i.modifiers.shift { 10 } else { 1 };
            if i.key_pressed(egui::Key::Space) {
                self.transport.toggle_play(now);
            }
            if i.key_pressed(egui::Key::L) {
                self.transport.shuttle_forward(now);
            }
            if i.key_pressed(egui::Key::J) {
                self.transport.shuttle_reverse(now);
            }
            if i.key_pressed(egui::Key::K) {
                self.transport.stop();
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                self.transport.step(big);
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                self.transport.step(-big);
            }
            if i.key_pressed(egui::Key::Home) {
                self.transport.home();
            }
            if i.key_pressed(egui::Key::End) {
                self.transport.end();
            }
            if i.key_pressed(egui::Key::I) {
                self.transport.set_in();
            }
            if i.key_pressed(egui::Key::O) {
                self.transport.set_out();
            }
            if let Some(UniversalMediaState::Document(ref mut doc)) = self.universal_media {
                if i.key_pressed(egui::Key::PageUp) {
                    doc.prev_page();
                }
                if i.key_pressed(egui::Key::PageDown) {
                    doc.next_page();
                }
            }
        });
    }

    /// The slot and frame the scopes analyse for the current view.
    fn scope_input(&self) -> Option<(&(usize, usize, Vec<u8>), Option<u64>)> {
        let slot = if self.mode == PreviewMode::Output { &self.output } else { &self.source };
        slot.pixels.as_ref().map(|p| (p, slot.shown_frame))
    }

    fn refresh_scope(&mut self, ctx: &egui::Context) {
        if self.scope == ScopeMode::Off {
            self.scope_texture = None;
            self.scope_key = None;
            return;
        }
        let shown = self.scope_input().map(|(_, f)| f);
        let key = (self.scope, self.mode, shown.flatten());
        if self.scope_key == Some(key) {
            return;
        }
        let image = self.scope_input().and_then(|((w, h, px), _)| scope_view::render(self.scope, *w, *h, px));
        match image {
            Some(img) => {
                match &mut self.scope_texture {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => self.scope_texture = Some(ctx.load_texture("preview-scope", img, egui::TextureOptions::LINEAR)),
                }
                self.scope_key = Some(key);
            }
            None => self.scope_texture = None,
        }
    }
}

/// Renders the preview monitor pane. `output_ready` is true once the job's own output file is final.
pub fn render_preview_panel(ui: &mut Ui, state: &mut PreviewState, job: Option<&CraftJobSpec>, output_ready: bool, tokens: &Tokens) {
    if let Some(j) = job {
        if state.active_job_id.as_deref() != Some(j.job_id.as_str()) {
            state.select(j);
        }
        let final_out = j.targets.first().map(|t| t.output_path.clone());
        if output_ready
            && state.render.is_none()
            && let Some(out) = final_out
            && state.output.path() != Some(out.as_path())
            && out.is_file()
        {
            state.output_base = 0;
            state.output.open(Some(out), "Output has not been rendered yet");
        }
    } else if state.active_job_id.is_some() {
        state.clear();
    }

    let ctx = ui.ctx().clone();
    state.poll_render();
    state.handle_keys(&ctx);
    state.source.pump(&ctx, "source", state.transport.position());
    state.output.pump(&ctx, "output", state.transport.position().saturating_sub(state.output_base));
    if state.source.frame_count != state.transport.frame_count() || (state.source.fps > 0.0 && state.source.fps != state.transport.fps()) {
        state.transport.set_media(state.source.fps, state.source.frame_count);
    }
    if state.transport.advance(Instant::now()) || state.transport.is_playing() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    if state.source.decoder.is_some() || state.output.decoder.is_some() || state.render.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
    }
    state.refresh_scope(&ctx);

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Preview Monitor").size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut state.mode, PreviewMode::SideBySideSplit, "A/B Split");
                ui.selectable_value(&mut state.mode, PreviewMode::Output, "Output");
                ui.selectable_value(&mut state.mode, PreviewMode::Source, "Source");
            });
        });
        view_tools(ui, state, job);
        ui.add_space(4.0);

        let size_px = state.source.size.unwrap_or((16, 9));
        let available = ui.available_size();
        let aspect = size_px.0.max(1) as f32 / size_px.1.max(1) as f32;
        let canvas_height = (available.x / aspect.max(0.1)).clamp(120.0, 260.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(available.x, canvas_height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, tokens.bg_panel_darker);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, tokens.border_subtle), StrokeKind::Inside);

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let current = match state.zoom {
                    ZoomMode::Scale(s) => s,
                    _ => 1.0,
                };
                state.zoom = ZoomMode::Scale((current * (1.0 + scroll * 0.002)).clamp(0.25, 16.0));
            }
        }
        let inner = rect.shrink(4.0);
        let frame_rect = monitor_geom::view_rect(inner, size_px, state.zoom, state.pan);
        let clip = painter.with_clip_rect(inner);
        clip.rect_filled(frame_rect, 2.0, Color32::BLACK);

        match state.mode {
            PreviewMode::Source => {
                if let Some(ref mut universal) = state.universal_media {
                    paint_universal_media(ui.ctx(), &clip, frame_rect, &response, universal, &mut state.transport, tokens);
                } else {
                    paint_slot(&clip, frame_rect, &state.source, None, tokens);
                }
            }
            PreviewMode::Output => paint_output(&clip, frame_rect, state, tokens),
            PreviewMode::SideBySideSplit => {
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    state.split_fraction = monitor_geom::split_fraction(pos.x, frame_rect);
                }
                let (left, right) = monitor_geom::split_rects(frame_rect, state.split_fraction);
                if let Some(ref mut universal) = state.universal_media {
                    let left_clip = clip.with_clip_rect(left.intersect(inner));
                    paint_universal_media(ui.ctx(), &left_clip, frame_rect, &response, universal, &mut state.transport, tokens);
                } else {
                    paint_slot(&clip, frame_rect, &state.source, Some(left.intersect(inner)), tokens);
                }
                paint_slot(&clip, frame_rect, &state.output, Some(right.intersect(inner)), tokens);
                clip.line_segment(
                    [Pos2::new(left.max.x, frame_rect.min.y), Pos2::new(left.max.x, frame_rect.max.y)],
                    Stroke::new(1.5, tokens.status_processing),
                );
            }
        }
        if state.universal_media.is_none() && state.mode != PreviewMode::SideBySideSplit && response.dragged() && state.zoom != ZoomMode::Fit {
            state.pan += response.drag_delta();
        }
        if response.double_clicked() {
            state.zoom = ZoomMode::Fit;
            state.pan = Vec2::ZERO;
            if let Some(UniversalMediaState::Spatial3D(ref mut s)) = state.universal_media {
                s.reset_camera();
            }
        }

        paint_overlays(&clip, frame_rect, state, tokens);
        clip.rect_stroke(frame_rect, 2.0, Stroke::new(1.0, tokens.border_strong), StrokeKind::Inside);

        let caption = match (&state.mode, job) {
            (_, None) => "No Active Clip Selected".to_string(),
            (PreviewMode::Output, Some(_)) => state.output.path().map(|p| p.display().to_string()).unwrap_or_else(|| "Output not rendered".into()),
            (_, Some(_)) => state.source.summary.clone().unwrap_or_default(),
        };
        ui.label(RichText::new(caption).small().color(tokens.text_dim));
        if let Some(err) = &state.render_error {
            ui.label(RichText::new(format!("Preview render failed: {err}")).small().color(tokens.status_failed));
        }

        if let Some(tex) = &state.scope_texture {
            let width = ui.available_width();
            ui.add_space(4.0);
            ui.add(egui::Image::new((tex.id(), Vec2::new(width, 90.0))).fit_to_exact_size(Vec2::new(width, 90.0)));
        }

        ui.add_space(4.0);
        transport_bar(ui, state, tokens);
    });
}

fn paint_universal_media(
    ctx: &egui::Context,
    painter: &egui::Painter,
    frame_rect: Rect,
    response: &egui::Response,
    universal: &mut UniversalMediaState,
    transport: &mut Transport,
    tokens: &Tokens,
) {
    match universal {
        UniversalMediaState::Audio(audio) => {
            paint_audio_waveform(painter, frame_rect, response, audio, transport, tokens);
        }
        UniversalMediaState::Document(doc) => {
            paint_document_preview(ctx, painter, frame_rect, response, doc, transport, tokens);
        }
        UniversalMediaState::Vector(vec) => {
            paint_vector_preview(ctx, painter, frame_rect, vec, tokens);
        }
        UniversalMediaState::Spatial3D(spatial) => {
            paint_3d_viewport(painter, frame_rect, response, spatial, tokens);
        }
    }
}

fn paint_audio_waveform(
    painter: &egui::Painter,
    frame_rect: Rect,
    response: &egui::Response,
    audio: &AudioPreview,
    transport: &mut Transport,
    tokens: &Tokens,
) {
    // Studio soundboard background
    painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(18, 20, 24));

    let mid_y = frame_rect.center().y;
    // Centerline
    painter.line_segment([Pos2::new(frame_rect.min.x, mid_y), Pos2::new(frame_rect.max.x, mid_y)], Stroke::new(1.0, tokens.border_subtle));

    // Waveform bars
    let n = audio.waveform_peaks.len();
    if n > 0 {
        let bar_w = frame_rect.width() / n as f32;
        let max_h = (frame_rect.height() * 0.42).max(10.0);
        for (i, &peak) in audio.waveform_peaks.iter().enumerate() {
            let x = frame_rect.min.x + i as f32 * bar_w;
            let h = (peak * max_h).max(1.0);
            painter.line_segment([Pos2::new(x, mid_y - h), Pos2::new(x, mid_y + h)], Stroke::new((bar_w * 0.85).max(1.0), tokens.accent_primary));
        }
    }

    // Interactive scrub
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let scrub_frac = ((pos.x - frame_rect.min.x) / frame_rect.width().max(1.0)).clamp(0.0, 1.0);
        let target_frame = (scrub_frac * transport.last_frame() as f32).round() as u64;
        transport.seek(target_frame);
    }

    // Playhead line
    let frac = (transport.position() as f32 / transport.last_frame().max(1) as f32).clamp(0.0, 1.0);
    let play_x = frame_rect.min.x + frame_rect.width() * frac;
    painter.line_segment([Pos2::new(play_x, frame_rect.min.y), Pos2::new(play_x, frame_rect.max.y)], Stroke::new(2.0, tokens.status_processing));

    // HUD tags
    painter.text(
        frame_rect.left_top() + Vec2::new(8.0, 8.0),
        egui::Align2::LEFT_TOP,
        format!("{} ({} ch · {} Hz)", audio.format_name, audio.channels, audio.sample_rate),
        egui::FontId::proportional(11.0),
        tokens.text_primary,
    );
    painter.text(
        frame_rect.right_top() + Vec2::new(-8.0, 8.0),
        egui::Align2::RIGHT_TOP,
        timecode(transport.position(), transport.fps()),
        egui::FontId::monospace(12.0),
        tokens.accent_primary,
    );
}

fn paint_document_preview(
    ctx: &egui::Context,
    painter: &egui::Painter,
    frame_rect: Rect,
    response: &egui::Response,
    doc: &mut DocumentPreview,
    transport: &mut Transport,
    tokens: &Tokens,
) {
    // Backdrop
    painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(26, 28, 32));

    // Synchronize page with transport
    if doc.total_pages > 0 {
        doc.current_page = (transport.position() as usize).min(doc.total_pages.saturating_sub(1));
    }

    // Interactive scrub across pages
    if doc.total_pages > 1
        && (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let scrub_frac = ((pos.x - frame_rect.min.x) / frame_rect.width().max(1.0)).clamp(0.0, 1.0);
        let target_page = (scrub_frac * (doc.total_pages.saturating_sub(1)) as f32).round() as u64;
        transport.seek(target_page);
        doc.current_page = (target_page as usize).min(doc.total_pages.saturating_sub(1));
        ctx.request_repaint();
    }

    let is_pdf = doc.path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        let needs_load = match &doc.cached_texture {
            Some((page_idx, _)) => *page_idx != doc.current_page,
            None => true,
        };
        if needs_load
            && let Ok(bytes) = std::fs::read(&doc.path)
            && let Ok((width, height, rgba)) = encodercraft_pipeline::render_pdf_page_to_rgba(&bytes, doc.current_page, 960.0)
        {
            let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            let tex = ctx.load_texture(format!("pdf-preview-{}-{}", doc.path.display(), doc.current_page), image, egui::TextureOptions::LINEAR);
            doc.cached_texture = Some((doc.current_page, tex));
            ctx.request_repaint();
        }
    }

    if let Some((page_idx, ref tex)) = doc.cached_texture
        && page_idx == doc.current_page
    {
        let tex_size = tex.size_vec2();
        let aspect = (tex_size.x / tex_size.y.max(1.0)).clamp(0.2, 5.0);
        let paper_h = (frame_rect.height() - 24.0).max(60.0);
        let paper_w = (paper_h * aspect).min(frame_rect.width() - 20.0);
        let paper_rect = Rect::from_center_size(frame_rect.center() - Vec2::new(0.0, 6.0), Vec2::new(paper_w, paper_h));

        let shadow_rect = paper_rect.translate(Vec2::new(2.0, 3.0));
        painter.rect_filled(shadow_rect, 2.0, Color32::from_black_alpha(80));

        painter.image(tex.id(), paper_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        painter.rect_stroke(paper_rect, 1.0, Stroke::new(1.0, Color32::from_rgb(100, 105, 115)), StrokeKind::Outside);

        painter.text(
            Pos2::new(paper_rect.center().x, frame_rect.bottom() - 12.0),
            egui::Align2::CENTER_CENTER,
            format!("Page {} of {}", doc.current_page + 1, doc.total_pages),
            egui::FontId::proportional(11.0),
            tokens.text_primary,
        );
        return;
    }

    // Centered sheet of paper (aspect ratio ~ 0.77 for standard 612x792 pt page)
    let paper_h = (frame_rect.height() - 16.0).max(60.0);
    let paper_w = (paper_h * (612.0 / 792.0)).min(frame_rect.width() - 20.0);
    let paper_rect = Rect::from_center_size(frame_rect.center(), Vec2::new(paper_w, paper_h));

    // Drop shadow
    let shadow_rect = paper_rect.translate(Vec2::new(2.0, 3.0));
    painter.rect_filled(shadow_rect, 2.0, Color32::from_black_alpha(80));

    // Paper background
    painter.rect_filled(paper_rect, 2.0, Color32::from_rgb(248, 248, 250));
    painter.rect_stroke(paper_rect, 2.0, Stroke::new(1.0, Color32::from_rgb(190, 192, 198)), StrokeKind::Outside);

    // Document header bar
    let header_y = paper_rect.top() + 16.0;
    painter.text(
        Pos2::new(paper_rect.left() + 14.0, header_y),
        egui::Align2::LEFT_TOP,
        &doc.title,
        egui::FontId::proportional(12.0),
        Color32::from_rgb(20, 22, 28),
    );
    painter.line_segment(
        [Pos2::new(paper_rect.left() + 14.0, header_y + 18.0), Pos2::new(paper_rect.right() - 14.0, header_y + 18.0)],
        Stroke::new(1.0, Color32::from_rgb(215, 218, 224)),
    );

    // Page text lines
    if let Some(lines) = doc.pages_text.get(doc.current_page) {
        let mut y = header_y + 26.0;
        let line_height = 13.5;
        for line in lines.iter().take(14) {
            let truncated = if line.len() > 65 {
                let end = line.char_indices().map(|(idx, _)| idx).take(62).last().unwrap_or(line.len());
                format!("{}...", &line[..end])
            } else {
                line.clone()
            };
            painter.text(
                Pos2::new(paper_rect.left() + 14.0, y),
                egui::Align2::LEFT_TOP,
                truncated,
                egui::FontId::proportional(10.5),
                Color32::from_rgb(45, 48, 55),
            );
            y += line_height;
            if y > paper_rect.bottom() - 25.0 {
                break;
            }
        }
    }

    // Page footer indicator
    painter.text(
        Pos2::new(paper_rect.center().x, paper_rect.bottom() - 12.0),
        egui::Align2::CENTER_CENTER,
        format!("Page {} of {}", doc.current_page + 1, doc.total_pages),
        egui::FontId::proportional(10.0),
        Color32::from_rgb(110, 115, 125),
    );
}

fn paint_vector_preview(ctx: &egui::Context, painter: &egui::Painter, frame_rect: Rect, vec_prev: &mut VectorPreview, tokens: &Tokens) {
    painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(24, 25, 28));

    if vec_prev.texture.is_none()
        && let Ok(canvas) = encodercraft_image_codecs::vector::VectorCanvas::from_svg(&std::fs::read_to_string(&vec_prev.path).unwrap_or_default())
    {
        let rgba = canvas.render_to_rgba8();
        let w = (canvas.width.max(1.0).round() as usize).min(2048);
        let h = (canvas.height.max(1.0).round() as usize).min(2048);
        let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        vec_prev.texture = Some(ctx.load_texture("vector-preview", img, egui::TextureOptions::LINEAR));
    }

    if let Some(tex) = &vec_prev.texture {
        let size = tex.size_vec2();
        let aspect = size.x / size.y.max(1.0);
        let fitted = monitor_geom::fit_rect(frame_rect.shrink(8.0), aspect);
        painter.image(tex.id(), fitted, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        painter.rect_stroke(fitted, 0.0, Stroke::new(1.0, tokens.border_subtle), StrokeKind::Outside);
    } else {
        painter.text(
            frame_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("Vector Graphic: {}", vec_prev.format_name),
            egui::FontId::proportional(13.0),
            tokens.text_dim,
        );
    }

    painter.text(
        frame_rect.left_top() + Vec2::new(8.0, 8.0),
        egui::Align2::LEFT_TOP,
        &vec_prev.summary,
        egui::FontId::proportional(11.0),
        tokens.text_primary,
    );
}

fn paint_3d_viewport(painter: &egui::Painter, frame_rect: Rect, response: &egui::Response, spatial: &mut Spatial3DPreview, tokens: &Tokens) {
    if response.hovered() {
        let scroll = response.ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            spatial.distance = (spatial.distance * (1.0 - scroll * 0.002)).clamp(0.01, 10000.0);
        }
    }
    if response.dragged() {
        if response.dragged_by(egui::PointerButton::Primary) {
            spatial.yaw += response.drag_delta().x * 0.01;
            spatial.pitch = (spatial.pitch + response.drag_delta().y * 0.01).clamp(-1.5, 1.5);
        } else {
            spatial.pan += response.drag_delta();
        }
    }

    // Viewport background
    painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(18, 19, 22));

    let center_x = (spatial.aabb.0[0] + spatial.aabb.1[0]) * 0.5;
    let center_y = (spatial.aabb.0[1] + spatial.aabb.1[1]) * 0.5;
    let center_z = (spatial.aabb.0[2] + spatial.aabb.1[2]) * 0.5;

    let cy = spatial.yaw.cos();
    let sy = spatial.yaw.sin();
    let cp = spatial.pitch.cos();
    let sp = spatial.pitch.sin();
    let fov = frame_rect.height().max(100.0) * 1.2;
    let screen_center = frame_rect.center() + spatial.pan;

    // Draw grid floor
    let grid_size = ((spatial.aabb.1[0] - spatial.aabb.0[0]).max(spatial.aabb.1[2] - spatial.aabb.0[2]) * 0.8).max(1.0);
    let floor_y = spatial.aabb.0[1] - center_y;
    for g in -4..=4 {
        let coord = (g as f32) * (grid_size / 4.0);
        let project_pt = |gx: f32, gz: f32| -> Option<Pos2> {
            let x1 = cy * gx + sy * gz;
            let z1 = -sy * gx + cy * gz;
            let y2 = cp * floor_y - sp * z1;
            let z2 = sp * floor_y + cp * z1;
            let z_cam = z2 - spatial.distance;
            if z_cam >= -0.01 {
                return None;
            }
            let inv_z = 1.0 / -z_cam;
            Some(Pos2::new(screen_center.x + x1 * inv_z * fov, screen_center.y - y2 * inv_z * fov))
        };
        if let (Some(p1), Some(p2)) = (project_pt(coord, -grid_size), project_pt(coord, grid_size)) {
            painter.line_segment([p1, p2], Stroke::new(1.0, Color32::from_rgb(35, 38, 44)));
        }
        if let (Some(p1), Some(p2)) = (project_pt(-grid_size, coord), project_pt(grid_size, coord)) {
            painter.line_segment([p1, p2], Stroke::new(1.0, Color32::from_rgb(35, 38, 44)));
        }
    }

    // Project vertices
    let mut proj = Vec::with_capacity(spatial.vertices.len());
    let mut cam_z = Vec::with_capacity(spatial.vertices.len());
    for v in &spatial.vertices {
        let rx = v[0] - center_x;
        let ry = v[1] - center_y;
        let rz = v[2] - center_z;

        let x1 = cy * rx + sy * rz;
        let z1 = -sy * rx + cy * rz;
        let y2 = cp * ry - sp * z1;
        let z2 = sp * ry + cp * z1;
        let z_c = z2 - spatial.distance;
        cam_z.push(z_c);

        if z_c < -0.001 {
            let inv_z = 1.0 / -z_c;
            proj.push(Some(Pos2::new(screen_center.x + x1 * inv_z * fov, screen_center.y - y2 * inv_z * fov)));
        } else {
            proj.push(None);
        }
    }

    if spatial.wireframe {
        let wire_color = tokens.accent_primary.gamma_multiply(0.7);
        let stroke = Stroke::new(1.0, wire_color);
        for tri in spatial.faces.iter().take(8000) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            if let (Some(Some(p0)), Some(Some(p1)), Some(Some(p2))) = (proj.get(i0), proj.get(i1), proj.get(i2))
                && (frame_rect.contains(*p0) || frame_rect.contains(*p1) || frame_rect.contains(*p2))
            {
                painter.line_segment([*p0, *p1], stroke);
                painter.line_segment([*p1, *p2], stroke);
                painter.line_segment([*p2, *p0], stroke);
            }
        }
    } else {
        // Shaded mode: painter's algorithm
        struct ShadedFace {
            depth: f32,
            pts: [Pos2; 3],
            color: Color32,
        }
        let mut shaded_faces = Vec::new();
        let light_dir = [0.4f32, 0.8f32, 0.4f32];
        let light_len = (light_dir[0] * light_dir[0] + light_dir[1] * light_dir[1] + light_dir[2] * light_dir[2]).sqrt();
        let l = [light_dir[0] / light_len, light_dir[1] / light_len, light_dir[2] / light_len];

        for tri in spatial.faces.iter().take(6000) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            if let (Some(Some(p0)), Some(Some(p1)), Some(Some(p2))) = (proj.get(i0), proj.get(i1), proj.get(i2))
                && let (Some(z0), Some(z1), Some(z2)) = (cam_z.get(i0), cam_z.get(i1), cam_z.get(i2))
                && let (Some(v0), Some(v1), Some(v2)) = (spatial.vertices.get(i0), spatial.vertices.get(i1), spatial.vertices.get(i2))
            {
                let avg_z = (*z0 + *z1 + *z2) / 3.0;
                let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
                let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
                let nx = e1[1] * e2[2] - e1[2] * e2[1];
                let ny = e1[2] * e2[0] - e1[0] * e2[2];
                let nz = e1[0] * e2[1] - e1[1] * e2[0];
                let n_len = (nx * nx + ny * ny + nz * nz).sqrt();
                if n_len > 1e-6 {
                    let (nx, ny, nz) = (nx / n_len, ny / n_len, nz / n_len);
                    // rotate normal by yaw and pitch
                    let rx = cy * nx + sy * nz;
                    let rz1 = -sy * nx + cy * nz;
                    let ry = cp * ny - sp * rz1;
                    let rz = sp * ny + cp * rz1;
                    let diffuse = (rx * l[0] + ry * l[1] + rz * l[2]).max(0.0);
                    let brightness = 0.25 + 0.75 * diffuse;
                    let c = (180.0 * brightness) as u8;
                    let color = Color32::from_rgb(c, (c as f32 * 0.95) as u8, (c as f32 * 0.90) as u8);
                    shaded_faces.push(ShadedFace { depth: avg_z, pts: [*p0, *p1, *p2], color });
                }
            }
        }
        shaded_faces.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));
        for face in shaded_faces {
            painter.add(egui::epaint::Shape::convex_polygon(face.pts.to_vec(), face.color, Stroke::new(0.5, face.color)));
        }
    }

    // Viewport HUD tags
    painter.text(
        frame_rect.left_top() + Vec2::new(8.0, 8.0),
        egui::Align2::LEFT_TOP,
        format!("3D Viewport ({})", spatial.format_name),
        egui::FontId::proportional(11.0),
        tokens.text_primary,
    );
    painter.text(
        frame_rect.right_top() + Vec2::new(-8.0, 8.0),
        egui::Align2::RIGHT_TOP,
        if spatial.wireframe { "Wireframe" } else { "Shaded" },
        egui::FontId::proportional(10.0),
        tokens.text_dim,
    );
}

fn paint_output(painter: &egui::Painter, frame_rect: Rect, state: &PreviewState, tokens: &Tokens) {
    if state.output.texture.is_some() {
        paint_slot(painter, frame_rect, &state.output, None, tokens);
    } else if state.render.is_some() {
        painter.text(frame_rect.center(), egui::Align2::CENTER_CENTER, "Rendering preview…", egui::FontId::proportional(12.0), tokens.text_dim);
    } else {
        let msg = state.output.message.clone().unwrap_or_else(|| "Output has not been rendered yet".into());
        painter.text(frame_rect.center(), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(12.0), tokens.text_dim);
    }
}

fn paint_overlays(painter: &egui::Painter, frame: Rect, state: &PreviewState, tokens: &Tokens) {
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(150));
    if state.safe_margins {
        painter.rect_stroke(monitor_geom::safe_rect(frame, ACTION_SAFE), 0.0, stroke, StrokeKind::Inside);
        painter.rect_stroke(monitor_geom::safe_rect(frame, TITLE_SAFE), 0.0, stroke, StrokeKind::Inside);
    }
    if let Some((name, aspect)) = state.guide.and_then(|i| GUIDES.get(i)) {
        let guide = monitor_geom::aspect_guide(frame, *aspect);
        painter.rect_stroke(guide, 0.0, Stroke::new(1.0, tokens.accent_primary), StrokeKind::Inside);
        painter.text(guide.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, *name, egui::FontId::proportional(10.0), tokens.accent_primary);
    }
    let (lo, hi) = state.transport.range();
    if state.transport.in_point.is_some() || state.transport.out_point.is_some() {
        let total = state.transport.last_frame().max(1) as f32;
        let x0 = frame.min.x + frame.width() * (lo as f32 / total).clamp(0.0, 1.0);
        let x1 = frame.min.x + frame.width() * (hi as f32 / total).clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, frame.max.y - 5.0), Pos2::new(x1.max(x0 + 1.0), frame.max.y)),
            1.0,
            tokens.accent_primary,
        );
    }
}

fn view_tools(ui: &mut Ui, state: &mut PreviewState, job: Option<&CraftJobSpec>) {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("preview-zoom")
            .selected_text(match state.zoom {
                ZoomMode::Fit => "Fit".to_string(),
                ZoomMode::Actual => "100%".to_string(),
                ZoomMode::Scale(s) => format!("{:.0}%", s * 100.0),
            })
            .width(64.0)
            .show_ui(ui, |ui| {
                for (label, mode) in [("Fit", ZoomMode::Fit), ("100%", ZoomMode::Actual), ("200%", ZoomMode::Scale(2.0))] {
                    if ui.selectable_label(state.zoom == mode, label).clicked() {
                        state.zoom = mode;
                        state.pan = Vec2::ZERO;
                    }
                }
            });
        ui.checkbox(&mut state.safe_margins, "Safe margins");
        egui::ComboBox::from_id_salt("preview-guide")
            .selected_text(state.guide.and_then(|i| GUIDES.get(i)).map(|(n, _)| *n).unwrap_or("No guide"))
            .width(70.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.guide, None, "No guide");
                for (i, (name, _)) in GUIDES.iter().enumerate() {
                    ui.selectable_value(&mut state.guide, Some(i), *name);
                }
            });
        egui::ComboBox::from_id_salt("preview-scope").selected_text(state.scope.label()).width(100.0).show_ui(ui, |ui| {
            for mode in ScopeMode::ALL {
                ui.selectable_value(&mut state.scope, mode, mode.label());
            }
        });
        if state.mode != PreviewMode::Source
            && let Some(job) = job
        {
            let busy = state.render.is_some();
            if ui.add_enabled(!busy, egui::Button::new(if busy { "Rendering…" } else { "Render output frame" })).clicked() {
                state.start_render(job);
            }
        }
        if let Some(ref mut universal) = state.universal_media {
            match universal {
                UniversalMediaState::Document(doc) => {
                    if ui.button("◀ Prev Page").on_hover_text("Previous Page (PageUp)").clicked() {
                        doc.prev_page();
                    }
                    ui.label(RichText::new(format!("Page {} of {}", doc.current_page + 1, doc.total_pages)).small().strong());
                    if ui.button("Next Page ▶").on_hover_text("Next Page (PageDown)").clicked() {
                        doc.next_page();
                    }
                }
                UniversalMediaState::Spatial3D(spatial) => {
                    let label = if spatial.wireframe { "Mode: Wireframe" } else { "Mode: Shaded" };
                    if ui.button(label).on_hover_text("Toggle Wireframe / Shaded View").clicked() {
                        spatial.wireframe = !spatial.wireframe;
                    }
                    if ui.button("Reset Orbit").on_hover_text("Reset 3D Viewport Camera").clicked() {
                        spatial.reset_camera();
                    }
                    ui.label(RichText::new(format!("{} verts · {} tris", spatial.vertex_count, spatial.face_count)).small());
                }
                UniversalMediaState::Audio(audio) => {
                    ui.label(RichText::new(format!("Waveform · {}Hz · {} ch", audio.sample_rate, audio.channels)).small());
                }
                UniversalMediaState::Vector(vec) => {
                    ui.label(RichText::new(format!("Vector · {:.0}x{:.0} pt", vec.width, vec.height)).small());
                }
            }
        }
    });
}

fn transport_bar(ui: &mut Ui, state: &mut PreviewState, tokens: &Tokens) {
    let fps = state.transport.fps();
    let now = Instant::now();
    ui.horizontal(|ui| {
        let tc_cur = timecode(state.transport.position(), fps);
        let tc_total = timecode(state.transport.last_frame(), fps);
        ui.monospace(RichText::new(format!("{tc_cur} / {tc_total}")).size(12.0).color(tokens.accent_primary));
        let mut pos = state.transport.position();
        let last = state.transport.last_frame();
        if ui.add(egui::Slider::new(&mut pos, 0..=last).show_value(false)).changed() {
            state.transport.seek(pos);
        }
    });
    ui.horizontal(|ui| {
        if icons::button(ui, icons::Icon::StepBack, 22.0, false, tokens, "Previous Frame (Left Arrow)").clicked() {
            state.transport.step(-1);
        }
        let play_icon = if state.transport.is_playing() { icons::Icon::Pause } else { icons::Icon::Play };
        if icons::button(ui, play_icon, 22.0, state.transport.is_playing(), tokens, "Play / Pause (Space); J / K / L shuttle").clicked() {
            state.transport.toggle_play(now);
        }
        if icons::button(ui, icons::Icon::StepFwd, 22.0, false, tokens, "Next Frame (Right Arrow)").clicked() {
            state.transport.step(1);
        }
        let rate = state.transport.rate();
        if rate.abs() > 1.0 || rate < 0.0 {
            ui.label(RichText::new(format!("{rate:+.0}x")).small().color(tokens.text_dim));
        }
        ui.separator();
        if ui.button("[ Set In").on_hover_text("Mark In point (I)").clicked() {
            state.transport.set_in();
        }
        if ui.button("] Set Out").on_hover_text("Mark Out point (O)").clicked() {
            state.transport.set_out();
        }
        if ui.button("Clear Range").on_hover_text("Reset In/Out range").clicked() {
            state.transport.clear_range();
        }
        ui.checkbox(&mut state.transport.loop_range, "Loop");
        if let Some(inp) = state.transport.in_point {
            ui.label(RichText::new(format!("In: {inp}")).small().color(tokens.text_dim));
        }
        if let Some(outp) = state.transport.out_point {
            ui.label(RichText::new(format!("Out: {outp}")).small().color(tokens.text_dim));
        }
        ui.separator();
        let mute_icon = if state.transport.muted { icons::Icon::Mute } else { icons::Icon::Speaker };
        let mute_tooltip = if state.transport.muted { "Unmute Audio" } else { "Mute Audio" };
        if icons::button(ui, mute_icon, 20.0, state.transport.muted, tokens, mute_tooltip).clicked() {
            state.transport.toggle_mute();
        }
        let mut vol_display = if state.transport.muted { 0.0 } else { state.transport.volume };
        if ui.add_sized([60.0, 18.0], egui::Slider::new(&mut vol_display, 0.0..=1.0).show_value(false)).changed() {
            state.transport.set_volume(vol_display);
            if state.transport.muted && vol_display > 0.0 {
                state.transport.muted = false;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, JobPriority, OutputTarget, StartPolicy};
    use encodercraft_engine::QueueManager;
    use encodercraft_presets::PresetRegistry;
    use std::sync::{Arc, Mutex};

    #[test]
    fn selected_still_job_shows_a_decoded_texture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bmp");
        let mut bmp = Vec::new();
        let (w, h) = (8u32, 8u32);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + w * h * 3).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&w.to_le_bytes());
        bmp.extend_from_slice(&h.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&(w * h * 3).to_le_bytes());
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend((0..w * h * 3).map(|i| (i * 5) as u8));
        std::fs::write(&path, bmp).unwrap();

        let job = CraftJobSpec {
            version: "1.0".into(),
            job_id: "p".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "0".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::Paused,
            source: SourceDescriptor::File { path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget { output_path: dir.path().join("missing.mp4"), ..Default::default() }],
        };
        let tokens = Tokens::dark();
        let mut state = PreviewState::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while state.source.texture.is_none() && std::time::Instant::now() < deadline {
            egui::__run_test_ui(|ui| render_preview_panel(ui, &mut state, Some(&job), false, &tokens));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.source.texture.is_some(), "no frame was decoded");
        assert_eq!(state.transport.last_frame(), 0);
        assert!(state.output.message.is_some(), "missing output must be explained");
    }

    #[test]
    fn headless_gui_adds_transcodes_and_previews_a_real_still() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.bmp");
        let mut bmp = Vec::new();
        let (width, height) = (8u32, 8u32);
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + width * height * 3).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&width.to_le_bytes());
        bmp.extend_from_slice(&height.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&(width * height * 3).to_le_bytes());
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend((0..width * height * 3).map(|index| (index * 5) as u8));
        std::fs::write(&source, bmp).unwrap();

        let queue = Arc::new(QueueManager::new_in_memory());
        let mut app = crate::app::EncoderCraftApp::new(queue.clone(), Arc::new(Mutex::new(PresetRegistry::default())));
        assert!(app.workspaces.activate(crate::workspace::PREVIEW_WORKSPACE));
        app.add_file_job(source);
        let queued = queue.list_jobs();
        assert_eq!(queued.len(), 1);
        app.queue_state.select_single(queued[0].spec.job_id.clone(), 0);
        app.run_command(crate::commands::AppCommand::StartQueue);

        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        while Instant::now() < deadline {
            crate::run_test_ui(|ui| app.render(ui));
            let completed = queue.list_jobs().iter().any(|job| job.status == encodercraft_core::JobStatus::Completed);
            let preview_ready = app.preview_state.source.texture.is_some();
            if completed && preview_ready {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let completed = queue.list_jobs().iter().any(|job| job.status == encodercraft_core::JobStatus::Completed);
        assert!(completed, "GUI start command did not finish the queued transcode");
        assert!(app.preview_state.source.texture.is_some(), "preview did not decode a real source frame");
        assert!(
            app.preview_state.source.pixels.as_ref().is_some_and(|(_, _, pixels)| pixels.iter().any(|pixel| *pixel != 0)),
            "preview frame contains only placeholder pixels"
        );
        assert!(dir.path().join("input_encoded.png").is_file(), "GUI queue did not produce its output");
    }

    #[test]
    fn test_universal_document_preview() {
        let dir = tempfile::tempdir().unwrap();
        let doc_path = dir.path().join("spec.md");
        std::fs::write(&doc_path, b"# Title\n\nPage one content\n\n## Section 2\nPage two content\n").unwrap();

        let job = CraftJobSpec {
            version: "1.0".into(),
            job_id: "doc-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::Paused,
            source: SourceDescriptor::File { path: doc_path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![],
        };

        let mut state = PreviewState::default();
        state.select(&job);

        assert!(state.universal_media.is_some());
        if let Some(UniversalMediaState::Document(mut doc)) = state.universal_media.clone() {
            assert_eq!(doc.total_pages, 1);
            assert_eq!(doc.current_page, 0);
            assert!(!doc.pages_text.is_empty());
            doc.next_page();
            assert_eq!(doc.current_page, 0);
        } else {
            panic!("expected Document universal media");
        }

        let tokens = Tokens::dark();
        egui::__run_test_ui(|ui| {
            render_preview_panel(ui, &mut state, Some(&job), false, &tokens);
        });
    }

    #[test]
    fn test_universal_vector_preview() {
        let dir = tempfile::tempdir().unwrap();
        let svg_path = dir.path().join("drawing.svg");
        std::fs::write(
            &svg_path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><path d="M 10 10 L 50 50" fill="#ff0000"/></svg>"##,
        )
        .unwrap();

        let job = CraftJobSpec {
            version: "1.0".into(),
            job_id: "vec-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::Paused,
            source: SourceDescriptor::File { path: svg_path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![],
        };

        let mut state = PreviewState::default();
        state.select(&job);

        assert!(state.universal_media.is_some());
        if let Some(UniversalMediaState::Vector(vec)) = &state.universal_media {
            assert_eq!(vec.format_name, "SVG");
            assert!(vec.width > 0.0);
            assert!(vec.height > 0.0);
        } else {
            panic!("expected Vector universal media");
        }

        let tokens = Tokens::dark();
        egui::__run_test_ui(|ui| {
            render_preview_panel(ui, &mut state, Some(&job), false, &tokens);
        });
    }

    #[test]
    fn test_universal_3d_spatial_preview() {
        let dir = tempfile::tempdir().unwrap();
        let stl_path = dir.path().join("cube.stl");
        let mut mesh = encodercraft_spatial_io::TriangleMesh::new("Cube");
        mesh.vertices = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 10.0, 0.0]];
        mesh.indices = vec![[0, 1, 2]];
        let stl_bytes = encodercraft_spatial_io::stl::write_stl_binary(&mesh);
        std::fs::write(&stl_path, stl_bytes).unwrap();

        let job = CraftJobSpec {
            version: "1.0".into(),
            job_id: "3d-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::Paused,
            source: SourceDescriptor::File { path: stl_path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![],
        };

        let mut state = PreviewState::default();
        state.select(&job);

        assert!(state.universal_media.is_some());
        if let Some(UniversalMediaState::Spatial3D(spatial)) = &mut state.universal_media {
            assert_eq!(spatial.format_name, "STL");
            assert_eq!(spatial.vertex_count, 3);
            assert_eq!(spatial.face_count, 1);
            assert!(!spatial.wireframe);
            spatial.wireframe = true;
            assert!(spatial.wireframe);
            spatial.reset_camera();
            assert_eq!(spatial.pan, Vec2::ZERO);
        } else {
            panic!("expected Spatial3D universal media");
        }

        let tokens = Tokens::dark();
        egui::__run_test_ui(|ui| {
            render_preview_panel(ui, &mut state, Some(&job), false, &tokens);
        });
    }

    #[test]
    fn test_universal_audio_waveform_preview() {
        let dir = tempfile::tempdir().unwrap();
        let flac_path = dir.path().join("sound.flac");
        let sample_rate = 44100;
        let samples = vec![0.0f32, 0.5, 0.8, -0.8, -0.5, 0.0];
        let buf = encodercraft_core::AudioSampleBuffer::from_planar_channels(sample_rate, encodercraft_core::ChannelLayout::Mono, 0, vec![samples])
            .unwrap();
        let encoded_flac = encodercraft_audio_dsp::io::encode_audio_flac(&buf).unwrap();
        std::fs::write(&flac_path, encoded_flac).unwrap();

        let job = CraftJobSpec {
            version: "1.0".into(),
            job_id: "audio-job".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::Paused,
            source: SourceDescriptor::File { path: flac_path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![],
        };

        let mut state = PreviewState::default();
        state.select(&job);

        assert!(state.universal_media.is_some());
        if let Some(UniversalMediaState::Audio(audio)) = &state.universal_media {
            assert_eq!(audio.sample_rate, 44100);
            assert_eq!(audio.channels, 1);
            assert!(!audio.waveform_peaks.is_empty());
        } else {
            panic!("expected Audio universal media");
        }

        let tokens = Tokens::dark();
        egui::__run_test_ui(|ui| {
            render_preview_panel(ui, &mut state, Some(&job), false, &tokens);
        });
    }

    #[test]
    fn transport_audio_controls_and_preview_integration() {
        let mut state = PreviewState::default();
        assert_eq!(state.transport.volume, 1.0);
        assert!(!state.transport.muted);
        state.transport.toggle_mute();
        assert!(state.transport.muted);
        let tokens = Tokens::dark();
        egui::__run_test_ui(|ui| {
            transport_bar(ui, &mut state, &tokens);
        });
    }
}
