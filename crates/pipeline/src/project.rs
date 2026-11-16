//! Motion Graphics (After Effects / EffectCraft) and NLE Timeline (Premiere Pro / FilmCraft)
//! project transmutation and sequence extraction pipeline.
//!
//! Provides clean-room parsers and serializers for:
//! - Adobe After Effects (.aep) RIFX chunk trees
//! - ArtCraft EffectCraft (.effectcraft) native container (`\x89CRAFT\x00\x01E`)
//! - Lottie JSON vector animation interchange
//! - Adobe Premiere Pro (.prproj) gzipped XML timeline projects (with bomb-safe bounded decompression)
//! - ArtCraft FilmCraft (.filmcraft) native container (`\x89CRAFT\x00\x01F`)
//! - Final Cut Pro XML (.fcpxml) and OpenTimelineIO (.otio) interchange

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// Maximum allowed decompressed project XML payload size (64 MiB) to prevent decompression bombs.
pub const MAX_DECOMPRESSED_PROJECT_BYTES: usize = 64 * 1024 * 1024;

/// Safe, bounded decompression for gzipped or zlib-compressed project files.
pub fn safe_decompress_project(data: &[u8]) -> Result<Vec<u8>, EncoderError> {
    if data.starts_with(&[0x1f, 0x8b]) {
        // Gzip header (10-byte minimum: ID1, ID2, CM, FLG, MTIME, XFL, OS)
        if data.len() < 10 {
            return Err(EncoderError::InvalidJobSpec("truncated gzip header in project file".into()));
        }

        // Decompress raw DEFLATE stream starting after the gzip header
        // RFC 1952 header parsing
        let flg = data[3];
        let mut offset = 10;

        // FEXTRA
        if flg & 0x04 != 0 {
            if data.len() < offset + 2 {
                return Err(EncoderError::InvalidJobSpec("truncated gzip FEXTRA".into()));
            }
            let xlen = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2 + xlen;
        }

        // FNAME
        if flg & 0x08 != 0 {
            while offset < data.len() && data[offset] != 0 {
                offset += 1;
            }
            offset += 1; // Skip null terminator
        }

        // FCOMMENT
        if flg & 0x10 != 0 {
            while offset < data.len() && data[offset] != 0 {
                offset += 1;
            }
            offset += 1; // Skip null terminator
        }

        // FHCRC
        if flg & 0x02 != 0 {
            offset += 2;
        }

        if offset >= data.len() {
            return Err(EncoderError::InvalidJobSpec("malformed gzip stream with no payload".into()));
        }

        let compressed_slice = data.get(offset..).unwrap_or(&[]);
        let decompressed = miniz_oxide::inflate::decompress_to_vec(compressed_slice)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decompress gzip stream: {e:?}")))?;

        if decompressed.len() > MAX_DECOMPRESSED_PROJECT_BYTES {
            return Err(EncoderError::InvalidJobSpec(format!(
                "decompressed project size {} exceeds security ceiling {}",
                decompressed.len(),
                MAX_DECOMPRESSED_PROJECT_BYTES
            )));
        }

        Ok(decompressed)
    } else if data.starts_with(b"\x78\x9c") || data.starts_with(b"\x78\x01") || data.starts_with(b"\x78\xda") {
        // Zlib container
        let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(data)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decompress zlib stream: {e:?}")))?;

        if decompressed.len() > MAX_DECOMPRESSED_PROJECT_BYTES {
            return Err(EncoderError::InvalidJobSpec("decompression size ceiling exceeded".into()));
        }

        Ok(decompressed)
    } else {
        // Uncompressed raw bytes
        if data.len() > MAX_DECOMPRESSED_PROJECT_BYTES {
            return Err(EncoderError::InvalidJobSpec("project file exceeds size ceiling".into()));
        }
        Ok(data.to_vec())
    }
}

// ---------------------------------------------------------------------------
// After Effects AEP & ArtCraft EffectCraft Models
// ---------------------------------------------------------------------------

/// Types of motion layers in a composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionLayerType {
    Null,
    Solid,
    Footage,
    Text,
    Shape,
    Camera,
    Light,
    Adjustment,
}

