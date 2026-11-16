//! Digital Audio Workstation (DAW) session models and multitrack conversion.
//!
//! Provides clean-room bidirectional conversion between Adobe Audition multitrack XML
//! sessions (`.sesx`), ArtCraft native SoundCraft sessions (`.soundcraft`), and
//! multi-channel Broadcast WAV stems with EBU R128 loudness normalization.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};
use serde::{Deserialize, Serialize};

use crate::loudness::normalize_loudness;
use crate::remix::remix_channels;

/// Ceiling limit for decompressed SoundCraft JSON payloads to prevent decompression bombs.
const MAX_DECOMPRESSED_SESSION_SIZE: usize = 64 * 1024 * 1024; // 64 MiB
const MAX_RENDER_SAMPLES: u64 = 180_000_000;
const MAX_SOURCE_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_SOURCE_SAMPLES: usize = 400_000_000;
const MIN_SOURCE_SAMPLE_RATE: u32 = 1_000;
const DECLICK_SECONDS: f32 = 0.005;
const MIN_SAMPLE_RATE: u32 = 8_000;
const MAX_SAMPLE_RATE: u32 = 384_000;

fn parse_finite_f32(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

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

        let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(payload, MAX_DECOMPRESSED_SESSION_SIZE)
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

        let sample_rate = extract_attribute(header_slice, "audioRate")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|r| (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(r))
            .unwrap_or(48000);
        let bit_depth = extract_attribute(header_slice, "bitDepth").and_then(|s| s.parse::<u16>().ok()).unwrap_or(24);
        let session_name = extract_attribute(header_slice, "sessionName")
            .or_else(|| extract_attribute(header_slice, "name"))
            .or_else(|| extract_tag_content(xml, "sessionName"))
            .unwrap_or_else(|| "Audition Session".to_string());

        let mut session = Self::new(session_name, sample_rate, bit_depth);

        // Parse master track settings if present
        if let Some(master_block) = extract_block(xml, "masterTrack") {
            if let Some(vol) = extract_attribute(&master_block, "volume").and_then(|s| parse_finite_f32(&s)) {
                session.master_volume_db = vol;
            }
            if let Some(pan) = extract_attribute(&master_block, "pan").and_then(|s| parse_finite_f32(&s)) {
                session.master_pan = pan;
            }
        }

        let file_table = parse_file_table(xml);

        // Parse individual audio tracks
        let mut track_idx = 0u32;
        let mut search_pos = 0;
        while let Some((track_block, next_pos)) = extract_next_block(xml, "audioTrack", search_pos) {
            track_idx += 1;
            let track_name = extract_attribute(&track_block, "name")
                .or_else(|| extract_tag_content(&track_block, "trackName"))
                .unwrap_or_else(|| format!("Track {track_idx}"));

            let volume_db = extract_attribute(&track_block, "volume").and_then(|s| parse_finite_f32(&s)).unwrap_or(0.0);
            let pan = extract_attribute(&track_block, "pan").and_then(|s| parse_finite_f32(&s)).unwrap_or(0.0);
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

                let source_in = extract_attribute(&clip_block, "sourceIn")
                    .or_else(|| extract_attribute(&clip_block, "sourceInPoint"))
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let gain = extract_attribute(&clip_block, "gain").and_then(|s| parse_finite_f32(&s)).unwrap_or(0.0);
                let source_path = extract_tag_content(&clip_block, "filePath")
                    .or_else(|| extract_tag_content(&clip_block, "sourceFilePath"))
                    .or_else(|| extract_attribute(&clip_block, "filePath"))
                    .or_else(|| extract_attribute(&clip_block, "fileID").and_then(|id| file_table.get(&id).cloned()));

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

    fn render_length(&self) -> Result<usize, EncoderError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate) {
            return Err(EncoderError::InvalidJobSpec(format!("unsupported session sample rate {}", self.sample_rate)));
        }
        let total = self.total_samples();
        if total == 0 {
            return Err(EncoderError::InvalidJobSpec("session contains no audio clips to render".into()));
        }
        if total > MAX_RENDER_SAMPLES {
            return Err(EncoderError::InvalidJobSpec(format!("session length {total} samples exceeds the {MAX_RENDER_SAMPLES} sample render limit")));
        }
        usize::try_from(total).map_err(|_| EncoderError::InvalidJobSpec("session length does not fit in memory".into()))
    }

    /// Creates a renderer that decodes every referenced clip source from disk.
    ///
    /// Relative clip paths are resolved against `base_dir` (normally the session file's folder).
    pub fn renderer(&self, base_dir: Option<&Path>) -> Result<SessionRenderer<'_>, EncoderError> {
        Ok(SessionRenderer {
            session: self,
            base_dir: base_dir.map(Path::to_path_buf),
            total_samples: self.render_length()?,
            sources: HashMap::new(),
        })
    }
}

