//! Encoding panel: what is encoding right now. Telemetry is derived from the queue's live job
//! state; remaining time uses a smoothed progress rate so it does not jump.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};

use encodercraft_core::{JobStatus, SourceDescriptor};
use encodercraft_engine::{QueueManager, QueuedJob};

use crate::preview_decoder::{PreviewDecoder, PreviewEvent};
use crate::theme::Tokens;

/// Weight of the newest rate sample in the smoothed rate.
const SMOOTHING: f64 = 0.2;
/// Samples closer together than this are ignored.
const MIN_SAMPLE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq)]
pub struct JobTelemetry {
    pub job_id: String,
    pub source_name: String,
    pub preset: String,
    pub percent: f64,
    pub fps: f64,
    pub elapsed: Duration,
    pub remaining: Option<Duration>,
    pub output_bytes: u64,
    pub settings: String,
    pub paused: bool,
}

struct Track {
    started: Instant,
    last: (Instant, f64),
    /// Smoothed progress, in percent per second.
    rate: Option<f64>,
}

#[derive(Default)]
pub struct EncodingTelemetry {
    tracks: HashMap<String, Track>,
}

impl EncodingTelemetry {
    /// Samples every job that is encoding or paused; tracks of other jobs are dropped.
    pub fn observe(&mut self, now: Instant, jobs: &[QueuedJob]) -> Vec<JobTelemetry> {
        let live: Vec<&QueuedJob> = jobs.iter().filter(|j| matches!(j.status, JobStatus::Processing | JobStatus::Paused)).collect();
        self.tracks.retain(|id, _| live.iter().any(|j| &j.spec.job_id == id));

        live.into_iter()
            .map(|job| {
                let pct = job.progress_percent.clamp(0.0, 100.0);
                let track = self.tracks.entry(job.spec.job_id.clone()).or_insert(Track { started: now, last: (now, pct), rate: None });
                let dt = now.saturating_duration_since(track.last.0);
                if dt >= MIN_SAMPLE {
                    let sample = (pct - track.last.1).max(0.0) / dt.as_secs_f64();
                    if matches!(job.status, JobStatus::Processing) {
                        track.rate = Some(match track.rate {
                            Some(r) => r + SMOOTHING * (sample - r),
                            None => sample,
                        });
                    }
                    track.last = (now, pct);
                }
                let remaining = match track.rate {
                    Some(rate) if rate > 1e-6 && pct < 100.0 => Some(Duration::from_secs_f64(((100.0 - pct) / rate).min(86_400.0 * 30.0))),
                    _ => None,
                };
                let target = job.spec.targets.first();
                JobTelemetry {
                    job_id: job.spec.job_id.clone(),
                    source_name: source_name(&job.spec.source),
                    preset: target.map(|t| t.preset_name.clone()).unwrap_or_default(),
                    percent: pct,
                    fps: job.current_fps,
                    elapsed: now.saturating_duration_since(track.started),
                    remaining,
                    output_bytes: target.and_then(|t| std::fs::metadata(&t.output_path).ok()).map(|m| m.len()).unwrap_or(0),
                    settings: target.map(settings_summary).unwrap_or_default(),
                    paused: matches!(job.status, JobStatus::Paused),
                }
            })
            .collect()
    }
}

fn source_name(source: &SourceDescriptor) -> String {
    match source {
        SourceDescriptor::File { path, .. } => {
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
        }
        SourceDescriptor::StitchedClips { .. } => "Stitched Sequence".into(),
        SourceDescriptor::Pipe { endpoint, .. } => endpoint.clone(),
        SourceDescriptor::ProjectReference { composition_id, .. } => composition_id.clone(),
    }
}

/// One line describing what the target encodes to.
pub fn settings_summary(t: &encodercraft_core::OutputTarget) -> String {
    let mut parts = vec![format!("{:?}", t.container)];
    if let (Some(w), Some(h)) = (t.custom_width, t.custom_height) {
        parts.push(format!("{w}x{h}"));
    }
    if let Some(r) = t.custom_frame_rate
        && r.den != 0
    {
        parts.push(format!("{:.3} fps", r.num as f64 / r.den as f64));
    }
    if let Some(kbps) = t.target_bitrate_kbps {
        parts.push(format!("{:.1} Mbps", f64::from(kbps) / 1000.0));
    }
    parts.join(" · ")
}

pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

/// A live thumbnail of the source at the encoder's current position.
struct Thumb {
    decoder: PreviewDecoder,
    frame_count: u64,
    requested: Option<u64>,
    texture: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub struct EncodingPanelState {
    telemetry: EncodingTelemetry,
    thumbs: HashMap<String, Thumb>,
    last_thumb: Option<Instant>,
}

/// Renders the panel. Returns the id of a job whose Stop button was pressed.
pub fn render_encoding_panel(ui: &mut Ui, state: &mut EncodingPanelState, queue: &QueueManager, tokens: &Tokens) -> Option<String> {
    let jobs = queue.list_jobs();
    let now = Instant::now();
    let rows = state.telemetry.observe(now, &jobs);
    let mut stop = None;

    ui.heading(RichText::new("Encoding").size(15.0));
    ui.add_space(4.0);
    if rows.is_empty() {
        state.thumbs.clear();
        ui.label(RichText::new("Not currently encoding.").color(tokens.text_dim));
        return None;
    }
    ui.ctx().request_repaint_after(Duration::from_millis(250));
    state.thumbs.retain(|id, _| rows.iter().any(|r| &r.job_id == id));
    let refresh = state.last_thumb.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_millis(500));
    if refresh {
        state.last_thumb = Some(now);
    }