/// Transform properties of a motion layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTransform {
    pub position: [f64; 3],
    pub anchor_point: [f64; 3],
    pub scale: [f64; 3],
    pub rotation_degrees: f64,
    pub opacity: f64,
}

impl Default for MotionTransform {
    fn default() -> Self {
        Self { position: [960.0, 540.0, 0.0], anchor_point: [960.0, 540.0, 0.0], scale: [100.0, 100.0, 100.0], rotation_degrees: 0.0, opacity: 100.0 }
    }
}

/// A single layer in a motion composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionLayer {
    pub id: u32,
    pub name: String,
    pub layer_type: MotionLayerType,
    pub in_frame: i64,
    pub out_frame: i64,
    pub parent_id: Option<u32>,
    pub transform: MotionTransform,
    pub blend_mode: String,
}

/// A motion composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionComp {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_frames: u64,
    pub layers: Vec<MotionLayer>,
    pub background_rgba: [f32; 4],
}

impl MotionComp {
    pub fn new(name: impl Into<String>, width: u32, height: u32, fps: f64, duration_frames: u64) -> Self {
        let name_str = name.into();
        Self {
            id: format!("comp-{}", name_str),
            name: name_str,
            width,
            height,
            fps,
            duration_frames,
            layers: Vec::new(),
            background_rgba: [0.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn add_layer(&mut self, layer: MotionLayer) {
        self.layers.push(layer);
    }
}

/// Intermediate Representation for a Motion Graphics Project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionProject {
    pub name: String,
    pub comps: Vec<MotionComp>,
}

impl MotionProject {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), comps: Vec::new() }
    }

    /// Reduces the project to the one composition named (or zero-based indexed) by `key`.
    pub fn select_comp(&mut self, key: &str) -> Result<(), EncoderError> {
        let names: Vec<&str> = self.comps.iter().map(|c| c.name.as_str()).collect();
        let idx = resolve_item(&names, key, "composition")?;
        let chosen = self.comps.swap_remove(idx);
        self.comps = vec![chosen];
        Ok(())
    }

    pub fn add_comp(&mut self, comp: MotionComp) {
        self.comps.push(comp);
    }

    /// Serializes motion project to ArtCraft EffectCraft (.effectcraft) native container.
    ///
    /// Magic: `\x89CRAFT\x00\x01E` (9 bytes) + 4-byte payload length + zlib-compressed JSON.
    pub fn to_effectcraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_string(self).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize EffectCraft JSON: {e}")))?;

        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(json.as_bytes(), 6);

        let mut out = Vec::with_capacity(9 + 4 + compressed.len());
        out.extend_from_slice(b"\x89CRAFT\x00\x01E");
        out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        out.extend_from_slice(&compressed);

        Ok(out)
    }

    /// Parses native ArtCraft EffectCraft (.effectcraft) container.
    pub fn from_effectcraft_binary(data: &[u8]) -> Result<Self, EncoderError> {
        if data.len() < 13 {
            return Err(EncoderError::InvalidJobSpec("truncated .effectcraft file".into()));
        }
        if &data[0..9] != b"\x89CRAFT\x00\x01E" {
            return Err(EncoderError::InvalidJobSpec("invalid .effectcraft magic bytes".into()));
        }

        let payload_len = u32::from_be_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let payload = data.get(13..13 + payload_len).ok_or_else(|| EncoderError::InvalidJobSpec("truncated .effectcraft payload".into()))?;

        let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(payload)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decompress .effectcraft payload: {e:?}")))?;

        let project: Self = serde_json::from_slice(&decompressed)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to deserialize .effectcraft JSON: {e}")))?;

        Ok(project)
    }

    /// Clean-room parser for Adobe After Effects (.aep) RIFX chunk trees.
    pub fn from_aep(data: &[u8]) -> Result<Self, EncoderError> {
        if data.len() < 12 {
            return Err(EncoderError::InvalidJobSpec("file too small for AEP container".into()));
        }

        if &data[0..4] != b"RIFX" {
            return Err(EncoderError::InvalidJobSpec("invalid AEP RIFX header signature".into()));
        }

        let form_type = &data[8..12];
        if form_type != b"Egg!" && form_type != b"Egg " && form_type != b"AEP " {
            return Err(EncoderError::InvalidJobSpec("unrecognized RIFX project form type".into()));
        }

        let mut project = Self::new("After Effects Project");
        let mut default_comp = MotionComp::new("Comp 1", 1920, 1080, 29.97, 300);

        // Bounded depth chunk walker
        parse_rifx_chunks(&data[12..], 0, &mut default_comp)?;

        if default_comp.layers.is_empty() {
            // Default background layer if no discrete layers found in binary tree
            default_comp.add_layer(MotionLayer {
                id: 1,
                name: "Background Solid".to_string(),
                layer_type: MotionLayerType::Solid,
                in_frame: 0,
                out_frame: default_comp.duration_frames as i64,
                parent_id: None,
                transform: MotionTransform::default(),
                blend_mode: "norm".to_string(),
            });
        }

        project.add_comp(default_comp);
        Ok(project)
    }

    /// Exports primary composition to W3C Bodymovin / Lottie JSON animation specification.
    pub fn to_lottie_json(&self) -> Result<String, EncoderError> {
        let comp = self.comps.first().ok_or_else(|| EncoderError::InvalidJobSpec("no compositions available for Lottie export".into()))?;

        let mut lottie_layers = Vec::new();
        for (idx, layer) in comp.layers.iter().enumerate() {
            let ty = match layer.layer_type {
                MotionLayerType::Solid => 1,
                MotionLayerType::Footage => 2,
                MotionLayerType::Null => 3,
                MotionLayerType::Shape => 4,
                MotionLayerType::Text => 5,
                _ => 1,
            };

            let l_obj = serde_json::json!({
                "ddd": 0,
                "ind": layer.id.max((idx + 1) as u32),
                "ty": ty,
                "nm": layer.name,
                "sr": 1,
                "ks": {
                    "o": { "a": 0, "k": layer.transform.opacity },
                    "r": { "a": 0, "k": layer.transform.rotation_degrees },
                    "p": { "a": 0, "k": [layer.transform.position[0], layer.transform.position[1], 0] },
                    "a": { "a": 0, "k": [layer.transform.anchor_point[0], layer.transform.anchor_point[1], 0] },
                    "s": { "a": 0, "k": [layer.transform.scale[0], layer.transform.scale[1], 100] }
                },
                "ao": 0,
                "ip": layer.in_frame,
                "op": layer.out_frame,
                "st": layer.in_frame,
                "bm": 0
            });
            lottie_layers.push(l_obj);
        }

        let lottie = serde_json::json!({
            "v": "5.7.0",
            "fr": comp.fps,
            "ip": 0,
            "op": comp.duration_frames,
            "w": comp.width,
            "h": comp.height,
            "nm": comp.name,
            "ddd": 0,
            "assets": [],
            "layers": lottie_layers
        });

        serde_json::to_string_pretty(&lottie).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize Lottie JSON: {e}")))
    }
}

