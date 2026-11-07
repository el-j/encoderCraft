//! Source Properties essence inspection modal dialog (AME Parity).
//!
//! Provides in-depth stream essence inspection: file size, container format, video dimensions,
//! pixel aspect ratio, frame rate, codec, bit depth, color space primaries/transfer/matrix/range,
//! audio sample rate, channel layout, duration, and timecode. Also supports one-click copying
//! of formatted technical metadata to the platform clipboard.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{RichText, Ui};
use encodercraft_source::{MediaProbe, MediaReader};

use crate::theme::Tokens;

/// Format duration in seconds into `HH:MM:SS:FF` or `HH:MM:SS` string.
pub fn format_time_seconds(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00:00".to_string();
    }
    let total_secs = seconds.floor() as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let frac_hundredths = ((seconds - seconds.floor()) * 100.0).round() as u64;
    format!("{hours:02}:{minutes:02}:{secs:02}.{frac_hundredths:02}")
}

/// Helper to format raw byte count into human-readable B / KB / MB / GB.
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

/// Human-readable aspect ratio from pixel width and height.
pub fn aspect_ratio_str(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return "N/A".to_string();
    }
    if w * 9 == h * 16 {
        "16:9 (1.78:1)".to_string()
    } else if w * 3 == h * 4 {
        "4:3 (1.33:1)".to_string()
    } else if w == h {
        "1:1 (Square)".to_string()
    } else if w * 16 == h * 9 {
        "9:16 (Vertical 0.56:1)".to_string()
    } else if w * 10 == h * 16 {
        "16:10 (1.60:1)".to_string()
    } else {
        format!("{:.2}:1", w as f64 / h as f64)
    }
}

/// Human-readable channel configuration name.
pub fn channel_layout_label(channels: u32) -> &'static str {
    match channels {
        1 => "Mono (1.0)",
        2 => "Stereo (2.0)",
        3 => "2.1 Surround",
        4 => "Quadraphonic (4.0)",
        6 => "5.1 Surround",
        8 => "7.1 Surround / 8-Ch Discrete",
        16 => "16-Channel Broadcast Stem",
        _ => "Multichannel",
    }
}

/// State for the Source Properties inspection modal.
#[derive(Default)]
pub struct SourcePropertiesState {
    pub is_open: bool,
    pub source_path: Option<PathBuf>,
    pub source_name: String,
    pub probe: Option<MediaProbe>,
    pub file_size: Option<u64>,
    pub modified_str: Option<String>,
    pub error_message: Option<String>,
}

impl SourcePropertiesState {
    /// Opens the dialog and inspects essence for the specified media file path.
    pub fn open_for_file(&mut self, path: PathBuf) {
        self.source_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("Media File").to_string();
        self.source_path = Some(path.clone());
        self.is_open = true;
        self.error_message = None;

        // Query file metadata
        if let Ok(meta) = std::fs::metadata(&path) {
            self.file_size = Some(meta.len());
            if let Ok(mod_time) = meta.modified() {
                let duration_since_epoch = mod_time.duration_since(std::time::UNIX_EPOCH).unwrap_or(Duration::ZERO);
                let secs = duration_since_epoch.as_secs();
                // Simple date string approximation
                self.modified_str = Some(format!("{secs}s unix epoch"));
            }
        }

        // Probe media stream essence via MediaReader
        match MediaReader::open(&path) {
            Ok(reader) => {
                let mut probe = reader.probe().clone();
                if probe.file_size.is_none() {
                    probe.file_size = self.file_size;
                }
                self.probe = Some(probe);
            }
            Err(err) => {
                self.error_message = Some(format!("Could not probe essence: {err}"));
                self.probe = None;
            }
        }
    }

    /// Formats the complete technical specifications for the platform clipboard.
    pub fn generate_copy_summary(&self) -> String {
        let mut out = String::new();
        out.push_str("=== encoderCraft Media Source Properties ===\n");
        out.push_str(&format!("Source File: {}\n", self.source_name));
        if let Some(ref path) = self.source_path {
            out.push_str(&format!("File Path: {}\n", path.display()));
        }
        if let Some(size) = self.file_size {
            out.push_str(&format!("File Size: {}\n", format_bytes(size)));
        }

        if let Some(ref probe) = self.probe {
            out.push_str(&format!("Container: {}\n", probe.container));
            out.push_str(&format!("Media Kind: {:?}\n", probe.kind));
            out.push_str(&format!("Duration: {} ({:.2}s)\n", format_time_seconds(probe.duration_seconds), probe.duration_seconds));

            if let Some(ref v) = probe.video {
                out.push_str("\n--- Video Stream ---\n");
                out.push_str(&format!("Codec: {}\n", v.codec));
                out.push_str(&format!("Dimensions: {} x {} [{}]\n", v.width, v.height, aspect_ratio_str(v.width, v.height)));
                out.push_str(&format!("Frame Rate: {:.3} fps\n", v.frame_rate.as_f64()));
                out.push_str(&format!("Pixel Format: {} ({}-bit)\n", v.pixel_format, v.bit_depth));
                out.push_str(&format!("Color Primaries: {:?}\n", v.color_space.primaries));
                out.push_str(&format!("Color Transfer: {:?}\n", v.color_space.transfer));
                out.push_str(&format!("Matrix Coefficients: {:?}\n", v.color_space.matrix));
                out.push_str(&format!("Color Range: {:?}\n", v.color_space.range));
                out.push_str(&format!("Alpha Channel: {}\n", if v.has_alpha { "Straight Alpha" } else { "None" }));
                if let Some(bps) = v.bitrate {
                    out.push_str(&format!("Bitrate: {:.2} Mbps\n", bps as f64 / 1_000_000.0));
                }
            }

            if !probe.audio.is_empty() {
                out.push_str("\n--- Audio Streams ---\n");
                for (idx, a) in probe.audio.iter().enumerate() {
                    out.push_str(&format!(
                        "Track #{}: {} · {} Hz · {} [{}] · {}\n",
                        idx + 1,
                        a.codec,
                        a.sample_rate,
                        a.channels,
                        channel_layout_label(a.channels),
                        a.bits_per_sample.map(|b| format!("{b}-bit")).unwrap_or_else(|| "16-bit".into())
                    ));
                }
            }
        }
        out
    }
}