    for row in &rows {
        let job = jobs.iter().find(|j| j.spec.job_id == row.job_id);
        if let Some(job) = job {
            update_thumb(ui.ctx(), state, job, row.percent, refresh);
        }
        egui::Frame::new().fill(tokens.bg_panel_darker).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(tex) = state.thumbs.get(&row.job_id).and_then(|t| t.texture.as_ref()) {
                    let size = tex.size_vec2();
                    let height = 54.0;
                    ui.image((tex.id(), egui::vec2(height * size.x / size.y.max(1.0), height)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&row.source_name).strong());
                    ui.label(RichText::new(&row.preset).small().color(tokens.text_secondary));
                    ui.label(RichText::new(&row.settings).small().color(tokens.text_dim));
                });
            });
            ui.add(egui::ProgressBar::new((row.percent / 100.0) as f32).show_percentage());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Elapsed {}", format_duration(row.elapsed))).small());
                let remaining = row.remaining.map(format_duration).unwrap_or_else(|| "--:--:--".into());
                ui.label(RichText::new(format!("Remaining {remaining}")).small());
                ui.label(RichText::new(format!("{:.1} fps", row.fps)).small());
                ui.label(RichText::new(format_bytes(row.output_bytes)).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Stop").clicked() {
                        stop = Some(row.job_id.clone());
                    }
                    if row.paused {
                        ui.label(RichText::new("Paused").small().color(tokens.status_queued));
                    }
                });
            });
        });
        ui.add_space(4.0);
    }
    stop
}

fn update_thumb(ctx: &egui::Context, state: &mut EncodingPanelState, job: &QueuedJob, percent: f64, refresh: bool) {
    let SourceDescriptor::File { path, .. } = &job.spec.source else { return };
    let thumb = state.thumbs.entry(job.spec.job_id.clone()).or_insert_with(|| Thumb {
        decoder: PreviewDecoder::open(path.clone()),
        frame_count: 0,
        requested: None,
        texture: None,
    });
    while let Some(ev) = thumb.decoder.poll() {
        match ev {
            PreviewEvent::Opened { frame_count, .. } => thumb.frame_count = frame_count,
            PreviewEvent::Frame { width, height, rgba, .. } if width > 0 && height > 0 => {
                let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                match &mut thumb.texture {
                    Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                    None => thumb.texture = Some(ctx.load_texture(format!("encoding-{}", job.spec.job_id), image, egui::TextureOptions::LINEAR)),
                }
            }
            _ => {}
        }
    }
    if refresh && thumb.frame_count > 0 {
        let wanted = ((percent / 100.0) * (thumb.frame_count - 1) as f64) as u64;
        if thumb.requested != Some(wanted) {
            thumb.decoder.request_frame(wanted);
            thumb.requested = Some(wanted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, CraftJobSpec, JobPriority, OutputTarget, StartPolicy};

    fn job(id: &str, status: JobStatus, pct: f64) -> QueuedJob {
        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: id.into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "1".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: "/media/clip.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                preset_name: "YouTube 4K".into(),
                custom_width: Some(3840),
                custom_height: Some(2160),
                target_bitrate_kbps: Some(35_000),
                ..Default::default()
            }],
        };
        let mut j = QueuedJob::new(spec);
        j.status = status;
        j.progress_percent = pct;
        j
    }

    #[test]
    fn only_running_jobs_are_reported() {
        let mut t = EncodingTelemetry::default();
        let rows = t.observe(
            Instant::now(),
            &[job("a", JobStatus::Queued, 0.0), job("b", JobStatus::Processing, 5.0), job("c", JobStatus::Completed, 100.0)],
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_name, "clip.mov");
        assert_eq!(rows[0].settings, "IsoBmffMp4 · 3840x2160 · 35.0 Mbps");
        assert_eq!(rows[0].remaining, None);
    }

    #[test]
    fn remaining_time_is_smoothed_and_shrinks() {
        let mut t = EncodingTelemetry::default();
        let t0 = Instant::now();
        t.observe(t0, &[job("a", JobStatus::Processing, 0.0)]);
        // Steady 10 % per second.
        let first = t.observe(t0 + Duration::from_secs(1), &[job("a", JobStatus::Processing, 10.0)]);
        assert_eq!(first[0].remaining, Some(Duration::from_secs(9)));
        // One stalled sample only nudges the estimate instead of resetting it.
        let stalled = t.observe(t0 + Duration::from_secs(2), &[job("a", JobStatus::Processing, 10.0)]);
        let est = stalled[0].remaining.unwrap();
        assert!(est > Duration::from_secs(9) && est < Duration::from_secs(60), "{est:?}");
        let later = t.observe(t0 + Duration::from_secs(12), &[job("a", JobStatus::Processing, 100.0)]);
        assert_eq!(later[0].remaining, None);
        assert_eq!(later[0].elapsed, Duration::from_secs(12));
    }

    #[test]
    fn finished_jobs_drop_their_history() {
        let mut t = EncodingTelemetry::default();
        let t0 = Instant::now();
        t.observe(t0, &[job("a", JobStatus::Processing, 50.0)]);
        t.observe(t0 + Duration::from_secs(5), &[job("a", JobStatus::Completed, 100.0)]);
        let again = t.observe(t0 + Duration::from_secs(9), &[job("a", JobStatus::Processing, 0.0)]);
        assert_eq!(again[0].elapsed, Duration::ZERO);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_duration(Duration::from_secs(3725)), "01:02:05");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