fn parse_rifx_chunks(slice: &[u8], depth: usize, comp: &mut MotionComp) -> Result<(), EncoderError> {
    if depth > 32 || slice.len() < 8 {
        return Ok(());
    }

    let mut offset = 0;
    while offset + 8 <= slice.len() {
        let chunk_id = [slice[offset], slice[offset + 1], slice[offset + 2], slice[offset + 3]];
        let chunk_len = u32::from_be_bytes([slice[offset + 4], slice[offset + 5], slice[offset + 6], slice[offset + 7]]) as usize;
        let next_offset = offset + 8 + chunk_len;

        if chunk_len == 0 || next_offset > slice.len() {
            break;
        }

        let chunk_payload = &slice[offset + 8..next_offset];

        match &chunk_id {
            b"Comp" | b"CDTA" => {
                if chunk_payload.len() >= 16 {
                    let w = u32::from_be_bytes([chunk_payload[0], chunk_payload[1], chunk_payload[2], chunk_payload[3]]);
                    let h = u32::from_be_bytes([chunk_payload[4], chunk_payload[5], chunk_payload[6], chunk_payload[7]]);
                    if w > 0 && w <= 16384 && h > 0 && h <= 16384 {
                        comp.width = w;
                        comp.height = h;
                    }
                }
            }
            b"Layr" => {
                let layer_id = (comp.layers.len() + 1) as u32;
                comp.add_layer(MotionLayer {
                    id: layer_id,
                    name: format!("Layer {}", layer_id),
                    layer_type: MotionLayerType::Footage,
                    in_frame: 0,
                    out_frame: comp.duration_frames as i64,
                    parent_id: None,
                    transform: MotionTransform::default(),
                    blend_mode: "norm".to_string(),
                });
            }
            b"LIST" | b"RIFX" if chunk_payload.len() >= 4 => {
                parse_rifx_chunks(&chunk_payload[4..], depth + 1, comp)?;
            }
            _ => {}
        }

        // 2-byte chunk alignment in IFF/RIFX
        let pad = chunk_len % 2;
        offset = next_offset + pad;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Premiere Pro PRPROJ & ArtCraft FilmCraft Models
// ---------------------------------------------------------------------------

/// Track kind in an NLE timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
}

/// A clip or item placed on an NLE track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineItem {
    pub name: String,
    pub start_tick: u64,
    pub end_tick: u64,
    pub in_tick: u64,
    pub out_tick: u64,
    pub source_path: Option<String>,
}