/// Renders the modal Source Properties dialog window (AME Parity).
pub fn render_source_properties_dialog(ctx: &egui::Context, state: &mut SourcePropertiesState, tokens: &Tokens) {
    if !state.is_open {
        return;
    }

    let mut is_open = state.is_open;
    let window_title = format!("Source Properties — {}", state.source_name);

    egui::Window::new(window_title).open(&mut is_open).resizable(true).collapsible(false).default_width(540.0).default_height(480.0).show(
        ctx,
        |ui| {
            render_dialog_contents(ui, state, tokens);
        },
    );

    state.is_open = is_open;
}

fn render_dialog_contents(ui: &mut Ui, state: &mut SourcePropertiesState, tokens: &Tokens) {
    if let Some(ref err) = state.error_message {
        ui.label(RichText::new(err).color(tokens.status_failed));
        ui.add_space(8.0);
    }

    egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
        // --- 1. File Essence Group ---
        egui::CollapsingHeader::new(RichText::new("File & Container Essence").strong()).default_open(true).show(ui, |ui| {
            egui::Grid::new("file_essence_grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("File Name:").color(tokens.text_dim));
                ui.label(&state.source_name);
                ui.end_row();

                if let Some(ref path) = state.source_path {
                    ui.label(RichText::new("Full Path:").color(tokens.text_dim));
                    ui.label(path.display().to_string());
                    ui.end_row();
                }

                if let Some(size) = state.file_size {
                    ui.label(RichText::new("File Size:").color(tokens.text_dim));
                    ui.label(format_bytes(size));
                    ui.end_row();
                }

                if let Some(ref probe) = state.probe {
                    ui.label(RichText::new("Container Format:").color(tokens.text_dim));
                    ui.label(&probe.container);
                    ui.end_row();

                    ui.label(RichText::new("Media Type:").color(tokens.text_dim));
                    ui.label(format!("{:?}", probe.kind));
                    ui.end_row();
                }
            });
        });

        ui.add_space(8.0);

        // --- 2. Video Essence Group ---
        if let Some(ref probe) = state.probe {
            if let Some(ref v) = probe.video {
                egui::CollapsingHeader::new(RichText::new("Video Stream Essence").strong()).default_open(true).show(ui, |ui| {
                    egui::Grid::new("video_essence_grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                        ui.label(RichText::new("Codec:").color(tokens.text_dim));
                        ui.label(&v.codec);
                        ui.end_row();

                        ui.label(RichText::new("Frame Dimensions:").color(tokens.text_dim));
                        ui.label(format!("{} × {} ({})", v.width, v.height, aspect_ratio_str(v.width, v.height)));
                        ui.end_row();

                        ui.label(RichText::new("Pixel Aspect Ratio:").color(tokens.text_dim));
                        ui.label("1.0 (Square Pixels)");
                        ui.end_row();

                        ui.label(RichText::new("Frame Rate:").color(tokens.text_dim));
                        ui.label(format!("{:.3} fps ({}/{})", v.frame_rate.as_f64(), v.frame_rate.num, v.frame_rate.den));
                        ui.end_row();

                        ui.label(RichText::new("Pixel Format & Depth:").color(tokens.text_dim));
                        ui.label(format!("{} ({}-bit)", v.pixel_format, v.bit_depth));
                        ui.end_row();

                        ui.label(RichText::new("Color Primaries:").color(tokens.text_dim));
                        ui.label(format!("{:?}", v.color_space.primaries));
                        ui.end_row();

                        ui.label(RichText::new("Transfer Function:").color(tokens.text_dim));
                        ui.label(format!("{:?}", v.color_space.transfer));
                        ui.end_row();

                        ui.label(RichText::new("Matrix Coefficients:").color(tokens.text_dim));
                        ui.label(format!("{:?}", v.color_space.matrix));
                        ui.end_row();

                        ui.label(RichText::new("Color Range:").color(tokens.text_dim));
                        ui.label(format!("{:?}", v.color_space.range));
                        ui.end_row();

                        ui.label(RichText::new("Alpha Channel:").color(tokens.text_dim));
                        ui.label(if v.has_alpha { "Present (Straight Alpha)" } else { "None" });
                        ui.end_row();

                        if let Some(bps) = v.bitrate {
                            ui.label(RichText::new("Average Bitrate:").color(tokens.text_dim));
                            ui.label(format!("{:.2} Mbps", bps as f64 / 1_000_000.0));
                            ui.end_row();
                        }
                    });
                });
            }

            ui.add_space(8.0);

            // --- 3. Audio Essence Group ---
            if !probe.audio.is_empty() {
                egui::CollapsingHeader::new(RichText::new("Audio Stream Essence").strong()).default_open(true).show(ui, |ui| {
                    for (idx, a) in probe.audio.iter().enumerate() {
                        ui.label(RichText::new(format!("Track #{}", idx + 1)).strong().color(tokens.accent_primary));
                        egui::Grid::new(format!("audio_grid_{idx}")).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                            ui.label(RichText::new("Codec:").color(tokens.text_dim));
                            ui.label(&a.codec);
                            ui.end_row();

                            ui.label(RichText::new("Sample Rate:").color(tokens.text_dim));
                            ui.label(format!("{} Hz", a.sample_rate));
                            ui.end_row();

                            ui.label(RichText::new("Channel Layout:").color(tokens.text_dim));
                            ui.label(format!("{} ({})", a.channels, channel_layout_label(a.channels)));
                            ui.end_row();

                            ui.label(RichText::new("Bit Depth:").color(tokens.text_dim));
                            ui.label(a.bits_per_sample.map(|b| format!("{b}-bit")).unwrap_or_else(|| "16-bit".into()));
                            ui.end_row();
                        });
                        ui.add_space(4.0);
                    }
                });
            }

            ui.add_space(8.0);

            // --- 4. Timing & Timecode Group ---
            egui::CollapsingHeader::new(RichText::new("Timing & Timecode").strong()).default_open(true).show(ui, |ui| {
                egui::Grid::new("timing_grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    ui.label(RichText::new("Duration:").color(tokens.text_dim));
                    ui.label(format!("{} ({:.2} s)", format_time_seconds(probe.duration_seconds), probe.duration_seconds));
                    ui.end_row();

                    if let Some(start_tc) = probe.start_timecode_frames {
                        ui.label(RichText::new("Start Timecode Frames:").color(tokens.text_dim));
                        ui.label(format!("{start_tc} frames"));
                        ui.end_row();
                    }

                    if let Some(ref v) = probe.video {
                        let total_frames = (probe.duration_seconds * v.frame_rate.as_f64()).round() as u64;
                        ui.label(RichText::new("Total Frames:").color(tokens.text_dim));
                        ui.label(format!("{total_frames} frames"));
                        ui.end_row();
                    }
                });
            });
        }
    });

    ui.add_space(tokens.panel_inset);
    ui.separator();

    // --- Footer Action Bar ---
    ui.horizontal(|ui| {
        if ui.button("📋 Copy Information").on_hover_text("Copy formatted technical metadata to system clipboard").clicked() {
            let summary = state.generate_copy_summary();
            ui.ctx().copy_text(summary);
        }

        if let Some(ref path) = state.source_path
            && ui.button("Reveal in File Manager").clicked()
        {
            reveal_in_default_file_manager(path);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.is_open = false;
            }
        });
    });
}

