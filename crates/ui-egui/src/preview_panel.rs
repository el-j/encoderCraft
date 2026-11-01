//! Preview monitor: interactive frame canvas, In/Out point trimming, scrub bar, and transport controls.

use egui::{Color32, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, Vec2};
use encodercraft_core::CraftJobSpec;

use crate::icons;
use crate::theme::Tokens;

/// Preview display modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewMode {
    #[default]
    Source,
    Output,
    SideBySideSplit,
}

/// State for the preview canvas and scrub controls.
#[derive(Debug, Clone)]
pub struct PreviewState {
    pub mode: PreviewMode,
    pub current_frame: u64,
    pub total_frames: u64,
    pub in_point: Option<u64>,
    pub out_point: Option<u64>,
    pub is_playing: bool,
    pub aspect_ratio: f32, // e.g. 16.0 / 9.0
    pub split_fraction: f32,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            mode: PreviewMode::Source,
            current_frame: 0,
            total_frames: 1800, // 60s at 30fps default placeholder
            in_point: None,
            out_point: None,
            is_playing: false,
            aspect_ratio: 16.0 / 9.0,
            split_fraction: 0.5,
        }
    }
}

/// Renders the preview monitor pane.
pub fn render_preview_panel(ui: &mut Ui, state: &mut PreviewState, job: Option<&CraftJobSpec>, tokens: &Tokens) {
    ui.vertical(|ui| {
        // --- 1. Header with Mode Selector ---
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Preview Monitor").size(15.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut state.mode, PreviewMode::SideBySideSplit, "A/B Split");
                ui.selectable_value(&mut state.mode, PreviewMode::Output, "Output");
                ui.selectable_value(&mut state.mode, PreviewMode::Source, "Source");
            });
        });

        ui.add_space(4.0);

        // --- 2. Interactive Video Frame Canvas ---
        let available_size = ui.available_size();
        let canvas_height = (available_size.x / state.aspect_ratio).clamp(120.0, 200.0);
        let canvas_size = Vec2::new(available_size.x, canvas_height);

        let (rect, _response) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Canvas Background & Safe Area Border
        painter.rect_filled(rect, 4.0, tokens.bg_panel_darker);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, tokens.border_subtle), StrokeKind::Inside);

        // Simulated Frame Display with aspect ratio box
        let frame_width = (rect.height() * state.aspect_ratio).min(rect.width() - 8.0);
        let frame_height = frame_width / state.aspect_ratio;
        let frame_x = rect.min.x + (rect.width() - frame_width) / 2.0;
        let frame_y = rect.min.y + (rect.height() - frame_height) / 2.0;
        let frame_rect = Rect::from_min_size(Pos2::new(frame_x, frame_y), Vec2::new(frame_width, frame_height));

        painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(16, 16, 18));
        painter.rect_stroke(frame_rect, 2.0, Stroke::new(1.0, tokens.border_strong), StrokeKind::Inside);

        // Title / Resolution text in canvas
        let (info_text, source_tag) = if let Some(j) = job {
            let label = match &j.source {
                encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("Media"),
                _ => "Stream",
            };
            (format!("{} · 3840x2160 · Rec.709", label), "Source: Ready")
        } else {
            ("No Active Clip Selected".to_string(), "Idle")
        };

        painter.text(
            Pos2::new(frame_rect.center().x, frame_rect.center().y - 10.0),
            egui::Align2::CENTER_CENTER,
            info_text,
            egui::FontId::proportional(13.0),
            tokens.text_dim,
        );

        painter.text(
            Pos2::new(frame_rect.center().x, frame_rect.center().y + 12.0),
            egui::Align2::CENTER_CENTER,
            match state.mode {
                PreviewMode::Source => format!("[{source_tag}]"),
                PreviewMode::Output => "[Target: Render Preview]".to_string(),
                PreviewMode::SideBySideSplit => "[A/B Comparison Split]".to_string(),
            },
            egui::FontId::proportional(11.0),
            tokens.accent_primary,
        );

        // Split line indicator if in SideBySideSplit mode
        if state.mode == PreviewMode::SideBySideSplit {
            let split_x = frame_rect.min.x + frame_rect.width() * state.split_fraction;
            painter.line_segment(
                [Pos2::new(split_x, frame_rect.min.y), Pos2::new(split_x, frame_rect.max.y)],
                Stroke::new(1.5, tokens.status_processing),
            );
        }

        ui.add_space(4.0);

        // --- 3. Timecode & Scrub Slider ---
        ui.horizontal(|ui| {
            // Current Timecode
            let tc_cur = frame_to_timecode(state.current_frame, 30);
            let tc_total = frame_to_timecode(state.total_frames, 30);
            ui.monospace(RichText::new(format!("{tc_cur} / {tc_total}")).size(12.0).color(tokens.accent_primary));

            ui.add(egui::Slider::new(&mut state.current_frame, 0..=state.total_frames).show_value(false));
        });

        // --- 4. Transport & In/Out Controls ---
        ui.horizontal(|ui| {
            // Step Back
            if ui.button(icons::STEP_BACK).on_hover_text("Previous Frame (Left Arrow)").clicked() {
                state.current_frame = state.current_frame.saturating_sub(1);
            }

            // Play / Pause
            let play_label = if state.is_playing { icons::PAUSE_QUEUE } else { icons::START_QUEUE };
            if ui.button(play_label).on_hover_text("Play / Pause (Space)").clicked() {
                state.is_playing = !state.is_playing;
            }

            // Step Forward
            if ui.button(icons::STEP_FWD).on_hover_text("Next Frame (Right Arrow)").clicked() && state.current_frame < state.total_frames {
                state.current_frame += 1;
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
    let total_secs = frame / fps;
    let frames = frame % fps;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}:{frames:02}")
}
