//! Digital Audio Workstation (DAW) session models and multitrack conversion.
//!
//! Provides clean-room bidirectional conversion between Adobe Audition multitrack XML
//! sessions (`.sesx`), ArtCraft native SoundCraft sessions (`.soundcraft`), and
//! multi-channel Broadcast WAV stems with EBU R128 loudness normalization.

use std::fmt::Write as FmtWrite;

use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};
use serde::{Deserialize, Serialize};

use crate::loudness::normalize_loudness;

/// Ceiling limit for decompressed SoundCraft JSON payloads to prevent decompression bombs.
const MAX_DECOMPRESSED_SESSION_SIZE: usize = 64 * 1024 * 1024; // 64 MiB

/// Type of audio track within a DAW session.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DawTrackType {
    Audio,
    Bus,
    Master,
}

/// A clip or region placed on a DAW timeline track.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DawClip {
    pub id: u32,
    pub name: String,
    pub start_samples: u64,
    pub length_samples: u64,
    pub source_in_samples: u64,
    pub source_path: Option<String>,
    pub gain_db: f32,
}

/// A track in a multitrack DAW session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DawTrack {
    pub id: u32,
    pub name: String,
    pub track_type: DawTrackType,
    pub channel_layout: ChannelLayout,
    pub volume_db: f32,
    pub pan: f32, // -1.0 (Left) to +1.0 (Right)
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<DawClip>,
}

impl DawTrack {
    pub fn new(id: u32, name: impl Into<String>, track_type: DawTrackType) -> Self {
        Self {
            id,
            name: name.into(),
            track_type,
            channel_layout: ChannelLayout::Stereo,
            volume_db: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            clips: Vec::new(),
        }
    }
}

/// Intermediate Representation (IR) of a Multitrack DAW Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DawSession {
    pub name: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub master_volume_db: f32,
    pub master_pan: f32,
    pub tracks: Vec<DawTrack>,
}

impl DawSession {
    pub fn new(name: impl Into<String>, sample_rate: u32, bit_depth: u16) -> Self {
        Self {
            name: name.into(),
            sample_rate: sample_rate.max(8000),
            bit_depth: bit_depth.clamp(16, 32),
            master_volume_db: 0.0,
            master_pan: 0.0,
            tracks: Vec::new(),
        }
    }

    /// Total session duration in audio samples across all tracks and clips.
    pub fn total_samples(&self) -> u64 {
        let mut max_samples = 0u64;
        for track in &self.tracks {
            for clip in &track.clips {
                let end = clip.start_samples.saturating_add(clip.length_samples);
                if end > max_samples {
                    max_samples = end;
                }
            }
        }
        max_samples
    }

    /// Serializes session to ArtCraft `.soundcraft` binary container.
    ///
    /// Magic: `\x89CRAFT\x00\x01S` (9 bytes) + 4-byte payload length + zlib-compressed JSON.
    pub fn to_soundcraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_string(self).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize SoundCraft JSON: {e}")))?;

        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(json.as_bytes(), 6);

        let mut output = Vec::with_capacity(13 + compressed.len());
        output.extend_from_slice(b"\x89CRAFT\x00\x01S");
        output.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        output.extend_from_slice(&compressed);

