//! Sidecar caption export and video burn-in engine.
//!
//! Provides subtitle and closed-caption sidecar export (SRT, WebVTT, SCC, MCC, TTML, ASS/SSA, STL)
//! and raster burn-in compositing over video frames, matching Adobe Media Encoder's Captions panel.
//! Reuses `filmcraft-captions` for clean-room format translation and timing normalization, and provides
//! native SubStation Alpha (.ass / .ssa) matrix conversion.

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
        Self::from_bytes(&bytes, ext)
    }

    /// Loads and detects caption format from in-memory byte slice with optional extension hint.
    pub fn from_bytes(bytes: &[u8], ext_hint: Option<&str>) -> Result<Self, EncoderError> {
        let ext_lower = ext_hint.map(|s| s.trim_start_matches('.').to_ascii_lowercase());
        let text_lossy = String::from_utf8_lossy(bytes);

        // SubStation Alpha (.ass / .ssa)
        if ext_lower.as_deref() == Some("ass")
            || ext_lower.as_deref() == Some("ssa")
            || (text_lossy.contains("[Script Info]") && text_lossy.contains("[Events]"))
        {
            let doc = parse_ass(&text_lossy)?;
            return Ok(Self { doc });
        }

        let format =
            filmcraft_captions::detect(bytes, ext_hint).ok_or_else(|| EncoderError::InvalidJobSpec("unable to recognize caption format".into()))?;
        let doc = filmcraft_captions::parse(bytes, format).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse caption file: {e}")))?;
        Ok(Self { doc })
    }

    /// Parses caption text from a raw string with an explicit format.
    pub fn from_text(text: &str, format: Format) -> Result<Self, EncoderError> {
        let doc = filmcraft_captions::parse(text.as_bytes(), format)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse caption text: {e}")))?;
        Ok(Self { doc })
    }

    /// Parses caption text with a unified CaptionExportFormat.
    pub fn from_export_format(text: &str, format: CaptionExportFormat) -> Result<Self, EncoderError> {
        match format {
            CaptionExportFormat::Ass => {
                let doc = parse_ass(text)?;
                Ok(Self { doc })
            }
            CaptionExportFormat::Srt => Self::from_text(text, Format::Srt),
            CaptionExportFormat::WebVtt => Self::from_text(text, Format::WebVtt),
            CaptionExportFormat::Scc => Self::from_text(text, Format::Scc),
            CaptionExportFormat::Mcc => Self::from_text(text, Format::Mcc),
            CaptionExportFormat::Ttml => Self::from_text(text, Format::Ttml),
            CaptionExportFormat::Stl => Self::from_text(text, Format::Stl),
        }
    }

    /// Access the slice of parsed cues.
    pub fn cues(&self) -> &[Cue] {
        &self.doc.cues
    }

    /// Shifts all cues forward or backward in time by milliseconds.
    pub fn shift_ms(&mut self, offset_ms: i64) {
        if offset_ms == 0 {
            return;
        }
        let offset_ticks = (offset_ms as i128 * TICKS_PER_SECOND as i128 / 1000) as i64;
        for cue in &mut self.doc.cues {
            cue.start = Tick(cue.start.0.saturating_add(offset_ticks));
            cue.end = Tick(cue.end.0.saturating_add(offset_ticks));
        }
        self.doc.normalize();
    }

    /// Retimes all cues by a scaling factor (e.g. `source_fps / target_fps`).
    pub fn retime(&mut self, factor: f64) {
        if factor > 0.0 && (factor - 1.0).abs() > 1e-6 {
            for cue in &mut self.doc.cues {
                cue.start = Tick((cue.start.0 as f64 * factor).round() as i64);
                cue.end = Tick((cue.end.0 as f64 * factor).round() as i64);
            }
            self.doc.normalize();
        }
    }

    /// Serializes the caption document into raw bytes of the requested format.
    pub fn write_format(&self, format: CaptionExportFormat) -> Result<Vec<u8>, EncoderError> {
        match format {
            CaptionExportFormat::Ass => Ok(write_ass(&self.doc)),
            CaptionExportFormat::Srt => Ok(filmcraft_captions::write(&self.doc, Format::Srt, WriteOptions::default())),
            CaptionExportFormat::WebVtt => Ok(filmcraft_captions::write(&self.doc, Format::WebVtt, WriteOptions::default())),
            CaptionExportFormat::Scc => Ok(filmcraft_captions::write(&self.doc, Format::Scc, WriteOptions::default())),
            CaptionExportFormat::Mcc => Ok(filmcraft_captions::write(&self.doc, Format::Mcc, WriteOptions::default())),
            CaptionExportFormat::Ttml => Ok(filmcraft_captions::write(&self.doc, Format::Ttml, WriteOptions::default())),
            CaptionExportFormat::Stl => Ok(filmcraft_captions::write(&self.doc, Format::Stl, WriteOptions::default())),
        }
    }

    /// Exports a sidecar subtitle file alongside the target media file with optional time offset.
    pub fn export_sidecar(&self, target_media_path: &Path, format: CaptionExportFormat, offset_ms: Option<i64>) -> Result<PathBuf, EncoderError> {
        self.export_sidecar_with_options(target_media_path, format, offset_ms, None)
    }

    /// Exports a sidecar subtitle file alongside the target media file with time offset and framerate retime.
    pub fn export_sidecar_with_options(
        &self,
        target_media_path: &Path,
        format: CaptionExportFormat,
        offset_ms: Option<i64>,
        retime_factor: Option<f64>,
    ) -> Result<PathBuf, EncoderError> {
        let mut track = self.clone();
        if let Some(offset) = offset_ms {
            track.shift_ms(offset);
        }
        if let Some(factor) = retime_factor {
            track.retime(factor);
        }

        let bytes = track.write_format(format)?;
        let sidecar_path = target_media_path.with_extension(format.extension());

        if let Some(parent) = sidecar_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EncoderError::Io(format!("failed to create sidecar directory: {e}")))?;
        }

        std::fs::write(&sidecar_path, bytes)
            .map_err(|e| EncoderError::Io(format!("failed to write sidecar caption {}: {e}", sidecar_path.display())))?;

        Ok(sidecar_path)
    }
}