/// Mixes a `DawSession` into stems and a master from the real audio files its clips reference.
pub struct SessionRenderer<'a> {
    session: &'a DawSession,
    base_dir: Option<PathBuf>,
    total_samples: usize,
    sources: HashMap<PathBuf, Arc<Vec<Vec<f32>>>>,
}

impl SessionRenderer<'_> {
    /// Indices of the tracks that produce a stem (audio tracks only).
    pub fn stem_track_indices(&self) -> Vec<usize> {
        self.session.tracks.iter().enumerate().filter(|(_, t)| t.track_type == DawTrackType::Audio).map(|(i, _)| i).collect()
    }

    /// Renders one track as a discrete stem. Mono tracks yield a mono stem, others stereo.
    /// Returns the file-safe stem name and its samples at the session sample rate.
    pub fn render_stem(&mut self, track_index: usize) -> Result<(String, AudioSampleBuffer), EncoderError> {
        let session = self.session;
        let track =
            session.tracks.get(track_index).ok_or_else(|| EncoderError::InvalidJobSpec(format!("track index {track_index} is out of range")))?;
        let mono = track.channel_layout == ChannelLayout::Mono;
        let mut planes = self.mix_track(track, mono)?;
        guard_peak(&mut planes);

        let (layout, planes) = if mono {
            let mixed: Vec<f32> = match planes.as_slice() {
                [l, r] => l.iter().zip(r.iter()).map(|(a, b)| 0.5 * (a + b)).collect(),
                _ => return Err(EncoderError::Internal("track mix did not produce a stereo buffer".into())),
            };
            (ChannelLayout::Mono, vec![mixed])
        } else {
            (ChannelLayout::Stereo, planes)
        };

        let name = format!("{:02}_{}", track.id, sanitize_file_component(&track.name));
        Ok((name, AudioSampleBuffer::from_planar_channels(session.sample_rate, layout, 0, planes)?))
    }

    /// Renders the stereo master: summed active tracks (mute/solo honoured), master gain and
    /// balance, a 0 dBFS peak guard, then optional ITU-R BS.1770-4 / EBU R128 normalization.
    pub fn render_master(&mut self, target_lufs: Option<f64>) -> Result<AudioSampleBuffer, EncoderError> {
        let session = self.session;
        let has_solo = session.tracks.iter().any(|t| t.solo);
        let mut master = vec![vec![0.0f32; self.total_samples]; 2];

        for track in &session.tracks {
            if track.track_type != DawTrackType::Audio || (has_solo && !track.solo) || (!has_solo && track.muted) {
                continue;
            }
            let planes = self.mix_track(track, false)?;
            for (acc, plane) in master.iter_mut().zip(planes.iter()) {
                for (a, s) in acc.iter_mut().zip(plane.iter()) {
                    *a += *s;
                }
            }
        }

        let master_gain = craft_dsp::db_to_gain(clamp_db(session.master_volume_db));
        let (left_gain, right_gain) = craft_dsp::pan::gains(session.master_pan, craft_dsp::pan::PanLaw::Zero);
        for (plane, pan_gain) in master.iter_mut().zip([left_gain, right_gain]) {
            for s in plane.iter_mut() {
                *s *= master_gain * pan_gain;
            }
        }
        guard_peak(&mut master);

        let mut buffer = AudioSampleBuffer::from_planar_channels(session.sample_rate, ChannelLayout::Stereo, 0, master)?;
        if let Some(lufs) = target_lufs {
            normalize_loudness(&mut buffer, lufs, -1.0)?;
        }
        Ok(buffer)
    }

    fn mix_track(&mut self, track: &DawTrack, ignore_pan: bool) -> Result<Vec<Vec<f32>>, EncoderError> {
        let total = self.total_samples;
        let rate = self.session.sample_rate;
        let mut out = vec![vec![0.0f32; total]; 2];
        let track_gain = craft_dsp::db_to_gain(clamp_db(track.volume_db));
        let declick = ((rate as f32 * DECLICK_SECONDS) as usize).max(1);

        for clip in &track.clips {
            let Ok(start) = usize::try_from(clip.start_samples) else { continue };
            if start >= total || clip.length_samples == 0 {
                continue;
            }
            let wanted = usize::try_from(clip.length_samples).unwrap_or(usize::MAX).min(total - start);

            let path = clip
                .source_path
                .as_deref()
                .ok_or_else(|| EncoderError::InvalidJobSpec(format!("clip `{}` on track `{}` has no source audio file", clip.name, track.name)))?;
            let source = self.load_source(path)?;
            let src_len = source.first().map(Vec::len).unwrap_or(0);
            let src_in = usize::try_from(clip.source_in_samples).unwrap_or(usize::MAX);
            let n = wanted.min(src_len.saturating_sub(src_in));
            if n == 0 {
                continue;
            }

            let mut segment: Vec<Vec<f32>> = Vec::with_capacity(source.len());
            for plane in source.iter() {
                segment.push(plane.get(src_in..src_in + n).map(<[f32]>::to_vec).unwrap_or_default());
            }
            let fade = declick.min(n / 2);
            craft_dsp::offline::fade(&mut segment, fade, fade, craft_dsp::offline::FadeShape::EqualPower);

            let gain = track_gain * craft_dsp::db_to_gain(clamp_db(clip.gain_db));
            let stereo_source = segment.len() == 2;
            let (pan, law) = match (ignore_pan, stereo_source) {
                (true, _) => (0.0, craft_dsp::pan::PanLaw::Zero),
                (false, true) => (track.pan, craft_dsp::pan::PanLaw::Zero),
                (false, false) => (track.pan, craft_dsp::pan::PanLaw::Minus3),
            };
            let (left_gain, right_gain) = craft_dsp::pan::gains(pan, law);

            let left_src = segment.first().map(Vec::as_slice).unwrap_or(&[]);
            let right_src = segment.get(1).map(Vec::as_slice).unwrap_or(left_src);
            for (dst_plane, src, pan_gain) in [(0usize, left_src, left_gain), (1usize, right_src, right_gain)] {
                if let Some(dst) = out.get_mut(dst_plane).and_then(|p| p.get_mut(start..start + n)) {
                    for (d, s) in dst.iter_mut().zip(src.iter()) {
                        *d += *s * gain * pan_gain;
                    }
                }
            }
        }
        Ok(out)
    }

    fn load_source(&mut self, raw_path: &str) -> Result<Arc<Vec<Vec<f32>>>, EncoderError> {
        let path = self.resolve_source_path(raw_path)?;
        if let Some(cached) = self.sources.get(&path) {
            return Ok(Arc::clone(cached));
        }
        let decoded = Arc::new(decode_source(&path, self.session.sample_rate)?);
        self.sources.insert(path, Arc::clone(&decoded));
        Ok(decoded)
    }

    fn resolve_source_path(&self, raw_path: &str) -> Result<PathBuf, EncoderError> {
        let given = PathBuf::from(raw_path);
        let mut candidates = vec![given.clone()];
        if let Some(base) = &self.base_dir {
            if given.is_relative() {
                candidates.push(base.join(&given));
            }
            // Sessions authored on another machine keep foreign absolute paths; look beside the session.
            let file_name = raw_path.rsplit(['/', '\\']).next().unwrap_or(raw_path);
            if !file_name.is_empty() {
                candidates.push(base.join(file_name));
            }
        }
        candidates.into_iter().find(|c| c.is_file()).ok_or_else(|| EncoderError::Io(format!("clip source audio file `{raw_path}` was not found")))
    }
}