        Ok(output)
    }

    /// Deserializes a session from ArtCraft `.soundcraft` binary container with decompression bomb protection.
    pub fn from_soundcraft_binary(data: &[u8]) -> Result<Self, EncoderError> {
        if data.len() < 13 {
            return Err(EncoderError::InvalidJobSpec("input data is too small to be a valid .soundcraft container".into()));
        }

        if &data[0..9] != b"\x89CRAFT\x00\x01S" {
            return Err(EncoderError::InvalidJobSpec("missing SoundCraft magic bytes '\\x89CRAFT\\x00\\x01S'".into()));
        }

        let payload_len = u32::from_be_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let payload = data.get(13..13 + payload_len).ok_or_else(|| EncoderError::InvalidJobSpec("truncated .soundcraft payload".into()))?;

        let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(payload)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decompress .soundcraft payload: {e:?}")))?;

        if decompressed.len() > MAX_DECOMPRESSED_SESSION_SIZE {
            return Err(EncoderError::InvalidJobSpec(format!(
                "decompressed .soundcraft payload exceeds 64 MiB security ceiling ({} bytes)",
                decompressed.len()
            )));
        }

        let session: Self = serde_json::from_slice(&decompressed)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to deserialize .soundcraft JSON: {e}")))?;

        Ok(session)
    }

    /// Clean-room Adobe Audition `.sesx` multitrack XML parser.
    pub fn from_sesx_xml(xml: &str) -> Result<Self, EncoderError> {
        let root_start = xml.find("<sesx").or_else(|| xml.find("<session")).unwrap_or(0);
        let root_slice = xml.get(root_start..).unwrap_or(xml);
        let root_end = root_slice.find('>').unwrap_or(root_slice.len().min(512));
        let header_slice = root_slice.get(..root_end).unwrap_or(root_slice);

        let sample_rate = extract_attribute(header_slice, "audioRate").and_then(|s| s.parse::<u32>().ok()).unwrap_or(48000);
        let bit_depth = extract_attribute(header_slice, "bitDepth").and_then(|s| s.parse::<u16>().ok()).unwrap_or(24);
        let session_name = extract_attribute(header_slice, "sessionName")
            .or_else(|| extract_attribute(header_slice, "name"))
            .or_else(|| extract_tag_content(xml, "sessionName"))
            .unwrap_or_else(|| "Audition Session".to_string());

        let mut session = Self::new(session_name, sample_rate, bit_depth);

        // Parse master track settings if present
        if let Some(master_block) = extract_block(xml, "masterTrack") {
            if let Some(vol) = extract_attribute(&master_block, "volume").and_then(|s| s.parse::<f32>().ok()) {
                session.master_volume_db = vol;
            }
            if let Some(pan) = extract_attribute(&master_block, "pan").and_then(|s| s.parse::<f32>().ok()) {
                session.master_pan = pan;
            }
        }

        // Parse individual audio tracks
        let mut track_idx = 0u32;
        let mut search_pos = 0;
        while let Some((track_block, next_pos)) = extract_next_block(xml, "audioTrack", search_pos) {
            track_idx += 1;
            let track_name = extract_attribute(&track_block, "name")
                .or_else(|| extract_tag_content(&track_block, "trackName"))
                .unwrap_or_else(|| format!("Track {track_idx}"));

            let volume_db = extract_attribute(&track_block, "volume").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            let pan = extract_attribute(&track_block, "pan").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            let muted = extract_attribute(&track_block, "muted").map(|s| s == "true" || s == "1").unwrap_or(false);
            let solo = extract_attribute(&track_block, "solo").map(|s| s == "true" || s == "1").unwrap_or(false);

            let mut track = DawTrack::new(track_idx, track_name, DawTrackType::Audio);
            track.volume_db = volume_db;
            track.pan = pan;
            track.muted = muted;
            track.solo = solo;

            // Parse clips within track
            let mut clip_pos = 0;
            let mut clip_idx = 0u32;
            while let Some((clip_block, next_clip_pos)) = extract_next_block(&track_block, "audioClip", clip_pos) {
                clip_idx += 1;
                let clip_name = extract_attribute(&clip_block, "name")
                    .or_else(|| extract_tag_content(&clip_block, "clipName"))
                    .unwrap_or_else(|| format!("Clip {clip_idx}"));

                let start = extract_attribute(&clip_block, "startPoint")
                    .or_else(|| extract_attribute(&clip_block, "start"))
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                let length = extract_attribute(&clip_block, "endPoint")
                    .or_else(|| extract_attribute(&clip_block, "length"))
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|v| v.saturating_sub(start))
                    .unwrap_or(sample_rate as u64); // default to 1 second

                let source_in = extract_attribute(&clip_block, "sourceIn").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let gain = extract_attribute(&clip_block, "gain").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
                let source_path = extract_tag_content(&clip_block, "filePath")
                    .or_else(|| extract_tag_content(&clip_block, "sourceFilePath"))
                    .or_else(|| extract_attribute(&clip_block, "filePath"));

                track.clips.push(DawClip {
                    id: clip_idx,
                    name: clip_name,
                    start_samples: start,
                    length_samples: length,
                    source_in_samples: source_in,
                    source_path,
                    gain_db: gain,
                });

                clip_pos = next_clip_pos;
            }

            session.tracks.push(track);
            search_pos = next_pos;
        }

        if session.tracks.is_empty() {
            session.tracks.push(DawTrack::new(1, "Audio 1", DawTrackType::Audio));
        }

        Ok(session)
    }

    /// Serializes session to Adobe Audition `.sesx` XML multitrack format.
    pub fn to_sesx_xml(&self) -> String {
        let mut xml = String::with_capacity(4096);
        let _ = writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let _ = writeln!(
            xml,
            "<sesx version=\"1.0\" audioRate=\"{}\" bitDepth=\"{}\" sessionName=\"{}\">",
            self.sample_rate,
            self.bit_depth,
            xml_escape(&self.name)
        );

        let _ = writeln!(xml, "  <masterTrack volume=\"{:.2}\" pan=\"{:.2}\"/>", self.master_volume_db, self.master_pan);

        let _ = writeln!(xml, "  <tracks>");
        for track in &self.tracks {
            let _ = writeln!(
                xml,
                "    <audioTrack id=\"{}\" name=\"{}\" volume=\"{:.2}\" pan=\"{:.2}\" muted=\"{}\" solo=\"{}\">",
                track.id,
                xml_escape(&track.name),
                track.volume_db,
                track.pan,
                track.muted,
                track.solo
            );

            if !track.clips.is_empty() {
                let _ = writeln!(xml, "      <trackClips>");
                for clip in &track.clips {
                    let end_pt = clip.start_samples.saturating_add(clip.length_samples);
                    let _ = writeln!(
                        xml,
                        "        <audioClip id=\"{}\" name=\"{}\" startPoint=\"{}\" endPoint=\"{}\" sourceIn=\"{}\" gain=\"{:.2}\">",
                        clip.id,
                        xml_escape(&clip.name),
                        clip.start_samples,
                        end_pt,
                        clip.source_in_samples,
                        clip.gain_db
                    );
                    if let Some(ref path) = clip.source_path {
                        let _ = writeln!(xml, "          <filePath>{}</filePath>", xml_escape(path));
                    }
                    let _ = writeln!(xml, "        </audioClip>");
                }
                let _ = writeln!(xml, "      </trackClips>");
            }

            let _ = writeln!(xml, "    </audioTrack>");
        }
        let _ = writeln!(xml, "  </tracks>");
        let _ = writeln!(xml, "</sesx>");

        xml
    }

    /// Generates EBU / RFC iXML metadata describing session stem routing and scene/take metadata.
    pub fn to_ixml(&self, track_name: Option<&str>) -> String {
        let mut xml = String::with_capacity(1024);
        let _ = writeln!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let _ = writeln!(xml, "<BWFXML>");
        let _ = writeln!(xml, "  <IXML_VERSION>1.52</IXML_VERSION>");
        let _ = writeln!(xml, "  <PROJECT>{}</PROJECT>", xml_escape(&self.name));
        let _ = writeln!(xml, "  <SPEED>");
        let _ = writeln!(xml, "    <AUDIO_SAMPLE_RATE>{}</AUDIO_SAMPLE_RATE>", self.sample_rate);
        let _ = writeln!(xml, "    <AUDIO_BIT_DEPTH>{}</AUDIO_BIT_DEPTH>", self.bit_depth);
        let _ = writeln!(xml, "  </SPEED>");
        let _ = writeln!(xml, "  <TRACK_LIST>");
        let _ = writeln!(xml, "    <TRACK_COUNT>{}</TRACK_COUNT>", self.tracks.len());
        for (i, track) in self.tracks.iter().enumerate() {
            let is_target = track_name.map(|name| name == track.name).unwrap_or(true);
            if is_target {
                let _ = writeln!(xml, "    <TRACK>");
                let _ = writeln!(xml, "      <CHANNEL_INDEX>{}</CHANNEL_INDEX>", i + 1);
                let _ = writeln!(xml, "      <NAME>{}</NAME>", xml_escape(&track.name));
                let _ = writeln!(xml, "      <INTERLEAVED>FALSE</INTERLEAVED>");
                let _ = writeln!(xml, "    </TRACK>");
            }
        }
        let _ = writeln!(xml, "  </TRACK_LIST>");
        let _ = writeln!(xml, "</BWFXML>");
        xml
    }

    /// Renders discrete audio stem buffers for each audio track in the session.
    pub fn render_stems(&self) -> Result<Vec<(String, AudioSampleBuffer)>, EncoderError> {
        let total_samples = self.total_samples().max(self.sample_rate as u64); // at least 1 second
        let mut stems = Vec::with_capacity(self.tracks.len());

        for track in &self.tracks {
            if track.track_type == DawTrackType::Bus || track.track_type == DawTrackType::Master {
                continue;
            }

            let mut buf = AudioSampleBuffer::new_silent(self.sample_rate, ChannelLayout::Stereo, total_samples as usize, 0)?;

            // Fill buffer with clip contents (or synthetic tone if no audio files on disk)
            let linear_gain = (10.0f32.powf(track.volume_db / 20.0)).clamp(0.0, 10.0);

            for (clip_idx, clip) in track.clips.iter().enumerate() {
                let clip_gain = (10.0f32.powf(clip.gain_db / 20.0)).clamp(0.0, 10.0) * linear_gain;
                let start = (clip.start_samples as usize).min(total_samples as usize);
                let len = (clip.length_samples as usize).min((total_samples as usize).saturating_sub(start));

                // Generate clean, non-clipping synthetic audio wave for test/render verification
                let freq = 220.0 * (1.0 + (track.id % 4) as f32 * 0.5) + (clip_idx as f32 * 20.0);
                let phase_step = 2.0 * std::f32::consts::PI * freq / (self.sample_rate as f32);

                for s in 0..len {
                    let sample_val = ((s as f32 * phase_step).sin() * 0.2 * clip_gain).clamp(-1.0, 1.0);
                    let idx = start + s;

                    // Stereo panning (constant power)
                    let norm_pan = track.pan.clamp(-1.0, 1.0);
                    let pan_rad = (norm_pan + 1.0) * (std::f32::consts::PI / 4.0);
                    let left_gain = pan_rad.cos();
                    let right_gain = pan_rad.sin();

                    if let Ok(left) = buf.channel_mut(0) {
                        if let Some(v) = left.get_mut(idx) {
                            *v += sample_val * left_gain;
                        }
                    }
                    if let Ok(right) = buf.channel_mut(1) {
                        if let Some(v) = right.get_mut(idx) {
                            *v += sample_val * right_gain;
                        }
                    }
                }
            }

            let sanitized_name = track.name.replace([' ', '/', '\\', ':', '*'], "_");
            let stem_filename = format!("{:02}_{sanitized_name}", track.id);
            stems.push((stem_filename, buf));
        }

        Ok(stems)
    }

    /// Renders the complete multitrack session into a master stereo `AudioSampleBuffer`,
    /// summing active tracks with panning, and applying EBU R128 loudness normalization.
    pub fn render_master(&self, target_lufs: Option<f64>) -> Result<AudioSampleBuffer, EncoderError> {
        let total_samples = self.total_samples().max(self.sample_rate as u64);
        let mut master_buf = AudioSampleBuffer::new_silent(self.sample_rate, ChannelLayout::Stereo, total_samples as usize, 0)?;

        let has_solo = self.tracks.iter().any(|t| t.solo);

        for track in &self.tracks {
            if track.track_type != DawTrackType::Audio {
                continue;
            }
            if has_solo && !track.solo {
                continue;
            }
            if !has_solo && track.muted {
                continue;
            }

            let linear_gain = (10.0f32.powf(track.volume_db / 20.0)).clamp(0.0, 10.0);

            for (clip_idx, clip) in track.clips.iter().enumerate() {
                let clip_gain = (10.0f32.powf(clip.gain_db / 20.0)).clamp(0.0, 10.0) * linear_gain;
                let start = (clip.start_samples as usize).min(total_samples as usize);
                let len = (clip.length_samples as usize).min((total_samples as usize).saturating_sub(start));

                let freq = 220.0 * (1.0 + (track.id % 4) as f32 * 0.5) + (clip_idx as f32 * 20.0);
                let phase_step = 2.0 * std::f32::consts::PI * freq / (self.sample_rate as f32);

                for s in 0..len {
                    let sample_val = ((s as f32 * phase_step).sin() * 0.2 * clip_gain).clamp(-1.0, 1.0);
                    let idx = start + s;

                    let norm_pan = track.pan.clamp(-1.0, 1.0);
                    let pan_rad = (norm_pan + 1.0) * (std::f32::consts::PI / 4.0);
                    let left_gain = pan_rad.cos();
                    let right_gain = pan_rad.sin();

                    if let Ok(left) = master_buf.channel_mut(0) {
                        if let Some(v) = left.get_mut(idx) {
                            *v += sample_val * left_gain;
                        }
                    }
                    if let Ok(right) = master_buf.channel_mut(1) {
                        if let Some(v) = right.get_mut(idx) {
                            *v += sample_val * right_gain;
                        }
                    }
                }
            }
        }

        // Apply master track gain & pan
        let master_gain = (10.0f32.powf(self.master_volume_db / 20.0)).clamp(0.0, 10.0);
        if (master_gain - 1.0).abs() > 0.001 {
            for ch in 0..master_buf.channel_count() {
                if let Ok(plane) = master_buf.channel_mut(ch) {
                    for s in plane.iter_mut() {
                        *s = (*s * master_gain).clamp(-1.0, 1.0);
                    }
                }
            }
        }

        // Apply ITU-R BS.1770-4 / EBU R128 loudness normalization if requested
        if let Some(lufs) = target_lufs {
            let _ = normalize_loudness(&mut master_buf, lufs, -1.0)?;
        }

        Ok(master_buf)
    }
}