/// Parses SubStation Alpha (.ass / .ssa) text into a normalized `Document`.
fn parse_ass(text: &str) -> Result<Document, EncoderError> {
    let mut cues = Vec::new();
    let mut in_events = false;
    let mut format_cols: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_events = trimmed.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if in_events {
            if let Some(rest) = trimmed.strip_prefix("Format:") {
                format_cols = rest.split(',').map(|s| s.trim().to_ascii_lowercase()).collect();
            } else if let Some(rest) = trimmed.strip_prefix("Dialogue:") {
                let col_limit = format_cols.len().max(10);
                let parts: Vec<&str> = rest.splitn(col_limit, ',').collect();
                let start_idx = format_cols.iter().position(|c| c == "start").unwrap_or(1);
                let end_idx = format_cols.iter().position(|c| c == "end").unwrap_or(2);
                let text_idx = format_cols.iter().position(|c| c == "text").unwrap_or(parts.len().saturating_sub(1));
                let speaker_idx = format_cols.iter().position(|c| c == "name");

                if let (Some(&start_str), Some(&end_str), Some(&raw_text)) = (parts.get(start_idx), parts.get(end_idx), parts.get(text_idx)) {
                    let start_tick = parse_ass_timestamp(start_str.trim())?;
                    let end_tick = parse_ass_timestamp(end_str.trim())?;
                    let cue_text = clean_ass_text(raw_text.trim());
                    let speaker = speaker_idx.and_then(|idx| parts.get(idx)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                    cues.push(Cue { start: start_tick, end: end_tick, text: cue_text, speaker, id: None, settings: String::new() });
                }
            }
        }
    }
    let mut doc = Document { cues, ..Default::default() };
    doc.normalize();
    Ok(doc)
}

/// Parses an ASS timestamp `H:MM:SS.cs` into `Tick`.
fn parse_ass_timestamp(s: &str) -> Result<Tick, EncoderError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return Err(EncoderError::InvalidJobSpec(format!("invalid ASS timestamp format '{s}'")));
    }
    let h: f64 =
        parts.first().and_then(|p| p.trim().parse().ok()).ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid hours in timestamp '{s}'")))?;
    let m: f64 = parts
        .get(1)
        .and_then(|p| p.trim().parse().ok())
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid minutes in timestamp '{s}'")))?;
    let s_part: f64 = parts
        .get(2)
        .and_then(|p| p.trim().parse().ok())
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("invalid seconds in timestamp '{s}'")))?;

    if !h.is_finite() || !m.is_finite() || !s_part.is_finite() || h < 0.0 || m < 0.0 || s_part < 0.0 {
        return Err(EncoderError::InvalidJobSpec(format!("non-finite or negative timestamp '{s}'")));
    }

    let total_seconds = h * 3600.0 + m * 60.0 + s_part;
    Ok(Tick::from_seconds_f64(total_seconds))
}

/// Removes style overrides `{...}` and converts newlines in ASS dialogue text.
fn clean_ass_text(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    let mut in_curly = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            in_curly = true;
            continue;
        }
        if c == '}' {
            in_curly = false;
            continue;
        }
        if in_curly {
            continue;
        }
        if c == '\\'
            && let Some(&next) = chars.peek()
        {
            if next == 'N' || next == 'n' {
                chars.next();
                cleaned.push('\n');
                continue;
            } else if next == 'h' {
                chars.next();
                cleaned.push(' ');
                continue;
            }
        }
        cleaned.push(c);
    }
    cleaned
}