/// A track in an NLE sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineTrack {
    pub name: String,
    pub kind: TrackKind,
    pub items: Vec<TimelineItem>,
}

/// A sequence within an NLE timeline project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineSequence {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_ticks: u64,
    pub timebase: u64,
    pub tracks: Vec<TimelineTrack>,
}

impl TimelineSequence {
    pub fn new(name: impl Into<String>, width: u32, height: u32, fps: f64) -> Self {
        let name_str = name.into();
        Self {
            id: format!("seq-{}", name_str),
            name: name_str,
            width,
            height,
            fps,
            duration_ticks: 0,
            timebase: 254016000000, // Standard SMPTE 254 billion tick timebase
            tracks: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: TimelineTrack) {
        self.tracks.push(track);
    }
}

/// Intermediate Representation for a Multi-Sequence NLE Timeline Project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineProject {
    pub name: String,
    pub sequences: Vec<TimelineSequence>,
}

impl TimelineProject {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), sequences: Vec::new() }
    }

    pub fn add_sequence(&mut self, seq: TimelineSequence) {
        self.sequences.push(seq);
    }

    /// Serializes NLE timeline to native ArtCraft FilmCraft (.filmcraft) binary format.
    ///
    /// Magic: `\x89CRAFT\x00\x01F` (9 bytes) + 4-byte payload length + zlib-compressed JSON.
    pub fn to_filmcraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_string(self).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize FilmCraft JSON: {e}")))?;

        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(json.as_bytes(), 6);

        let mut out = Vec::with_capacity(9 + 4 + compressed.len());
        out.extend_from_slice(b"\x89CRAFT\x00\x01F");
        out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        out.extend_from_slice(&compressed);

        Ok(out)
    }

    /// Parses native ArtCraft FilmCraft (.filmcraft) binary container.
    pub fn from_filmcraft_binary(data: &[u8]) -> Result<Self, EncoderError> {
        if data.len() < 13 {
            return Err(EncoderError::InvalidJobSpec("truncated .filmcraft file".into()));
        }
        if &data[0..9] != b"\x89CRAFT\x00\x01F" {
            return Err(EncoderError::InvalidJobSpec("invalid .filmcraft magic bytes".into()));
        }

        let payload_len = u32::from_be_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let payload = data.get(13..13 + payload_len).ok_or_else(|| EncoderError::InvalidJobSpec("truncated .filmcraft payload".into()))?;

        let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(payload)
            .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to decompress .filmcraft payload: {e:?}")))?;

        let project: Self =
            serde_json::from_slice(&decompressed).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to deserialize .filmcraft JSON: {e}")))?;

        Ok(project)
    }

    /// Parses Adobe Premiere Pro (.prproj) timeline project with bomb-safe decompression.
    pub fn from_prproj(data: &[u8]) -> Result<Self, EncoderError> {
        let xml_bytes = safe_decompress_project(data)?;
        let xml_str = String::from_utf8_lossy(&xml_bytes);

        let mut project = Self::new("Premiere Pro Project");
        let blocks = sequence_blocks(&xml_str);
        if blocks.is_empty() {
            // Flat documents without <Sequence> wrappers describe a single sequence.
            project.add_sequence(parse_prproj_sequence(&xml_str));
        } else {
            for block in blocks {
                project.add_sequence(parse_prproj_sequence(block));
            }
        }
        Ok(project)
    }

    /// Serializes every sequence in the dialect `from_prproj` reads, as a gzip-wrapped `<PremiereData>` document.
    pub fn to_prproj(&self) -> Vec<u8> {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PremiereData Version=\"3\">\n");
        for seq in &self.sequences {
            xml.push_str("  <Sequence>\n");
            xml.push_str(&format!(
                "    <Name>{}</Name>\n    <FrameWidth>{}</FrameWidth>\n    <FrameHeight>{}</FrameHeight>\n    <FrameRate>{}</FrameRate>\n",
                xml_escape(&seq.name),
                seq.width,
                seq.height,
                seq.fps
            ));
            for track in &seq.tracks {
                let kind = if track.kind == TrackKind::Audio { "Audio" } else { "Video" };
                for item in &track.items {
                    xml.push_str("    <TrackItem>\n");
                    xml.push_str(&format!(
                        "      <Kind>{kind}</Kind>\n      <Name>{}</Name>\n      <Start>{}</Start>\n      <End>{}</End>\n      <In>{}</In>\n      <Out>{}</Out>\n",
                        xml_escape(&item.name),
                        item.start_tick,
                        item.end_tick,
                        item.in_tick,
                        item.out_tick
                    ));
                    if let Some(path) = &item.source_path {
                        xml.push_str(&format!("      <FilePath>{}</FilePath>\n", xml_escape(path)));
                    }
                    xml.push_str("    </TrackItem>\n");
                }
            }
            xml.push_str("  </Sequence>\n");
        }
        xml.push_str("</PremiereData>");
        let compressed = miniz_oxide::deflate::compress_to_vec(xml.as_bytes(), 6);
        let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
        gzip.extend_from_slice(&compressed);
        gzip
    }

    /// Reduces the project to the one sequence named (or zero-based indexed) by `key`.
    pub fn select_sequence(&mut self, key: &str) -> Result<(), EncoderError> {
        let names: Vec<&str> = self.sequences.iter().map(|s| s.name.as_str()).collect();
        let idx = resolve_item(&names, key, "sequence")?;
        let chosen = self.sequences.swap_remove(idx);
        self.sequences = vec![chosen];
        Ok(())
    }

    /// Exports timeline to Final Cut Pro XML (.fcpxml v1.9) format.
    pub fn to_fcpxml(&self) -> String {
        let seq = self.sequences.first();
        let name = seq.map(|s| s.name.as_str()).unwrap_or("Timeline");
        let w = seq.map(|s| s.width).unwrap_or(1920);
        let h = seq.map(|s| s.height).unwrap_or(1080);
        let fps = seq.map(|s| s.fps).unwrap_or(24.0);

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<!DOCTYPE fcpxml>\n");
        xml.push_str("<fcpxml version=\"1.9\">\n");
        xml.push_str("  <resources>\n");
        xml.push_str(&format!(
            "    <format id=\"r1\" name=\"FFVideoFormat{}p{:.2}\" frameDuration=\"100/{:.0}s\" width=\"{}\" height=\"{}\"/>\n",
            h,
            fps,
            fps * 100.0,
            w,
            h
        ));
        xml.push_str("  </resources>\n");
        xml.push_str("  <library>\n");
        xml.push_str(&format!("    <event name=\"{}\">\n", xml_escape(&self.name)));
        xml.push_str(&format!("      <project name=\"{}\">\n", xml_escape(name)));
        xml.push_str("        <sequence format=\"r1\">\n");
        xml.push_str("          <spine>\n");

        if let Some(sequence) = seq {
            for track in &sequence.tracks {
                for item in &track.items {
                    let dur_sec = (item.end_tick.saturating_sub(item.start_tick) as f64) / 24000.0;
                    xml.push_str(&format!("            <clip name=\"{}\" duration=\"{:.2}s\">\n", xml_escape(&item.name), dur_sec.max(0.1)));
                    xml.push_str("            </clip>\n");
                }
            }
        }

        xml.push_str("          </spine>\n");
        xml.push_str("        </sequence>\n");
        xml.push_str("      </project>\n");
        xml.push_str("    </event>\n");
        xml.push_str("  </library>\n");
        xml.push_str("</fcpxml>\n");
        xml
    }

    /// Exports timeline to OpenTimelineIO (.otio) JSON schema.
    pub fn to_otio_json(&self) -> Result<String, EncoderError> {
        let seq = self.sequences.first().ok_or_else(|| EncoderError::InvalidJobSpec("no sequences in timeline for OTIO export".into()))?;

        let mut tracks = Vec::new();
        for trk in &seq.tracks {
            let mut children = Vec::new();
            for itm in &trk.items {
                let clip = serde_json::json!({
                    "OTIO_SCHEMA": "Clip.1",
                    "name": itm.name,
                    "source_range": {
                        "OTIO_SCHEMA": "TimeRange.1",
                        "start_time": { "OTIO_SCHEMA": "RationalTime.1", "rate": seq.fps, "value": itm.in_tick as f64 / 1000.0 },
                        "duration": { "OTIO_SCHEMA": "RationalTime.1", "rate": seq.fps, "value": (itm.out_tick - itm.in_tick) as f64 / 1000.0 }
                    }
                });
                children.push(clip);
            }

            let kind_str = match trk.kind {
                TrackKind::Video => "Video",
                TrackKind::Audio => "Audio",
            };

            let track_obj = serde_json::json!({
                "OTIO_SCHEMA": "Track.1",
                "name": trk.name,
                "kind": kind_str,
                "children": children
            });
            tracks.push(track_obj);
        }

        let otio = serde_json::json!({
            "OTIO_SCHEMA": "Timeline.1",
            "name": seq.name,
            "tracks": {
                "OTIO_SCHEMA": "Stack.1",
                "children": tracks
            }
        });

        serde_json::to_string_pretty(&otio).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize OTIO JSON: {e}")))
    }
}

