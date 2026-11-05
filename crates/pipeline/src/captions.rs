//! Sidecar caption export and video burn-in engine.
//!
//! Provides subtitle and closed-caption sidecar export (SRT, WebVTT, SCC, MCC, TTML)
//! and raster burn-in compositing over video frames, matching Adobe Media Encoder's Captions panel.
//! Reuses `filmcraft-captions` for clean-room format translation and timing normalization.

use std::path::{Path, PathBuf};

use encodercraft_core::{CaptionExportFormat, EncoderError, VideoFrame};
use filmcraft_captions::{Cue, Document, Format, WriteOptions};
use filmcraft_time::{TICKS_PER_SECOND, Tick};

/// Re-export the caption formats for convenient use.
pub use filmcraft_captions::Format as NativeCaptionFormat;

/// Encapsulates a parsed caption track capable of time-shifting and sidecar serialization.
#[derive(Debug, Clone)]
pub struct CaptionTrack {
    doc: Document,
}

impl CaptionTrack {
    /// Loads and automatically detects the format of a caption file from disk.
    pub fn from_file(path: &Path) -> Result<Self, EncoderError> {
        let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read caption file {}: {e}", path.display())))?;
        let ext = path.extension().and_then(|s| s.to_str());
        let format = filmcraft_captions::detect(&bytes, ext)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("unable to recognize caption format for {}", path.display())))?;
        let doc = filmcraft_captions::parse(&bytes, format)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse caption file {}: {e}", path.display())))?;
        Ok(Self { doc })
    }

    /// Parses caption text from a raw string with an explicit format.
    pub fn from_text(text: &str, format: Format) -> Result<Self, EncoderError> {
        let doc = filmcraft_captions::parse(text.as_bytes(), format)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse caption text: {e}")))?;
        Ok(Self { doc })
    }

    /// Access the slice of parsed cues.
    pub fn cues(&self) -> &[Cue] {
        &self.doc.cues
    }

    /// Exports a sidecar subtitle file alongside the target media file.
    pub fn export_sidecar(&self, target_media_path: &Path, format: CaptionExportFormat, offset_ms: Option<i64>) -> Result<PathBuf, EncoderError> {
        let mut doc = self.doc.clone();
        if let Some(offset) = offset_ms {
            let offset_ticks = (offset as i128 * TICKS_PER_SECOND as i128 / 1000) as i64;
            for cue in &mut doc.cues {
                cue.start = Tick(cue.start.0.saturating_add(offset_ticks));
                cue.end = Tick(cue.end.0.saturating_add(offset_ticks));
            }
            doc.normalize();
        }

        let craft_format = match format {
            CaptionExportFormat::Srt => Format::Srt,
            CaptionExportFormat::WebVtt => Format::WebVtt,
            CaptionExportFormat::Scc => Format::Scc,
            CaptionExportFormat::Mcc => Format::Mcc,
            CaptionExportFormat::Ttml => Format::Ttml,
        };

        let bytes = filmcraft_captions::write(&doc, craft_format, WriteOptions::default());
        let sidecar_path = target_media_path.with_extension(format.extension());

        if let Some(parent) = sidecar_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create sidecar directory: {e}")))?;
        }

        std::fs::write(&sidecar_path, bytes)
            .map_err(|e| EncoderError::Io(format!("failed to write sidecar caption {}: {e}", sidecar_path.display())))?;

        Ok(sidecar_path)
    }
}

/// Bitmapped 5x7 glyph representation for ASCII characters.
fn caption_glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10011, 0b01111],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        ':' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00010, 0b00100],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '\'' => [0b00100, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000],
        '"' => [0b01010, 0b01010, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000],
        ' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

/// Caption burn-in renderer for compositing styled subtitle cues onto video frames.
#[derive(Debug, Clone)]
pub struct CaptionBurner {
    pub scale: usize,
    pub bg_opacity: f32,
    pub text_opacity: f32,
    pub margin_bottom: usize,
}

impl Default for CaptionBurner {
    fn default() -> Self {
        Self { scale: 3, bg_opacity: 0.75, text_opacity: 1.0, margin_bottom: 40 }
    }
}

impl CaptionBurner {
    /// Renders any active subtitle cues at `pts_seconds` directly onto the given frame.
    pub fn burn(&self, frame: &mut VideoFrame, track: &CaptionTrack, pts_seconds: f64) -> Result<(), EncoderError> {
        crate::overlay::rgb_layout(frame.format)?;

        // Find active cues at current presentation timestamp
        let mut active_text = String::new();
        for cue in track.cues() {
            let start = cue.start.seconds();
            let end = cue.end.seconds();
            if start <= pts_seconds && pts_seconds <= end {
                if !active_text.is_empty() {
                    active_text.push(' ');
                }
                // Strip common inline HTML tags like <i>, </i>, <b>, </b>
                let cleaned = cue.text.replace("<i>", "").replace("</i>", "").replace("<b>", "").replace("</b>", "").replace('\n', " ");
                active_text.push_str(&cleaned);
            }
        }

        if active_text.is_empty() {
            return Ok(());
        }

        self.draw_subtitle_line(frame, &active_text)
    }