fn clamp_db(db: f32) -> f32 {
    if db.is_finite() { db.clamp(-144.0, 24.0) } else { 0.0 }
}

/// Scales planes down so no sample exceeds full scale and replaces non-finite samples with silence.
fn guard_peak(planes: &mut [Vec<f32>]) {
    for plane in planes.iter_mut() {
        for s in plane.iter_mut() {
            if !s.is_finite() {
                *s = 0.0;
            }
        }
    }
    let peak = craft_dsp::offline::peak(planes);
    if peak > 1.0 {
        let scale = 1.0 / peak;
        for plane in planes.iter_mut() {
            for s in plane.iter_mut() {
                *s *= scale;
            }
        }
    }
}

fn sanitize_file_component(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() { "track".to_string() } else { trimmed.chars().take(80).collect() }
}

/// Decodes an audio file, folds it to mono or stereo and resamples it to `session_rate`.
fn decode_source(path: &Path, session_rate: u32) -> Result<Vec<Vec<f32>>, EncoderError> {
    let meta = std::fs::metadata(path).map_err(|e| EncoderError::Io(format!("failed to stat clip source `{}`: {e}", path.display())))?;
    if meta.len() > MAX_SOURCE_FILE_BYTES {
        return Err(EncoderError::InvalidJobSpec(format!("clip source `{}` exceeds the {MAX_SOURCE_FILE_BYTES} byte limit", path.display())));
    }
    let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("failed to read clip source `{}`: {e}", path.display())))?;
    let (_info, decoded) = craft_audio_io::decode(&bytes, None)
        .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decode clip source `{}`: {e}", path.display())))?;
    drop(bytes);

    let source_rate = decoded.sample_rate;
    if !(MIN_SOURCE_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&source_rate) {
        return Err(EncoderError::InvalidJobSpec(format!("clip source `{}` has unsupported sample rate {source_rate}", path.display())));
    }
    let frames = decoded.channels.first().map(Vec::len).unwrap_or(0);
    if frames == 0 || decoded.channels.iter().any(|c| c.len() != frames) {
        return Err(EncoderError::InvalidJobSpec(format!("clip source `{}` contains no audio or has ragged channels", path.display())));
    }
    if frames.saturating_mul(decoded.channels.len()) > MAX_SOURCE_SAMPLES {
        return Err(EncoderError::InvalidJobSpec(format!("clip source `{}` is too large to mix", path.display())));
    }

    let mut channels = decoded.channels;
    for plane in channels.iter_mut() {
        for s in plane.iter_mut() {
            if !s.is_finite() {
                *s = 0.0;
            }
        }
    }
    let mut folded = fold_to_stereo_or_mono(channels)?;

    if source_rate != session_rate {
        let out_frames = (frames as u128).saturating_mul(session_rate as u128) / source_rate as u128;
        if out_frames > MAX_RENDER_SAMPLES as u128 {
            return Err(EncoderError::InvalidJobSpec(format!("clip source `{}` is too long after resampling", path.display())));
        }
        folded = craft_dsp::offline::resample(&folded, source_rate, session_rate);
    }
    Ok(folded)
}