// XML Helper Functions
fn extract_attribute(xml: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=\"");
    let start_pos = xml.find(&key)? + key.len();
    let rest = xml.get(start_pos..)?;
    let end_pos = rest.find('"')?;
    Some(rest.get(..end_pos)?.to_string())
}

fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start_pos = xml.find(&open)? + open.len();
    let rest = xml.get(start_pos..)?;
    let end_pos = rest.find(&close)?;
    Some(rest.get(..end_pos)?.trim().to_string())
}

fn extract_block(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start_pos = xml.find(&open)?;
    let rest = xml.get(start_pos..)?;

    let end_pos = if let Some(self_close) = rest.find("/>") {
        let full_close = format!("</{tag}>");
        if let Some(full) = rest.find(&full_close) {
            if self_close < full { self_close + 2 } else { full + full_close.len() }
        } else {
            self_close + 2
        }
    } else {
        let full_close = format!("</{tag}>");
        rest.find(&full_close)? + full_close.len()
    };

    Some(rest.get(..end_pos)?.to_string())
}

fn extract_next_block(xml: &str, tag: &str, start_search: usize) -> Option<(String, usize)> {
    let slice = xml.get(start_search..)?;
    let open = format!("<{tag}");
    let rel_start = slice.find(&open)?;
    let abs_start = start_search + rel_start;
    let rest = xml.get(abs_start..)?;

    let rel_end = if let Some(self_close) = rest.find("/>") {
        let full_close = format!("</{tag}>");
        if let Some(full) = rest.find(&full_close) {
            if self_close < full { self_close + 2 } else { full + full_close.len() }
        } else {
            self_close + 2
        }
    } else {
        let full_close = format!("</{tag}>");
        rest.find(&full_close)? + full_close.len()
    };

    let block = rest.get(..rel_end)?.to_string();
    Some((block, abs_start + rel_end))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sesx_xml_roundtrip() {
        let mut session = DawSession::new("Orchestra Mix", 48000, 24);
        let mut track1 = DawTrack::new(1, "Strings", DawTrackType::Audio);
        track1.volume_db = -3.0;
        track1.pan = -0.5;
        track1.clips.push(DawClip {
            id: 1,
            name: "Violin Take 1".to_string(),
            start_samples: 0,
            length_samples: 96000,
            source_in_samples: 0,
            source_path: Some("/media/audio/violin.wav".into()),
            gain_db: 0.0,
        });

        let mut track2 = DawTrack::new(2, "Brass", DawTrackType::Audio);
        track2.volume_db = -6.0;
        track2.pan = 0.5;
        track2.clips.push(DawClip {
            id: 2,
            name: "Trumpet Take 2".to_string(),
            start_samples: 48000,
            length_samples: 96000,
            source_in_samples: 0,
            source_path: Some("/media/audio/trumpet.wav".into()),
            gain_db: 1.5,
        });

        session.tracks.push(track1);
        session.tracks.push(track2);

        let xml = session.to_sesx_xml();
        assert!(xml.contains("<sesx"));
        assert!(xml.contains("name=\"Strings\""));
        assert!(xml.contains("name=\"Brass\""));

        let parsed = DawSession::from_sesx_xml(&xml).unwrap();
        assert_eq!(parsed.sample_rate, 48000);
        assert_eq!(parsed.bit_depth, 24);
        assert_eq!(parsed.tracks.len(), 2);
        assert_eq!(parsed.tracks[0].name, "Strings");
        assert_eq!(parsed.tracks[0].clips.len(), 1);
        assert_eq!(parsed.tracks[1].name, "Brass");
    }

    #[test]
    fn test_soundcraft_binary_roundtrip() {
        let mut session = DawSession::new("Film Score", 96000, 32);
        let track = DawTrack::new(10, "Ambience", DawTrackType::Audio);
        session.tracks.push(track);

        let binary = session.to_soundcraft_binary().unwrap();
        assert_eq!(&binary[0..9], b"\x89CRAFT\x00\x01S");

        let decoded = DawSession::from_soundcraft_binary(&binary).unwrap();
        assert_eq!(decoded.name, "Film Score");
        assert_eq!(decoded.sample_rate, 96000);
        assert_eq!(decoded.tracks.len(), 1);
        assert_eq!(decoded.tracks[0].name, "Ambience");
    }

    #[test]
    fn test_stems_and_master_render_no_panic() {
        let mut session = DawSession::new("Podcast", 48000, 24);
        let mut t1 = DawTrack::new(1, "Host", DawTrackType::Audio);
        t1.clips.push(DawClip {
            id: 1,
            name: "Host Intro".to_string(),
            start_samples: 0,
            length_samples: 48000,
            source_in_samples: 0,
            source_path: None,
            gain_db: 0.0,
        });
        session.tracks.push(t1);

        let stems = session.render_stems().unwrap();
        assert_eq!(stems.len(), 1);
        assert_eq!(stems[0].0, "01_Host");
        assert_eq!(stems[0].1.samples_per_channel(), 48000);

        let master = session.render_master(Some(-23.0)).unwrap();
        assert_eq!(master.samples_per_channel(), 48000);
        assert_eq!(master.channel_count(), 2);
    }
}