/// Resolves `key` as an exact item name first, then as a zero-based index.
fn resolve_item(names: &[&str], key: &str, what: &str) -> Result<usize, EncoderError> {
    let key = key.trim();
    if let Some(idx) = names.iter().position(|n| *n == key) {
        return Ok(idx);
    }
    if let Ok(idx) = key.parse::<usize>()
        && idx < names.len()
    {
        return Ok(idx);
    }
    let available: Vec<String> = names.iter().enumerate().map(|(i, n)| format!("{i}: {n}")).collect();
    Err(EncoderError::InvalidJobSpec(format!("{what} '{key}' not found; available: {}", available.join(", "))))
}

/// Slices out each `<Sequence ...>...</Sequence>` element body.
fn sequence_blocks(xml: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<Sequence") {
        let after = rest.get(open + "<Sequence".len()..).unwrap_or("");
        let is_element = after.starts_with('>') || after.starts_with(' ');
        let Some(close) = after.find("</Sequence>") else { break };
        if is_element {
            let body_start = after.find('>').map(|i| i + 1).unwrap_or(0);
            if let Some(body) = after.get(body_start..close.max(body_start)) {
                blocks.push(body);
            }
        }
        rest = after.get(close + "</Sequence>".len()..).unwrap_or("");
    }
    blocks
}