fn fold_to_stereo_or_mono(channels: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>, EncoderError> {
    match channels.len() {
        0 => Err(EncoderError::InvalidJobSpec("clip source has no channels".into())),
        1 | 2 => Ok(channels),
        n @ (6 | 8) => {
            let layout = if n == 6 { ChannelLayout::Surround51 } else { ChannelLayout::Surround71 };
            let buffer = AudioSampleBuffer::from_planar_channels(48_000, layout, 0, channels)?;
            Ok(remix_channels(&buffer, ChannelLayout::Stereo)?.into_planar_channels())
        }
        n => {
            let frames = channels.first().map(Vec::len).unwrap_or(0);
            let (mut left, mut right) = (vec![0.0f32; frames], vec![0.0f32; frames]);
            let (left_count, right_count) = (n.div_ceil(2) as f32, (n / 2) as f32);
            for (idx, plane) in channels.iter().enumerate() {
                let (dst, count) = if idx % 2 == 0 { (&mut left, left_count) } else { (&mut right, right_count) };
                for (d, s) in dst.iter_mut().zip(plane.iter()) {
                    *d += *s / count;
                }
            }
            Ok(vec![left, right])
        }
    }
}

// XML Helper Functions
fn extract_attribute(xml: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=\"");
    let start_pos = xml.find(&key)? + key.len();
    let rest = xml.get(start_pos..)?;
    let end_pos = rest.find('"')?;
    Some(xml_unescape(rest.get(..end_pos)?))
}

