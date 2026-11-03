//! Preview monitor: interactive frame canvas, In/Out point trimming, scrub bar, and transport controls.

use egui::{Color32, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;

use encodercraft_core::{CraftJobSpec, SourceDescriptor};

use crate::icons;
use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::theme::Tokens;

/// Preview display modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Source,
    Output,
    SideBySideSplit,
}

/// One decoded view (source or output) with its GPU texture.
#[derive(Default)]
struct Slot {
    decoder: Option<PreviewDecoder>,
    texture: Option<egui::TextureHandle>,
    shown_frame: Option<u64>,
    requested_frame: Option<u64>,
    frame_count: u64,
    fps: f64,
    aspect: Option<f32>,
    summary: Option<String>,
    message: Option<String>,
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
                        self.aspect = Some(v.width as f32 / v.height.max(1) as f32);
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
    pub current_frame: u64,
    pub total_frames: u64,
    pub in_point: Option<u64>,
    pub out_point: Option<u64>,
    pub is_playing: bool,
    pub aspect_ratio: f32,
    pub split_fraction: f32,
    /// Frame rate of the source, used for timecode and playback timing.
    pub fps: f64,
    source: Slot,
    output: Slot,
    play_clock: Option<(std::time::Instant, u64)>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            active_job_id: None,
            mode: PreviewMode::Source,
            current_frame: 0,
            total_frames: 0,
            in_point: None,
            out_point: None,
            is_playing: false,
            aspect_ratio: 16.0 / 9.0,
            split_fraction: 0.5,
            fps: 30.0,
            source: Slot::default(),
            output: Slot::default(),
            play_clock: None,
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

/// Renders the preview monitor pane.
pub fn render_preview_panel(ui: &mut Ui, state: &mut PreviewState, job: Option<&CraftJobSpec>, tokens: &Tokens) {
    // Re-open the decoders when the selection changes.
    if let Some(j) = job {
        if state.active_job_id.as_deref() != Some(j.job_id.as_str()) {
            state.active_job_id = Some(j.job_id.clone());
            state.current_frame = 0;
            state.total_frames = 0;
            state.is_playing = false;
            state.play_clock = None;
            let missing = match &j.source {
                SourceDescriptor::File { .. } => "Source file is offline",
                _ => "Preview is not available for this source type",
            };
            state.source.open(source_path(j), missing);
            state.output.open(j.targets.first().map(|t| t.output_path.clone()), "Output has not been rendered yet");
            if let SourceDescriptor::File { start_tick, duration_ticks, .. } = &j.source {
                state.in_point = start_tick.and_then(|s| u64::try_from(s).ok());
                state.out_point = match (state.in_point, duration_ticks) {
                    (Some(s), Some(d)) if *d > 0 => Some(s.saturating_add((*d as u64).saturating_sub(1))),
                    (None, Some(d)) if *d > 0 => Some((*d as u64).saturating_sub(1)),
                    _ => None,
                };
            } else {
                state.in_point = None;
                state.out_point = None;
            }
        }
        // A render finished after selection: pick up the new output file.
        if state.output.decoder.is_none()
            && let Some(out) = j.targets.first().map(|t| t.output_path.clone())
            && out.is_file()
            && std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false)
        {
            state.output.open(Some(out), "Output has not been rendered yet");
        }
    } else if state.active_job_id.take().is_some() {
        state.source = Slot::default();
        state.output = Slot::default();
        state.source.message = Some("No Active Clip Selected".into());
        state.total_frames = 0;
        state.current_frame = 0;
        state.is_playing = false;
    }

    let ctx = ui.ctx().clone();
    state.source.pump(&ctx, "source", state.current_frame);
    state.output.pump(&ctx, "output", state.current_frame);
    if state.source.frame_count > 0 {
        state.total_frames = state.source.frame_count - 1;
    }
    if state.source.fps > 0.0 {
        state.fps = state.source.fps;
    }
    if let Some(a) = state.source.aspect {
        state.aspect_ratio = a;
    }

