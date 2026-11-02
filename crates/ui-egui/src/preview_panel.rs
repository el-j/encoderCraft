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
    pub active_job_id: Option<String>,
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
            active_job_id: None,
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
    // Synchronize In/Out trim state when selection changes
    if let Some(j) = job {
        if state.active_job_id.as_deref() != Some(j.job_id.as_str()) {
            state.active_job_id = Some(j.job_id.clone());
            if let encodercraft_core::SourceDescriptor::File { start_tick, duration_ticks, .. } = &j.source {
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
    } else {
        state.active_job_id = None;
    }

    if state.is_playing {
        state.current_frame = (state.current_frame + 1) % (state.total_frames.max(1) + 1);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }

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

        // Frame Display with aspect ratio box
        let frame_width = (rect.height() * state.aspect_ratio).min(rect.width() - 8.0);
        let frame_height = frame_width / state.aspect_ratio;
        let frame_x = rect.min.x + (rect.width() - frame_width) / 2.0;
        let frame_y = rect.min.y + (rect.height() - frame_height) / 2.0;
        let frame_rect = Rect::from_min_size(Pos2::new(frame_x, frame_y), Vec2::new(frame_width, frame_height));

        painter.rect_filled(frame_rect, 2.0, Color32::from_rgb(16, 16, 18));

        // Render subtle SMPTE reference bars & active frame scanline when a job is selected
        if job.is_some() {
            let smpte_colors = [
                Color32::from_rgba_unmultiplied(192, 192, 192, 28),
                Color32::from_rgba_unmultiplied(192, 192, 0, 28),
                Color32::from_rgba_unmultiplied(0, 192, 192, 28),
                Color32::from_rgba_unmultiplied(0, 192, 0, 28),
                Color32::from_rgba_unmultiplied(192, 0, 192, 28),
                Color32::from_rgba_unmultiplied(192, 0, 0, 28),
                Color32::from_rgba_unmultiplied(0, 0, 192, 28),
            ];
            let bar_w = frame_rect.width() / smpte_colors.len() as f32;
            for (i, col) in smpte_colors.iter().enumerate() {
                let bx = frame_rect.min.x + (i as f32) * bar_w;
                let bar_rect = Rect::from_min_size(Pos2::new(bx, frame_rect.min.y), Vec2::new(bar_w, frame_rect.height()));
                painter.rect_filled(bar_rect, 0.0, *col);
            }

            // Animated playhead indicator inside frame
            let progress_frac = (state.current_frame as f32) / (state.total_frames.max(1) as f32);
            let scan_x = frame_rect.min.x + frame_rect.width() * progress_frac.clamp(0.0, 1.0);
            painter.line_segment(
                [Pos2::new(scan_x, frame_rect.min.y), Pos2::new(scan_x, frame_rect.max.y)],
                Stroke::new(1.0, tokens.accent_primary.gamma_multiply(0.45)),
            );
        }

        // Draw In/Out Trim Range Highlight Strip at bottom of frame_rect
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

        // Title / Resolution / Container text in canvas
        let (info_text, source_tag) = if let Some(j) = job {
            let label = match &j.source {
                encodercraft_core::SourceDescriptor::File { path, .. } => path.file_name().and_then(|s| s.to_str()).unwrap_or("Media"),
                encodercraft_core::SourceDescriptor::StitchedClips { clips } => {
                    if clips.is_empty() {
                        "Stitched Sequence"
                    } else {
                        "Stitched Multi-Clip Sequence"
                    }
                }
                encodercraft_core::SourceDescriptor::Pipe { endpoint, .. } => endpoint.as_str(),
                encodercraft_core::SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.as_str(),
            };
            let target_summary = if let Some(t) = j.targets.first() {
                let w = t.custom_width.unwrap_or(1920);
                let h = t.custom_height.unwrap_or(1080);
                format!("{label} · {w}x{h} · {:?}", t.container)
            } else {
                format!("{label} · Match Source")
            };
            (target_summary, "Source: Ready")
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
            if icons::button(ui, icons::Icon::StepBack, 22.0, false, tokens, "Previous Frame (Left Arrow)").clicked() {
                state.current_frame = state.current_frame.saturating_sub(1);
            }

            // Play / Pause
            let play_icon = if state.is_playing { icons::Icon::Pause } else { icons::Icon::Play };
            if icons::button(ui, play_icon, 22.0, state.is_playing, tokens, "Play / Pause (Space)").clicked() {
                state.is_playing = !state.is_playing;
            }

            // Step Forward
            if icons::button(ui, icons::Icon::StepFwd, 22.0, false, tokens, "Next Frame (Right Arrow)").clicked()
                && state.current_frame < state.total_frames
            {
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