/// Maps `<file id=".." absolutePath=".." relativePath=".."/>` entries to a media path.
fn parse_file_table(xml: &str) -> HashMap<String, String> {
    let mut table = HashMap::new();
    let mut pos = 0;
    while let Some(rel) = xml.get(pos..).and_then(|rest| rest.find("<file ")) {
        let start = pos + rel;
        let Some(end) = xml.get(start..).and_then(|rest| rest.find('>')) else { break };
        let header = xml.get(start..start + end).unwrap_or("");
        if let Some(id) = extract_attribute(header, "id")
            && let Some(path) = extract_attribute(header, "absolutePath").or_else(|| extract_attribute(header, "relativePath"))
        {
            table.insert(id, path);
        }
        pos = start + end + 1;
    }
    table
}

fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start_pos = xml.find(&open)? + open.len();
    let rest = xml.get(start_pos..)?;
    let end_pos = rest.find(&close)?;
    Some(xml_unescape(rest.get(..end_pos)?.trim()))
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

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
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

    use craft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};

    fn write_wav(path: &Path, rate: u32, channels: Vec<Vec<f32>>) {
        let buffer = AudioBuffer { sample_rate: rate, channels };
        let options = EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Float32, dither: false, bwf: None };
        std::fs::write(path, craft_audio_io::encode(&buffer, &options).unwrap()).unwrap();
    }

    fn sine(rate: u32, freq: f32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    fn rms(plane: &[f32]) -> f32 {
        (plane.iter().map(|v| v * v).sum::<f32>() / plane.len().max(1) as f32).sqrt()
    }

    fn zero_crossings(plane: &[f32]) -> usize {
        plane.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    fn clip(id: u32, start: u64, length: u64, path: &Path) -> DawClip {
        DawClip {
            id,
            name: format!("clip{id}"),
            start_samples: start,
            length_samples: length,
            source_in_samples: 0,
            source_path: Some(path.to_string_lossy().into_owned()),
            gain_db: 0.0,
        }
    }

    fn single_track_session(rate: u32, clips: Vec<DawClip>) -> DawSession {
        let mut session = DawSession::new("Test", rate, 24);
        let mut track = DawTrack::new(1, "Host", DawTrackType::Audio);
        track.clips = clips;
        session.tracks.push(track);
        session
    }

    fn sample_is_clean(buf: &AudioSampleBuffer) -> bool {
        (0..buf.channel_count()).all(|c| buf.channel(c).map(|ch| ch.iter().all(|v| v.is_finite() && v.abs() <= 1.0)).unwrap_or(false))
    }

    #[test]
    fn test_render_uses_real_clip_audio_and_resamples_to_session_rate() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        write_wav(&wav, 44_100, vec![sine(44_100, 1000.0, 1.0, 0.5)]);

        let mut session = single_track_session(48_000, vec![clip(1, 0, 48_000, &wav)]);
        session.tracks[0].pan = -1.0; // mono source, hard left: full level, nothing on the right
        let mut renderer = session.renderer(Some(dir.path())).unwrap();
        let master = renderer.render_master(None).unwrap();

        assert_eq!(master.sample_rate, 48_000);
        assert_eq!(master.samples_per_channel(), 48_000);
        let left = master.channel(0).unwrap();
        let right = master.channel(1).unwrap();
        // 0.5 amplitude sine => RMS 0.3535, and the declick fades only touch the first/last 5 ms.
        assert!((rms(left) - 0.3535).abs() < 0.01, "rms {}", rms(left));
        assert!(rms(right) < 1e-6);
        // 1 kHz must stay 1 kHz after 44.1k -> 48k conversion.
        let crossings = zero_crossings(left);
        assert!((995..=1005).contains(&crossings), "crossings {crossings}");
        assert!(sample_is_clean(&master));
    }

    #[test]
    fn test_source_in_clip_gain_and_track_gain_are_applied() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("step.wav");
        let mut plane = vec![0.1f32; 24_000];
        plane.extend(vec![0.4f32; 24_000]);
        write_wav(&wav, 48_000, vec![plane.clone(), plane]);

        let mut c = clip(1, 12_000, 24_000, &wav);
        c.source_in_samples = 24_000; // skip the 0.1 half
        c.gain_db = -6.0;
        let mut session = single_track_session(48_000, vec![c]);
        session.tracks[0].volume_db = -6.0;
        let mut renderer = session.renderer(Some(dir.path())).unwrap();
        let (_, stem) = renderer.render_stem(0).unwrap();

        let left = stem.channel(0).unwrap();
        let expected = 0.4 * craft_dsp::db_to_gain(-12.0);
        assert_eq!(stem.samples_per_channel(), 36_000);
        assert!(left[..12_000].iter().all(|v| *v == 0.0), "audio before the clip start must be silent");
        assert!((left[24_000] - expected).abs() < 1e-4, "{} vs {expected}", left[24_000]);
        assert_eq!(left[0], 0.0);
    }

    #[test]
    fn test_overlapping_tracks_sum_mute_and_solo_are_honoured() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("dc.wav");
        write_wav(&wav, 48_000, vec![vec![0.2; 48_000], vec![0.2; 48_000]]);

        let mut session = DawSession::new("Mix", 48_000, 24);
        for id in 1..=3u32 {
            let mut t = DawTrack::new(id, format!("T{id}"), DawTrackType::Audio);
            t.clips.push(clip(id, 0, 48_000, &wav));
            session.tracks.push(t);
        }
        let mid = |buf: &AudioSampleBuffer| buf.channel(0).unwrap()[24_000];

        let all = session.renderer(None).unwrap().render_master(None).unwrap();
        assert!((mid(&all) - 0.6).abs() < 1e-4);

        session.tracks[1].muted = true;
        assert!((mid(&session.renderer(None).unwrap().render_master(None).unwrap()) - 0.4).abs() < 1e-4);

        session.tracks[2].solo = true;
        assert!((mid(&session.renderer(None).unwrap().render_master(None).unwrap()) - 0.2).abs() < 1e-4);
    }

    #[test]
    fn test_master_never_clips_even_when_tracks_sum_past_full_scale() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("loud.wav");
        write_wav(&wav, 48_000, vec![vec![0.9; 48_000], vec![0.9; 48_000]]);

        let mut session = DawSession::new("Loud", 48_000, 24);
        for id in 1..=3u32 {
            let mut t = DawTrack::new(id, format!("T{id}"), DawTrackType::Audio);
            t.volume_db = 12.0;
            t.clips.push(clip(id, 0, 48_000, &wav));
            session.tracks.push(t);
        }
        let mut renderer = session.renderer(None).unwrap();
        assert!(sample_is_clean(&renderer.render_master(None).unwrap()));
        assert!(sample_is_clean(&renderer.render_stem(0).unwrap().1));
    }

    #[test]
    fn test_master_loudness_normalization_reaches_target() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("voice.wav");
        write_wav(&wav, 48_000, vec![sine(48_000, 997.0, 6.0, 0.05), sine(48_000, 997.0, 6.0, 0.05)]);

        let session = single_track_session(48_000, vec![clip(1, 0, 288_000, &wav)]);
        let master = session.renderer(None).unwrap().render_master(Some(-23.0)).unwrap();
        let measured = crate::loudness::measure_loudness(&master).unwrap();
        assert!((measured.integrated_lufs + 23.0).abs() < 0.5, "integrated {}", measured.integrated_lufs);
        assert!(measured.true_peak_dbtp <= -0.9);
    }

    #[test]
    fn test_mono_track_yields_mono_stem_and_surround_source_folds_to_stereo() {
        let dir = tempfile::tempdir().unwrap();
        let wav51 = dir.path().join("surround.wav");
        let mut planes = vec![vec![0.0f32; 4_800]; 6];
        planes[2] = vec![0.3; 4_800]; // centre only
        write_wav(&wav51, 48_000, planes);

        let mut session = single_track_session(48_000, vec![clip(1, 0, 4_800, &wav51)]);
        session.tracks[0].channel_layout = ChannelLayout::Mono;
        let (name, stem) = session.renderer(None).unwrap().render_stem(0).unwrap();
        assert_eq!(name, "01_Host");
        assert_eq!(stem.channel_count(), 1);
        assert!(rms(stem.channel(0).unwrap()) > 0.05, "centre channel must survive the downmix");
    }

    #[test]
    fn test_missing_or_undecodable_sources_are_errors_not_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.wav");
        let session = single_track_session(48_000, vec![clip(1, 0, 48_000, &missing)]);
        assert!(session.renderer(None).unwrap().render_master(None).is_err());

        let junk = dir.path().join("junk.wav");
        std::fs::write(&junk, b"this is not audio at all").unwrap();
        let session = single_track_session(48_000, vec![clip(1, 0, 48_000, &junk)]);
        assert!(session.renderer(None).unwrap().render_master(None).is_err());

        let mut no_path = clip(1, 0, 48_000, &missing);
        no_path.source_path = None;
        let session = single_track_session(48_000, vec![no_path]);
        assert!(session.renderer(None).unwrap().render_stem(0).is_err());

        assert!(DawSession::new("Empty", 48_000, 24).renderer(None).is_err());
    }

    #[test]
    fn test_relative_and_foreign_absolute_paths_resolve_next_to_the_session() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("take.wav"), 48_000, vec![vec![0.25; 4_800]]);

        let mut session = single_track_session(48_000, vec![clip(1, 0, 4_800, Path::new("take.wav"))]);
        assert!(session.renderer(Some(dir.path())).unwrap().render_master(None).is_ok());

        session.tracks[0].clips[0].source_path = Some("C:\\Users\\someone\\Sessions\\take.wav".into());
        assert!(session.renderer(Some(dir.path())).unwrap().render_master(None).is_ok());
        assert!(session.renderer(None).unwrap().render_master(None).is_err());
    }

    #[test]
    fn test_sesx_file_ids_resolve_to_media_paths() {
        let xml = r#"<sesx audioRate="48000"><files><file id="7" relativePath="vox.wav" absolutePath="/old/vox.wav"/></files>
            <tracks><audioTrack name="Vox"><audioClip name="a" startPoint="100" endPoint="4900" sourceInPoint="50" fileID="7"/></audioTrack></tracks></sesx>"#;
        let session = DawSession::from_sesx_xml(xml).unwrap();
        let clip = &session.tracks[0].clips[0];
        assert_eq!(clip.source_path.as_deref(), Some("/old/vox.wav"));
        assert_eq!((clip.start_samples, clip.length_samples, clip.source_in_samples), (100, 4_800, 50));
    }

    #[test]
    fn test_sesx_paths_with_xml_entities_round_trip() {
        let mut session = single_track_session(48_000, vec![clip(1, 0, 10, Path::new("/a/B&B <take> \"1\".wav"))]);
        session.tracks[0].name = "Lead & Backing".into();
        let parsed = DawSession::from_sesx_xml(&session.to_sesx_xml()).unwrap();
        assert_eq!(parsed.tracks[0].clips[0].source_path.as_deref(), Some("/a/B&B <take> \"1\".wav"));
        assert_eq!(parsed.tracks[0].name, "Lead & Backing");
    }

    #[test]
    fn test_hostile_sesx_values_never_produce_nan_or_clipping() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        write_wav(&wav, 48_000, vec![vec![0.9; 48_000], vec![0.9; 48_000]]);
        let path = wav.to_string_lossy();
        let xml = format!(
            r#"<sesx audioRate="0" bitDepth="24"><masterTrack volume="NaN" pan="inf"/>
            <audioTrack name="A" volume="NaN" pan="NaN"><audioClip name="c" startPoint="0" endPoint="48000" gain="inf"><filePath>{path}</filePath></audioClip></audioTrack>
            <audioTrack name="B" volume="1000" pan="-inf"><audioClip name="d" startPoint="0" endPoint="48000" gain="1000"><filePath>{path}</filePath></audioClip></audioTrack></sesx>"#
        );
        let session = DawSession::from_sesx_xml(&xml).unwrap();
        assert_eq!(session.sample_rate, 48_000);
        assert!(session.master_volume_db.is_finite() && session.master_pan.is_finite());
        let mut renderer = session.renderer(None).unwrap();
        for idx in renderer.stem_track_indices() {
            assert!(sample_is_clean(&renderer.render_stem(idx).unwrap().1));
        }
        assert!(sample_is_clean(&renderer.render_master(Some(-23.0)).unwrap()));
    }

    #[test]
    fn test_oversized_session_render_is_an_error() {
        let mut session = DawSession::new("Huge", 48_000, 24);
        let mut t = DawTrack::new(1, "T", DawTrackType::Audio);
        t.clips.push(DawClip {
            id: 1,
            name: "c".into(),
            start_samples: u64::MAX - 10,
            length_samples: u64::MAX,
            source_in_samples: 0,
            source_path: None,
            gain_db: 0.0,
        });
        session.tracks.push(t);
        assert!(session.renderer(None).is_err());
    }

    #[test]
    fn test_soundcraft_decompression_bomb_is_rejected() {
        let bomb = miniz_oxide::deflate::compress_to_vec_zlib(&vec![b' '; MAX_DECOMPRESSED_SESSION_SIZE + 1024], 9);
        let mut data = b"\x89CRAFT\x00\x01S".to_vec();
        data.extend_from_slice(&(bomb.len() as u32).to_be_bytes());
        data.extend_from_slice(&bomb);
        assert!(DawSession::from_soundcraft_binary(&data).is_err());
    }

    #[test]
    fn test_truncated_and_garbage_inputs_are_errors() {
        assert!(DawSession::from_soundcraft_binary(b"\x89CRAFT\x00\x01S\xff\xff\xff\xff").is_err());
        assert!(DawSession::from_soundcraft_binary(b"short").is_err());
        let _ = DawSession::from_sesx_xml("<sesx <<< \u{1F600}\u{0}");
    }
}