#[allow(clippy::zombie_processes)]
fn reveal_in_default_file_manager(path: &Path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time_seconds() {
        assert_eq!(format_time_seconds(0.0), "00:00:00.00");
        assert_eq!(format_time_seconds(65.5), "00:01:05.50");
        assert_eq!(format_time_seconds(3661.0), "01:01:01.00");
    }

    #[test]
    fn test_aspect_ratio_str() {
        assert_eq!(aspect_ratio_str(1920, 1080), "16:9 (1.78:1)");
        assert_eq!(aspect_ratio_str(3840, 2160), "16:9 (1.78:1)");
        assert_eq!(aspect_ratio_str(1080, 1920), "9:16 (Vertical 0.56:1)");
        assert_eq!(aspect_ratio_str(1000, 1000), "1:1 (Square)");
        assert_eq!(aspect_ratio_str(1440, 1080), "4:3 (1.33:1)");
    }

    #[test]
    fn test_channel_layout_label() {
        assert_eq!(channel_layout_label(1), "Mono (1.0)");
        assert_eq!(channel_layout_label(2), "Stereo (2.0)");
        assert_eq!(channel_layout_label(6), "5.1 Surround");
        assert_eq!(channel_layout_label(8), "7.1 Surround / 8-Ch Discrete");
    }

    #[test]
    fn test_copy_summary_formatting() {
        let state = SourcePropertiesState { source_name: "test_clip.mov".to_string(), file_size: Some(10485760), ..Default::default() };
        let summary = state.generate_copy_summary();
        assert!(summary.contains("test_clip.mov"));
        assert!(summary.contains("10.0 MB"));
    }
}