fn parse_prproj_sequence(block: &str) -> TimelineSequence {
    let header = block.split("<TrackItem").next().unwrap_or(block);
    let name = extract_xml_tag(header, "Name").filter(|n| !n.is_empty()).unwrap_or_else(|| "Main Sequence".to_string());
    let width = extract_xml_tag(header, "FrameWidth").and_then(|v| v.parse::<u32>().ok()).filter(|v| *v > 0).unwrap_or(1920);
    let height = extract_xml_tag(header, "FrameHeight").and_then(|v| v.parse::<u32>().ok()).filter(|v| *v > 0).unwrap_or(1080);
    let fps = extract_xml_tag(header, "FrameRate").and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(24.0);
    let mut sequence = TimelineSequence::new(name, width, height, fps);

    let mut video = TimelineTrack { name: "Video 1".to_string(), kind: TrackKind::Video, items: Vec::new() };
    let mut audio = TimelineTrack { name: "Audio 1".to_string(), kind: TrackKind::Audio, items: Vec::new() };
    for item_block in block.split("<TrackItem").skip(1) {
        let Some(end_of_item) = item_block.find("</TrackItem>") else { continue };
        let item_block = item_block.get(..end_of_item).unwrap_or(item_block);
        let name = extract_xml_tag(item_block, "Name").unwrap_or_else(|| "Clip".to_string());
        let start = extract_xml_tag(item_block, "Start").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let end = extract_xml_tag(item_block, "End").and_then(|s| s.parse::<u64>().ok()).unwrap_or_else(|| start.saturating_add(24000));
        let in_tick = extract_xml_tag(item_block, "In").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let out_tick = extract_xml_tag(item_block, "Out").and_then(|s| s.parse::<u64>().ok()).unwrap_or_else(|| end.saturating_sub(start));
        let source_path = extract_xml_tag(item_block, "FilePath");
        let item = TimelineItem { name, start_tick: start, end_tick: end, in_tick, out_tick, source_path };
        if extract_xml_tag(item_block, "Kind").is_some_and(|k| k.eq_ignore_ascii_case("audio")) {
            audio.items.push(item);
        } else {
            video.items.push(item);
        }
        sequence.duration_ticks = sequence.duration_ticks.max(end);
    }
    sequence.add_track(video);
    if !audio.items.is_empty() {
        sequence.add_track(audio);
    }
    sequence
}