/// Serializes `Document` into SubStation Alpha (.ass) format.
fn write_ass(doc: &Document) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("[Script Info]\n");
    out.push_str("Title: encoderCraft Subtitle Export\n");
    out.push_str("ScriptType: v4.00+\n");
    out.push_str("WrapStyle: 0\n");
    out.push_str("ScaledBorderAndShadow: yes\n");
    out.push_str("PlayResX: 1920\n");
    out.push_str("PlayResY: 1080\n\n");
    out.push_str("[V4+ Styles]\n");
    out.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    out.push_str("Style: Default,Arial,22,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n");
    out.push_str("[Events]\n");
    out.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for cue in &doc.cues {
        let start = format_ass_timestamp(cue.start);
        let end = format_ass_timestamp(cue.end);
        let name = cue.speaker.as_deref().unwrap_or("");
        let text = cue.text.replace("\r\n", "\\N").replace(['\n', '\r'], "\\N");
        out.push_str(&format!("Dialogue: 0,{start},{end},Default,{name},0,0,0,,{text}\n"));
    }
    out.into_bytes()
}

/// Formats a `Tick` timestamp as `H:MM:SS.cs` for ASS subtitles.
fn format_ass_timestamp(tick: Tick) -> String {
    let secs = tick.seconds();
    let total_cs = (secs * 100.0).round().max(0.0) as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
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

    #[test]
    fn test_ass_subtitle_parse_and_write_roundtrip() {
        let sample_ass = "\
[Script Info]
Title: Test ASS
ScriptType: v4.00+

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.50,0:00:04.25,Default,Hero,0,0,0,,Line 1{\\b1} styled{\\b0}\\NLine 2
";
        let track = CaptionTrack::from_export_format(sample_ass, CaptionExportFormat::Ass).unwrap();
        assert_eq!(track.cues().len(), 1);
        let cue = &track.cues()[0];
        assert_eq!(cue.speaker.as_deref(), Some("Hero"));
        assert_eq!(cue.text, "Line 1 styled\nLine 2");
        assert!((cue.start.seconds() - 1.5).abs() < 0.01);
        assert!((cue.end.seconds() - 4.25).abs() < 0.01);

        let exported_bytes = track.write_format(CaptionExportFormat::Ass).unwrap();
        let exported_text = String::from_utf8(exported_bytes).unwrap();
        assert!(exported_text.contains("[Script Info]"));
        assert!(exported_text.contains("Dialogue: 0,0:00:01.50,0:00:04.25,Default,Hero,0,0,0,,Line 1 styled\\NLine 2"));
    }

    #[test]
    fn test_subtitle_universal_transcoding_matrix() {
        let track = CaptionTrack::from_text(SAMPLE_SRT, Format::Srt).unwrap();

        // Convert to WebVTT
        let vtt_bytes = track.write_format(CaptionExportFormat::WebVtt).unwrap();
        let vtt_text = String::from_utf8(vtt_bytes).unwrap();
        assert!(vtt_text.contains("WEBVTT"));

        // Convert to ASS
        let ass_bytes = track.write_format(CaptionExportFormat::Ass).unwrap();
        let ass_text = String::from_utf8(ass_bytes).unwrap();
        assert!(ass_text.contains("Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,Hello ArtCraft!"));

        // Parse back from ASS
        let track_from_ass = CaptionTrack::from_export_format(&ass_text, CaptionExportFormat::Ass).unwrap();
        assert_eq!(track_from_ass.cues().len(), 2);
        assert_eq!(track_from_ass.cues()[0].text, "Hello ArtCraft!");

        // Convert to TTML
        let ttml_bytes = track_from_ass.write_format(CaptionExportFormat::Ttml).unwrap();
        let ttml_text = String::from_utf8(ttml_bytes).unwrap();
        assert!(ttml_text.contains("<tt") || ttml_text.contains("<p"));
    }

    #[test]
    fn test_caption_retiming_framerate_scaling() {
        let mut track = CaptionTrack::from_text(SAMPLE_SRT, Format::Srt).unwrap();
        let orig_start = track.cues()[0].start.seconds();
        let orig_end = track.cues()[0].end.seconds();

        // 24.0 fps conforming to 25.0 fps (speed up by 25/24, duration shrinks by 24/25)
        let retime_factor = 24.0 / 25.0;
        track.retime(retime_factor);

        let new_start = track.cues()[0].start.seconds();
        let new_end = track.cues()[0].end.seconds();
        assert!((new_start - (orig_start * retime_factor)).abs() < 0.001);
        assert!((new_end - (orig_end * retime_factor)).abs() < 0.001);
    }
}