    fn draw_subtitle_line(&self, frame: &mut VideoFrame, text: &str) -> Result<(), EncoderError> {
        let char_w = 5 * self.scale;
        let char_h = 7 * self.scale;
        let spacing = self.scale;
        let padding = 4 * self.scale;

        let total_chars = text.len();
        let text_width = total_chars.checked_mul(char_w + spacing).and_then(|w| w.checked_sub(spacing)).unwrap_or(0);
        let box_width = text_width.checked_add(padding * 2).unwrap_or(text_width);
        let box_height = char_h.checked_add(padding * 2).unwrap_or(char_h);

        let fw = frame.dimensions.width as usize;
        let fh = frame.dimensions.height as usize;

        if box_width >= fw || box_height >= fh {
            return Ok(());
        }

        // Center horizontally at bottom of frame
        let box_x = fw.saturating_sub(box_width) / 2;
        let box_y = fh.saturating_sub(box_height + self.margin_bottom);

        let (channels, bytes_per_component) = crate::overlay::rgb_layout(frame.format)?;
        let bg_alpha = self.bg_opacity.clamp(0.0, 1.0);
        let text_alpha = self.text_opacity.clamp(0.0, 1.0);

        let (data, stride) = frame.plane_mut(0)?;
        let bytes_per_pixel = channels * bytes_per_component;

        // 1. Draw rounded/pill dark background
        for by in 0..box_height {
            let py = box_y + by;
            if py >= fh {
                break;
            }
            for bx in 0..box_width {
                let px = box_x + bx;
                if px >= fw {
                    break;
                }
                let idx = py * stride + px * bytes_per_pixel;
                crate::overlay::blend_rgb(data, idx, bytes_per_component, [0.0; 3], bg_alpha)?;
            }
        }

        // 2. Draw text glyphs in high-contrast white
        let text_start_x = box_x + padding;
        let text_start_y = box_y + padding;

        for (i, ch) in text.chars().enumerate() {
            let glyph = caption_glyph(ch);
            let gx_base = text_start_x + i * (char_w + spacing);

            for (row_idx, &row_bits) in glyph.iter().enumerate() {
                for bit_col in 0..5 {
                    if (row_bits & (1 << (4 - bit_col))) != 0 {
                        // Render scaled block for this pixel
                        for sy in 0..self.scale {
                            let py = text_start_y + row_idx * self.scale + sy;
                            if py >= fh {
                                continue;
                            }
                            for sx in 0..self.scale {
                                let px = gx_base + bit_col * self.scale + sx;
                                if px >= fw {
                                    continue;
                                }
                                let idx = py * stride + px * bytes_per_pixel;
                                crate::overlay::blend_rgb(data, idx, bytes_per_component, [1.0; 3], text_alpha)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_geom::Dimensions;
    use tempfile::tempdir;

    const SAMPLE_SRT: &str = "\
1
00:00:01,000 --> 00:00:03,000
Hello ArtCraft!

2
00:00:04,000 --> 00:00:06,500
Subtitles Ready.
";

    #[test]
    fn test_caption_track_parse_and_export_sidecar() {
        let track = CaptionTrack::from_text(SAMPLE_SRT, Format::Srt).unwrap();
        assert_eq!(track.cues().len(), 2);
        assert_eq!(track.cues()[0].text, "Hello ArtCraft!");

        let dir = tempdir().unwrap();
        let target_video = dir.path().join("movie.mp4");

        // Export WebVTT sidecar
        let sidecar_vtt = track.export_sidecar(&target_video, CaptionExportFormat::WebVtt, None).unwrap();
        assert!(sidecar_vtt.exists());
        assert_eq!(sidecar_vtt.extension().unwrap(), "vtt");
        let content = std::fs::read_to_string(&sidecar_vtt).unwrap();
        assert!(content.contains("WEBVTT"));
        assert!(content.contains("Hello ArtCraft!"));

        // Export SRT sidecar with +500ms offset
        let sidecar_srt = track.export_sidecar(&target_video, CaptionExportFormat::Srt, Some(500)).unwrap();
        assert!(sidecar_srt.exists());
        let srt_content = std::fs::read_to_string(&sidecar_srt).unwrap();
        assert!(srt_content.contains("00:00:01,500 --> 00:00:03,500"));
    }

    #[test]
    fn test_caption_burner_draws_on_frame() {
        let track = CaptionTrack::from_text(SAMPLE_SRT, Format::Srt).unwrap();
        let dims = Dimensions::new(640, 360).unwrap();
        let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, encodercraft_core::Rational64::FPS_24).unwrap();

        let burner = CaptionBurner::default();

        // At 0.5s: no active caption, frame unchanged
        burner.burn(&mut frame, &track, 0.5).unwrap();
        assert_eq!(frame.data()[0], 0);

        // At 1.5s: active caption "Hello ArtCraft!"
        burner.burn(&mut frame, &track, 1.5).unwrap();

        // Verify that pixels were drawn (non-zero)
        let has_non_zero = frame.data().iter().any(|&b| b > 0);
        assert!(has_non_zero);
    }
}