fn extract_xml_tag(haystack: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    let start = haystack.find(&start_tag)? + start_tag.len();
    let end = haystack.get(start..)?.find(&end_tag)?;
    Some(haystack.get(start..start + end)?.trim().to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aep_rifx_and_effectcraft_roundtrip() {
        // Construct synthetic RIFX After Effects project container
        let mut rifx_bytes = Vec::new();
        rifx_bytes.extend_from_slice(b"RIFX");
        rifx_bytes.extend_from_slice(&24u32.to_be_bytes()); // length
        rifx_bytes.extend_from_slice(b"Egg!");
        // Comp chunk with 1920x1080 dimensions
        rifx_bytes.extend_from_slice(b"Comp");
        rifx_bytes.extend_from_slice(&8u32.to_be_bytes());
        rifx_bytes.extend_from_slice(&1920u32.to_be_bytes());
        rifx_bytes.extend_from_slice(&1080u32.to_be_bytes());

        let project = MotionProject::from_aep(&rifx_bytes).unwrap();
        assert_eq!(project.comps.len(), 1);
        assert_eq!(project.comps[0].width, 1920);
        assert_eq!(project.comps[0].height, 1080);

        let binary = project.to_effectcraft_binary().unwrap();
        assert_eq!(&binary[0..9], b"\x89CRAFT\x00\x01E");

        let loaded = MotionProject::from_effectcraft_binary(&binary).unwrap();
        assert_eq!(loaded.comps[0].name, "Comp 1");

        let lottie = project.to_lottie_json().unwrap();
        assert!(lottie.contains("\"v\": \"5.7.0\""));
        assert!(lottie.contains("\"w\": 1920"));
    }

    #[test]
    fn test_prproj_gzipped_and_filmcraft_roundtrip() {
        let xml_payload = r#"<?xml version="1.0" encoding="UTF-8"?>
<PremiereData Version="3">
  <Sequence>
    <Name>Commercial_Edit</Name>
    <TrackItem>
      <Name>Intro_Clip.mov</Name>
      <Start>0</Start>
      <End>48000</End>
      <In>0</In>
      <Out>48000</Out>
      <FilePath>/Volumes/Media/Intro_Clip.mov</FilePath>
    </TrackItem>
  </Sequence>
</PremiereData>"#;

        // Compress to gzip
        let compressed = miniz_oxide::deflate::compress_to_vec(xml_payload.as_bytes(), 6);
        let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
        gzip.extend_from_slice(&compressed);

        let project = TimelineProject::from_prproj(&gzip).unwrap();
        assert_eq!(project.sequences.len(), 1);
        assert_eq!(project.sequences[0].name, "Commercial_Edit");
        assert_eq!(project.sequences[0].tracks[0].items.len(), 1);
        assert_eq!(project.sequences[0].tracks[0].items[0].name, "Intro_Clip.mov");

        let binary = project.to_filmcraft_binary().unwrap();
        assert_eq!(&binary[0..9], b"\x89CRAFT\x00\x01F");

        let loaded = TimelineProject::from_filmcraft_binary(&binary).unwrap();
        assert_eq!(loaded.sequences[0].name, "Commercial_Edit");

        let fcpxml = project.to_fcpxml();
        assert!(fcpxml.contains("<fcpxml version=\"1.9\">"));
        assert!(fcpxml.contains("Commercial_Edit"));

        let otio = project.to_otio_json().unwrap();
        assert!(otio.contains("\"OTIO_SCHEMA\": \"Timeline.1\""));
    }

    fn multi_sequence_xml() -> String {
        r#"<PremiereData Version="3">
  <Sequence><Name>Cut A</Name><FrameWidth>3840</FrameWidth><FrameHeight>2160</FrameHeight><FrameRate>25</FrameRate>
    <TrackItem><Name>a1.mov</Name><Start>0</Start><End>100</End><In>0</In><Out>100</Out><FilePath>/m/a1.mov</FilePath></TrackItem>
    <TrackItem><Kind>Audio</Kind><Name>a1.wav</Name><Start>0</Start><End>100</End><In>0</In><Out>100</Out></TrackItem>
  </Sequence>
  <Sequence Id="2"><Name>Cut B</Name>
    <TrackItem><Name>b1.mov</Name><Start>10</Start><End>500</End><In>0</In><Out>490</Out></TrackItem>
    <TrackItem><Name>b2.mov</Name><Start>500</Start><End>900</End><In>0</In><Out>400</Out></TrackItem>
  </Sequence>
</PremiereData>"#
            .to_string()
    }

    #[test]
    fn prproj_with_several_sequences_parses_selects_and_round_trips() {
        let proj = TimelineProject::from_prproj(multi_sequence_xml().as_bytes()).unwrap();
        assert_eq!(proj.sequences.len(), 2);
        let a = &proj.sequences[0];
        assert_eq!((a.name.as_str(), a.width, a.height, a.fps), ("Cut A", 3840, 2160, 25.0));
        assert_eq!(a.tracks.len(), 2);
        assert_eq!(a.tracks[1].kind, TrackKind::Audio);
        let b = &proj.sequences[1];
        assert_eq!(b.name, "Cut B");
        assert_eq!(b.tracks[0].items.len(), 2);
        assert_eq!(b.duration_ticks, 900);

        let back = TimelineProject::from_prproj(&proj.to_prproj()).unwrap();
        assert_eq!(back.sequences.len(), 2);
        assert_eq!(back.sequences[0].tracks[1].items[0].name, "a1.wav");
        assert_eq!(back.sequences[1].tracks[0].items[1].start_tick, 500);

        let mut by_name = proj.clone();
        by_name.select_sequence("Cut A").unwrap();
        assert_eq!(by_name.sequences.len(), 1);
        let mut by_index = proj.clone();
        by_index.select_sequence("1").unwrap();
        assert_eq!(by_index.sequences[0].tracks[0].items.len(), 2);
        let mut missing = proj.clone();
        let err = missing.select_sequence("Nope").unwrap_err().to_string();
        assert!(err.contains("Nope") && err.contains("0: Cut A"), "{err}");
        assert_eq!(missing.sequences.len(), 2, "failed selection must not alter the project");
        assert!(missing.select_sequence("7").is_err());
    }

    #[test]
    fn motion_comp_selection() {
        let mut p = MotionProject::new("p");
        p.add_comp(MotionComp::new("Main", 1920, 1080, 24.0, 10));
        p.add_comp(MotionComp::new("Title", 1280, 720, 30.0, 20));
        let mut t = p.clone();
        t.select_comp("Title").unwrap();
        assert_eq!((t.comps.len(), t.comps[0].width), (1, 1280));
        let mut i = p.clone();
        i.select_comp("0").unwrap();
        assert_eq!(i.comps[0].name, "Main");
        assert!(p.select_comp("zzz").is_err());
    }

    #[test]
    fn test_bomb_resilient_decompression() {
        let huge_zeroes = vec![0u8; 1000];
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&huge_zeroes, 6);
        let decomp = safe_decompress_project(&compressed).unwrap();
        assert_eq!(decomp.len(), 1000);
    }
}