    if state.is_playing && state.total_frames > 0 {
        let (start, origin) = *state.play_clock.get_or_insert((std::time::Instant::now(), state.current_frame));
        let advanced = (start.elapsed().as_secs_f64() * state.fps) as u64;
        let span = state.total_frames + 1;
        state.current_frame = (origin + advanced) % span;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    } else {
        state.play_clock = None;
    }
    if state.source.decoder.is_some() || state.output.decoder.is_some() {
        // Keep polling while a decoder is still working.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Preview Monitor").size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut state.mode, PreviewMode::SideBySideSplit, "A/B Split");
                ui.selectable_value(&mut state.mode, PreviewMode::Output, "Output");
                ui.selectable_value(&mut state.mode, PreviewMode::Source, "Source");
            });
        });
        ui.add_space(4.0);

        let available_size = ui.available_size();
        let canvas_height = (available_size.x / state.aspect_ratio.max(0.1)).clamp(120.0, 260.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(available_size.x, canvas_height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, tokens.bg_panel_darker);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, tokens.border_subtle), StrokeKind::Inside);

        let aspect = state.aspect_ratio.max(0.1);
        let frame_width = (rect.height() * aspect).min(rect.width() - 8.0);
        let frame_height = frame_width / aspect;
        let frame_rect = Rect::from_center_size(rect.center(), Vec2::new(frame_width, frame_height));
        painter.rect_filled(frame_rect, 2.0, Color32::BLACK);

        match state.mode {
            PreviewMode::Source => paint_slot(&painter, frame_rect, &state.source, None, tokens),
            PreviewMode::Output => paint_slot(&painter, frame_rect, &state.output, None, tokens),
            PreviewMode::SideBySideSplit => {
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    state.split_fraction = ((pos.x - frame_rect.min.x) / frame_rect.width().max(1.0)).clamp(0.0, 1.0);
                }
                let split_x = frame_rect.min.x + frame_rect.width() * state.split_fraction;
                let left = Rect::from_min_max(frame_rect.min, Pos2::new(split_x, frame_rect.max.y));
                let right = Rect::from_min_max(Pos2::new(split_x, frame_rect.min.y), frame_rect.max);
                paint_slot(&painter, frame_rect, &state.source, Some(left), tokens);
                paint_slot(&painter, frame_rect, &state.output, Some(right), tokens);
                painter.line_segment(
                    [Pos2::new(split_x, frame_rect.min.y), Pos2::new(split_x, frame_rect.max.y)],
                    Stroke::new(1.5, tokens.status_processing),
                );
            }
        }

        if state.in_point.is_some() || state.out_point.is_some() {
            let total = state.total_frames.max(1) as f32;
            let in_frac = (state.in_point.unwrap_or(0) as f32 / total).clamp(0.0, 1.0);
            let out_frac = (state.out_point.unwrap_or(state.total_frames) as f32 / total).clamp(in_frac, 1.0);
            let rx0 = frame_rect.min.x + frame_rect.width() * in_frac;
            let rx1 = frame_rect.min.x + frame_rect.width() * out_frac;
            let strip_rect = Rect::from_min_max(Pos2::new(rx0, frame_rect.max.y - 5.0), Pos2::new(rx1, frame_rect.max.y));
            painter.rect_filled(strip_rect, 1.0, tokens.accent_primary);
        }
        painter.rect_stroke(frame_rect, 2.0, Stroke::new(1.0, tokens.border_strong), StrokeKind::Inside);

        let caption = match (&state.mode, job) {
            (_, None) => "No Active Clip Selected".to_string(),
            (PreviewMode::Output, Some(_)) => state.output.path().map(|p| p.display().to_string()).unwrap_or_else(|| "Output not rendered".into()),
            (_, Some(_)) => state.source.summary.clone().unwrap_or_default(),
        };
        ui.label(RichText::new(caption).small().color(tokens.text_dim));

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let fps = state.fps.round().max(1.0) as u64;
            let tc_cur = frame_to_timecode(state.current_frame, fps);
            let tc_total = frame_to_timecode(state.total_frames, fps);
            ui.monospace(RichText::new(format!("{tc_cur} / {tc_total}")).size(12.0).color(tokens.accent_primary));
            let before = state.current_frame;
            ui.add(egui::Slider::new(&mut state.current_frame, 0..=state.total_frames).show_value(false));
            if state.current_frame != before {
                state.play_clock = None;
            }
        });

        // --- 4. Transport & In/Out Controls ---
        ui.horizontal(|ui| {
            // Step Back
            if icons::button(ui, icons::Icon::StepBack, 22.0, false, tokens, "Previous Frame (Left Arrow)").clicked() {
                state.current_frame = state.current_frame.saturating_sub(1);
                state.play_clock = None;
            }

            // Play / Pause
            let play_icon = if state.is_playing { icons::Icon::Pause } else { icons::Icon::Play };
            if icons::button(ui, play_icon, 22.0, state.is_playing, tokens, "Play / Pause (Space)").clicked() {
                state.is_playing = !state.is_playing;
                state.play_clock = None;
            }

            // Step Forward
            if icons::button(ui, icons::Icon::StepFwd, 22.0, false, tokens, "Next Frame (Right Arrow)").clicked()
                && state.current_frame < state.total_frames
            {
                state.current_frame += 1;
                state.play_clock = None;
            }

            ui.separator();

            // Set In
            if ui.button("[ Set In").on_hover_text("Mark In point").clicked() {
                state.in_point = Some(state.current_frame);
            }

            // Set Out
            if ui.button("] Set Out").on_hover_text("Mark Out point").clicked() {
                state.out_point = Some(state.current_frame);
            }

            // Clear In/Out
            if ui.button("Clear Range").on_hover_text("Reset In/Out range").clicked() {
                state.in_point = None;
                state.out_point = None;
            }

            if let Some(inp) = state.in_point {
                ui.label(RichText::new(format!("In: {inp}")).small().color(tokens.text_dim));
            }
            if let Some(outp) = state.out_point {
                ui.label(RichText::new(format!("Out: {outp}")).small().color(tokens.text_dim));
            }
        });
    });
}

fn frame_to_timecode(frame: u64, fps: u64) -> String {
    let fps = fps.max(1);
    let total_secs = frame / fps;
    let frames = frame % fps;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}:{frames:02}")
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
            egui::__run_test_ui(|ui| render_preview_panel(ui, &mut state, Some(&job), &tokens));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.source.texture.is_some(), "no frame was decoded");
        assert_eq!(state.total_frames, 0);
        assert!(state.output.message.is_some(), "missing output must be explained");
    }
}
