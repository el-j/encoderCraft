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
        }
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
        let missing = match &j.source {
            SourceDescriptor::File { .. } => "Source file is offline",
            _ => "Preview is not available for this source type",
        };
        self.source.open(source_path(j), missing);
        self.output.open(None, "Output has not been rendered yet");
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
            PreviewMode::Source => paint_slot(&clip, frame_rect, &state.source, None, tokens),
            PreviewMode::Output => paint_output(&clip, frame_rect, state, tokens),
            PreviewMode::SideBySideSplit => {
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    state.split_fraction = monitor_geom::split_fraction(pos.x, frame_rect);
                }
                let (left, right) = monitor_geom::split_rects(frame_rect, state.split_fraction);
                paint_slot(&clip, frame_rect, &state.source, Some(left.intersect(inner)), tokens);
                paint_slot(&clip, frame_rect, &state.output, Some(right.intersect(inner)), tokens);
                clip.line_segment(
                    [Pos2::new(left.max.x, frame_rect.min.y), Pos2::new(left.max.x, frame_rect.max.y)],
                    Stroke::new(1.5, tokens.status_processing),
                );
            }
        }
        if state.mode != PreviewMode::SideBySideSplit && response.dragged() && state.zoom != ZoomMode::Fit {
            state.pan += response.drag_delta();
        }
        if response.double_clicked() {
            state.zoom = ZoomMode::Fit;
            state.pan = Vec2::ZERO;
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, JobPriority, OutputTarget, StartPolicy};

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
}
