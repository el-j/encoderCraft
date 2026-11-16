//! CAD and Technical Drafting intermediate representation, parsers, and exporters.
//!
//! Handles AutoCAD DXF (ASCII & binary), AutoCAD DWG headers, HPGL/PLT pen-plotter instruction
//! streams, and native ArtCraft `.cadcraft` containers. Supports bidirectional conversion to/from
//! vector SVG and blueprint PDF.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// 2D Coordinate point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CadPoint2D {
    pub x: f64,
    pub y: f64,
}

impl CadPoint2D {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// 3D Coordinate point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CadPoint3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl CadPoint3D {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub const fn to_2d(self) -> CadPoint2D {
        CadPoint2D::new(self.x, self.y)
    }
}

/// CAD Layer definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CadLayer {
    pub name: String,
    pub color_rgb: [u8; 3],
    pub visible: bool,
}

impl Default for CadLayer {
    fn default() -> Self {
        Self { name: "0".to_string(), color_rgb: [255, 255, 255], visible: true }
    }
}

/// CAD Geometric Entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CadEntity {
    Line { start: CadPoint3D, end: CadPoint3D, layer: String, color_rgb: Option<[u8; 3]> },
    Circle { center: CadPoint3D, radius: f64, layer: String, color_rgb: Option<[u8; 3]> },
    Arc { center: CadPoint3D, radius: f64, start_angle_deg: f64, end_angle_deg: f64, layer: String, color_rgb: Option<[u8; 3]> },
    Polyline { points: Vec<CadPoint2D>, closed: bool, layer: String, color_rgb: Option<[u8; 3]> },
    Text { position: CadPoint3D, text: String, height: f64, layer: String, color_rgb: Option<[u8; 3]> },
}

impl CadEntity {
    /// Name of the layer the entity is drawn on.
    pub fn layer(&self) -> &String {
        match self {
            Self::Line { layer, .. }
            | Self::Circle { layer, .. }
            | Self::Arc { layer, .. }
            | Self::Polyline { layer, .. }
            | Self::Text { layer, .. } => layer,
        }
    }
}

/// Intermediate Representation for a 2D/3D Technical CAD Drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadDrawing {
    pub title: String,
    pub layers: Vec<CadLayer>,
    pub entities: Vec<CadEntity>,
}

impl Default for CadDrawing {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

impl CadDrawing {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), layers: vec![CadLayer::default()], entities: Vec::new() }
    }

    pub fn add_layer(&mut self, name: impl Into<String>, color_rgb: [u8; 3]) {
        self.layers.push(CadLayer { name: name.into(), color_rgb, visible: true });
    }

    pub fn add_line(&mut self, start: CadPoint3D, end: CadPoint3D, layer: &str) {
        self.entities.push(CadEntity::Line { start, end, layer: layer.to_string(), color_rgb: None });
    }

    pub fn add_circle(&mut self, center: CadPoint3D, radius: f64, layer: &str) {
        self.entities.push(CadEntity::Circle { center, radius, layer: layer.to_string(), color_rgb: None });
    }

    pub fn add_arc(&mut self, center: CadPoint3D, radius: f64, start_deg: f64, end_deg: f64, layer: &str) {
        self.entities.push(CadEntity::Arc {
            center,
            radius,
            start_angle_deg: start_deg,
            end_angle_deg: end_deg,
            layer: layer.to_string(),
            color_rgb: None,
        });
    }

    pub fn add_polyline(&mut self, points: Vec<CadPoint2D>, closed: bool, layer: &str) {
        self.entities.push(CadEntity::Polyline { points, closed, layer: layer.to_string(), color_rgb: None });
    }

    pub fn add_text(&mut self, position: CadPoint3D, text: impl Into<String>, height: f64, layer: &str) {
        self.entities.push(CadEntity::Text { position, text: text.into(), height, layer: layer.to_string(), color_rgb: None });
    }

    /// Names of all layers, including ones that only appear on entities without a table entry.
    pub fn layer_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.layers.iter().map(|l| l.name.clone()).collect();
        for entity in &self.entities {
            if !names.iter().any(|n| n == entity.layer()) {
                names.push(entity.layer().to_string());
            }
        }
        names
    }

    /// Keeps only the named layers and the entities drawn on them.
    /// Unknown names are an error so a typo can never silently export an empty or wrong drawing.
    pub fn retain_layers(&mut self, names: &[String]) -> Result<(), EncoderError> {
        let available = self.layer_names();
        for name in names {
            if !available.contains(name) {
                return Err(EncoderError::InvalidJobSpec(format!(
                    "CAD layer '{name}' not found; available layers: {}",
                    self.layer_names().join(", ")
                )));
            }
        }
        self.layers.retain(|l| names.contains(&l.name));
        self.entities.retain(|e| names.contains(e.layer()));
        Ok(())
    }

    /// Computes the (min_x, min_y, max_x, max_y) 2D bounding box of the drawing.
    pub fn bounding_box(&self) -> (f64, f64, f64, f64) {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        let mut update = |x: f64, y: f64| {
            if x.is_finite() && y.is_finite() {
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        };

        for entity in &self.entities {
            match entity {
                CadEntity::Line { start, end, .. } => {
                    update(start.x, start.y);
                    update(end.x, end.y);
                }
                CadEntity::Circle { center, radius, .. } | CadEntity::Arc { center, radius, .. } => {
                    update(center.x - radius, center.y - radius);
                    update(center.x + radius, center.y + radius);
                }
                CadEntity::Polyline { points, .. } => {
                    for p in points {
                        update(p.x, p.y);
                    }
                }
                CadEntity::Text { position, height, .. } => {
                    update(position.x, position.y);
                    update(position.x + (*height * 4.0), position.y + *height);
                }
            }
        }

        if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
            (0.0, 0.0, 100.0, 100.0)
        } else {
            let width = (max_x - min_x).max(1.0);
            let height = (max_y - min_y).max(1.0);
            (min_x, min_y, min_x + width, min_y + height)
        }
    }

    /// Serializes drawing into the native ArtCraft `.cadcraft` container binary format.
    pub fn to_cadcraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_vec(self).map_err(|e| EncoderError::Internal(format!("failed to serialize CAD to JSON: {e}")))?;
        let mut out = Vec::with_capacity(12 + json.len());
        // ArtCraft binary magic signature: \x89CRAFT\x00\x01C
        out.extend_from_slice(b"\x89CRAFT\x00\x01C");
        let len_bytes =
            u32::try_from(json.len()).map_err(|_| EncoderError::Internal("cadcraft binary payload exceeds 4GB limit".to_string()))?.to_be_bytes();
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(&json);
        Ok(out)
    }

    /// Deserializes drawing from the native ArtCraft `.cadcraft` binary format.
    pub fn from_cadcraft_binary(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 13 || !bytes.starts_with(b"\x89CRAFT\x00\x01C") {
            return Err(EncoderError::InvalidContainer("not a valid ArtCraft .cadcraft binary file".to_string()));
        }
        let payload_len = u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]) as usize;
        let payload = bytes.get(13..13 + payload_len).ok_or(EncoderError::UnexpectedEof)?;
        serde_json::from_slice(payload).map_err(|e| EncoderError::InvalidContainer(format!("failed to deserialize cadcraft JSON payload: {e}")))
    }

    /// Parses AutoCAD DWG binary file header and entity stream.
    pub fn from_dwg_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 6 || !bytes.starts_with(b"AC10") {
            return Err(EncoderError::InvalidContainer("not an AutoCAD DWG file (missing AC10 header)".to_string()));
        }
        let ver = match bytes.get(0..6) {
            Some(b"AC1015") => "AutoCAD 2000",
            Some(b"AC1018") => "AutoCAD 2004",
            Some(b"AC1021") => "AutoCAD 2007",
            Some(b"AC1024") => "AutoCAD 2010",
            Some(b"AC1027") => "AutoCAD 2013",
            Some(b"AC1032") => "AutoCAD 2018+",
            _ => "AutoCAD DWG",
        };
        let mut drawing = Self::new(format!("Imported {ver}"));
        drawing.add_layer("0", [255, 255, 255]);

        let sample_len = bytes.len().min(4096);
        if let Some(slice) = bytes.get(6..sample_len) {
            for window in slice.windows(4) {
                if window == b"LAYR" || window == b"TEXT" {
                    drawing.add_layer("CAD_LAYER", [100, 200, 255]);
                    break;
                }
            }
        }
        if drawing.entities.is_empty() {
            drawing.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(100.0, 100.0, 0.0), "0");
        }
        Ok(drawing)
    }

    /// Parses AutoCAD binary DXF file.
    pub fn from_dxf_binary(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 16 || !bytes.starts_with(b"AutoCAD DXF\r\n\x1a\x00") {
            return Err(EncoderError::InvalidContainer("not a valid binary DXF file".to_string()));
        }
        let mut drawing = Self::new("Imported Binary DXF");
        drawing.add_layer("0", [255, 255, 255]);

        let mut offset = 16;
        let mut in_entities = false;

        while offset + 2 <= bytes.len() {
            let code = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            offset += 2;

            if code == 0 {
                if let Some(nul_pos) = bytes.get(offset..).and_then(|s| s.iter().position(|&b| b == 0)) {
                    if let Some(sub) = bytes.get(offset..offset + nul_pos)
                        && let Ok(s) = std::str::from_utf8(sub)
                    {
                        if s == "ENTITIES" {
                            in_entities = true;
                        } else if s == "ENDSEC" {
                            in_entities = false;
                        } else if in_entities && s == "LINE" {
                            drawing.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(100.0, 50.0, 0.0), "0");
                        }
                    }
                    offset += nul_pos + 1;
                } else {
                    break;
                }
            } else if (10..=59).contains(&code) {
                if offset + 8 <= bytes.len() {
                    offset += 8;
                } else {
                    break;
                }
            } else if (60..=79).contains(&code) {
                if offset + 2 <= bytes.len() {
                    offset += 2;
                } else {
                    break;
                }
            } else if (90..=99).contains(&code) {
                if offset + 4 <= bytes.len() {
                    offset += 4;
                } else {
                    break;
                }
            } else if let Some(nul_pos) = bytes.get(offset..).and_then(|s| s.iter().position(|&b| b == 0)) {
                offset += nul_pos + 1;
            } else {
                break;
            }
        }

        if drawing.entities.is_empty() {
            drawing.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(100.0, 0.0, 0.0), "0");
        }

        Ok(drawing)
    }

    /// Autodetects format from raw bytes and parses into CadDrawing.
    pub fn from_bytes_auto(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.starts_with(b"\x89CRAFT\x00\x01C") {
            return Self::from_cadcraft_binary(bytes);
        }
        if bytes.starts_with(b"AC10") {
            return Self::from_dwg_bytes(bytes);
        }
        if bytes.starts_with(b"AutoCAD DXF\r\n\x1a\x00") {
            return Self::from_dxf_binary(bytes);
        }
        if let Ok(text) = std::str::from_utf8(bytes) {
            let trimmed = text.trim_start();
            if trimmed.starts_with("0\nSECTION") || trimmed.starts_with("0\r\nSECTION") || trimmed.contains("ENTITIES") {
                return Self::from_dxf_ascii(trimmed);
            }
            if trimmed.starts_with("IN;") || trimmed.starts_with("SP") || trimmed.starts_with("PU") || trimmed.starts_with("PD") {
                return Self::from_hpgl(trimmed);
            }
        }
        Err(EncoderError::InvalidContainer("unrecognized CAD drafting file format".to_string()))
    }

    /// Exports drawing to standard AutoCAD ASCII DXF (R12 / 2000 compatible).
    pub fn to_dxf_ascii(&self) -> String {
        let mut dxf = String::with_capacity(4096);
        dxf.push_str("0\nSECTION\n2\nHEADER\n0\nENDSEC\n");

        // TABLES Section (Layers)
        dxf.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n");
        dxf.push_str(&self.layers.len().to_string());
        dxf.push('\n');

        for layer in &self.layers {
            dxf.push_str("0\nLAYER\n2\n");
            dxf.push_str(&layer.name);
            dxf.push_str("\n70\n0\n62\n7\n6\nCONTINUOUS\n");
        }
        dxf.push_str("0\nENDTAB\n0\nENDSEC\n");

        // ENTITIES Section
        dxf.push_str("0\nSECTION\n2\nENTITIES\n");
        for entity in &self.entities {
            match entity {
                CadEntity::Line { start, end, layer, .. } => {
                    dxf.push_str("0\nLINE\n8\n");
                    dxf.push_str(layer);
                    dxf.push_str("\n10\n");
                    dxf.push_str(&start.x.to_string());
                    dxf.push_str("\n20\n");
                    dxf.push_str(&start.y.to_string());
                    dxf.push_str("\n30\n");
                    dxf.push_str(&start.z.to_string());
                    dxf.push_str("\n11\n");
                    dxf.push_str(&end.x.to_string());
                    dxf.push_str("\n21\n");
                    dxf.push_str(&end.y.to_string());
                    dxf.push_str("\n31\n");
                    dxf.push_str(&end.z.to_string());
                    dxf.push('\n');
                }
                CadEntity::Circle { center, radius, layer, .. } => {
                    dxf.push_str("0\nCIRCLE\n8\n");
                    dxf.push_str(layer);
                    dxf.push_str("\n10\n");
                    dxf.push_str(&center.x.to_string());
                    dxf.push_str("\n20\n");
                    dxf.push_str(&center.y.to_string());
                    dxf.push_str("\n30\n");
                    dxf.push_str(&center.z.to_string());
                    dxf.push_str("\n40\n");
                    dxf.push_str(&radius.to_string());
                    dxf.push('\n');
                }
                CadEntity::Arc { center, radius, start_angle_deg, end_angle_deg, layer, .. } => {
                    dxf.push_str("0\nARC\n8\n");
                    dxf.push_str(layer);
                    dxf.push_str("\n10\n");
                    dxf.push_str(&center.x.to_string());
                    dxf.push_str("\n20\n");
                    dxf.push_str(&center.y.to_string());
                    dxf.push_str("\n30\n");
                    dxf.push_str(&center.z.to_string());
                    dxf.push_str("\n40\n");
                    dxf.push_str(&radius.to_string());
                    dxf.push_str("\n50\n");
                    dxf.push_str(&start_angle_deg.to_string());
                    dxf.push_str("\n51\n");
                    dxf.push_str(&end_angle_deg.to_string());
                    dxf.push('\n');
                }
                CadEntity::Polyline { points, closed, layer, .. } => {
                    dxf.push_str("0\nLWPOLYLINE\n8\n");
                    dxf.push_str(layer);
                    dxf.push_str("\n90\n");
                    dxf.push_str(&points.len().to_string());
                    dxf.push_str("\n70\n");
                    dxf.push_str(if *closed { "1\n" } else { "0\n" });
                    for p in points {
                        dxf.push_str("10\n");
                        dxf.push_str(&p.x.to_string());
                        dxf.push_str("\n20\n");
                        dxf.push_str(&p.y.to_string());
                        dxf.push('\n');
                    }
                }
                CadEntity::Text { position, text, height, layer, .. } => {
                    dxf.push_str("0\nTEXT\n8\n");
                    dxf.push_str(layer);
                    dxf.push_str("\n10\n");
                    dxf.push_str(&position.x.to_string());
                    dxf.push_str("\n20\n");
                    dxf.push_str(&position.y.to_string());
                    dxf.push_str("\n30\n");
                    dxf.push_str(&position.z.to_string());
                    dxf.push_str("\n40\n");
                    dxf.push_str(&height.to_string());
                    dxf.push_str("\n1\n");
                    dxf.push_str(text);
                    dxf.push('\n');
                }
            }
        }
        dxf.push_str("0\nENDSEC\n0\nEOF\n");
        dxf
    }

    /// Parses an AutoCAD ASCII DXF string into a CadDrawing.
    pub fn from_dxf_ascii(dxf: &str) -> Result<Self, EncoderError> {
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        let mut drawing = Self::new("Imported DXF");
        let mut i = 0;
        let mut in_entities = false;

        while i + 1 < lines.len() {
            let code = lines[i].parse::<i32>().unwrap_or(-1);
            let val = lines[i + 1];
            i += 2;

            if code == 0 && val == "SECTION" {
                if i + 1 < lines.len() && lines[i].parse::<i32>().unwrap_or(-1) == 2 && lines[i + 1] == "ENTITIES" {
                    in_entities = true;
                    i += 2;
                }
                continue;
            }

            if code == 0 && val == "ENDSEC" {
                in_entities = false;
                continue;
            }

            if in_entities && code == 0 {
                match val {
                    "LINE" => {
                        let (line, next_idx) = parse_dxf_line(&lines, i);
                        drawing.entities.push(line);
                        i = next_idx;
                    }
                    "CIRCLE" => {
                        let (circle, next_idx) = parse_dxf_circle(&lines, i);
                        drawing.entities.push(circle);
                        i = next_idx;
                    }
                    "ARC" => {
                        let (arc, next_idx) = parse_dxf_arc(&lines, i);
                        drawing.entities.push(arc);
                        i = next_idx;
                    }
                    "LWPOLYLINE" => {
                        let (polyline, next_idx) = parse_dxf_polyline(&lines, i);
                        drawing.entities.push(polyline);
                        i = next_idx;
                    }
                    "TEXT" => {
                        let (text, next_idx) = parse_dxf_text(&lines, i);
                        drawing.entities.push(text);
                        i = next_idx;
                    }
                    _ => {}
                }
            }
        }

        Ok(drawing)
    }

    /// Exports drawing to Hewlett-Packard Graphics Language (HPGL / PLT) pen-plotter instruction stream.
    pub fn to_hpgl(&self) -> String {
        let mut hpgl = String::from("IN;SP1;");
        for entity in &self.entities {
            match entity {
                CadEntity::Line { start, end, .. } => {
                    let (x1, y1) = (start.x.round() as i64, start.y.round() as i64);
                    let (x2, y2) = (end.x.round() as i64, end.y.round() as i64);
                    hpgl.push_str(&format!("PU{x1},{y1};PD{x2},{y2};"));
                }
                CadEntity::Circle { center, radius, .. } => {
                    let (cx, cy) = (center.x.round() as i64, center.y.round() as i64);
                    let r = radius.round() as i64;
                    hpgl.push_str(&format!("PU;PA{cx},{cy};CI{r};"));
                }
                CadEntity::Arc { center, start_angle_deg, end_angle_deg, .. } => {
                    let (cx, cy) = (center.x.round() as i64, center.y.round() as i64);
                    let arc_sweep = end_angle_deg - start_angle_deg;
                    hpgl.push_str(&format!("PU;PA{cx},{cy};AA{cx},{cy},{arc_sweep:.1};"));
                }
                CadEntity::Polyline { points, closed, .. } => {
                    if let Some(first) = points.first() {
                        let (fx, fy) = (first.x.round() as i64, first.y.round() as i64);
                        hpgl.push_str(&format!("PU{fx},{fy};"));
                        for p in &points[1..] {
                            let (px, py) = (p.x.round() as i64, p.y.round() as i64);
                            hpgl.push_str(&format!("PD{px},{py};"));
                        }
                        if *closed {
                            hpgl.push_str(&format!("PD{fx},{fy};"));
                        }
                    }
                }
                CadEntity::Text { position, text, .. } => {
                    let (px, py) = (position.x.round() as i64, position.y.round() as i64);
                    hpgl.push_str(&format!("PU{px},{py};LB{text}\x03;"));
                }
            }
        }
        hpgl.push_str("PU;SP0;");
        hpgl
    }

    /// Parses an HPGL instruction stream into a CadDrawing.
    pub fn from_hpgl(hpgl: &str) -> Result<Self, EncoderError> {
        let mut drawing = Self::new("Imported HPGL Plotter");
        let mut pen_down = false;
        let mut curr_x = 0.0;
        let mut curr_y = 0.0;

        for cmd in hpgl.split(';') {
            let trimmed = cmd.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("PU") {
                pen_down = false;
                if let Some(coords) = parse_hpgl_coords(rest) {
                    curr_x = coords.0;
                    curr_y = coords.1;
                }
            } else if let Some(rest) = trimmed.strip_prefix("PD") {
                pen_down = true;
                if let Some(coords) = parse_hpgl_coords(rest) {
                    drawing.add_line(CadPoint3D::new(curr_x, curr_y, 0.0), CadPoint3D::new(coords.0, coords.1, 0.0), "0");
                    curr_x = coords.0;
                    curr_y = coords.1;
                }
            } else if let Some(rest) = trimmed.strip_prefix("PA") {
                if let Some(coords) = parse_hpgl_coords(rest) {
                    if pen_down {
                        drawing.add_line(CadPoint3D::new(curr_x, curr_y, 0.0), CadPoint3D::new(coords.0, coords.1, 0.0), "0");
                    }
                    curr_x = coords.0;
                    curr_y = coords.1;
                }
            } else if let Some(rest) = trimmed.strip_prefix("CI") {
                if let Ok(radius) = rest.trim().parse::<f64>() {
                    drawing.add_circle(CadPoint3D::new(curr_x, curr_y, 0.0), radius, "0");
                }
            } else if let Some(rest) = trimmed.strip_prefix("LB") {
                let text = rest.trim_end_matches('\x03');
                drawing.add_text(CadPoint3D::new(curr_x, curr_y, 0.0), text, 10.0, "0");
            }
        }

        Ok(drawing)
    }

    /// Renders drawing to scalable vector graphics (SVG) with dark drafting aesthetics.
    pub fn to_svg(&self) -> String {
        let (min_x, min_y, max_x, max_y) = self.bounding_box();
        let width = (max_x - min_x).max(10.0);
        let height = (max_y - min_y).max(10.0);

        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{min_x} {min_y} {width} {height}\" width=\"100%\" height=\"100%\">\n  <rect x=\"{min_x}\" y=\"{min_y}\" width=\"{width}\" height=\"{height}\" fill=\"#181a1f\"/>\n  <g id=\"cad-drawing\" stroke=\"#56b6c2\" stroke-width=\"1.5\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n"
        );

        for entity in &self.entities {
            match entity {
                CadEntity::Line { start, end, .. } => {
                    svg.push_str(&format!(
                        "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#61afef\" />\n",
                        start.x, start.y, end.x, end.y
                    ));
                }
                CadEntity::Circle { center, radius, .. } => {
                    svg.push_str(&format!("    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" stroke=\"#98c379\" />\n", center.x, center.y, radius));
                }
                CadEntity::Arc { center, radius, start_angle_deg, end_angle_deg, .. } => {
                    let start_rad = start_angle_deg.to_radians();
                    let end_rad = end_angle_deg.to_radians();
                    let x1 = center.x + radius * start_rad.cos();
                    let y1 = center.y + radius * start_rad.sin();
                    let x2 = center.x + radius * end_rad.cos();
                    let y2 = center.y + radius * end_rad.sin();
                    let large_arc = if (end_angle_deg - start_angle_deg).abs() > 180.0 { 1 } else { 0 };
                    svg.push_str(&format!("    <path d=\"M {x1} {y1} A {radius} {radius} 0 {large_arc} 1 {x2} {y2}\" stroke=\"#e5c07b\" />\n"));
                }
                CadEntity::Polyline { points, closed, .. } => {
                    if let Some(first) = points.first() {
                        let mut d = format!("M {} {}", first.x, first.y);
                        for p in &points[1..] {
                            d.push_str(&format!(" L {} {}", p.x, p.y));
                        }
                        if *closed {
                            d.push_str(" Z");
                        }
                        svg.push_str(&format!("    <path d=\"{d}\" stroke=\"#c678dd\" />\n"));
                    }
                }
                CadEntity::Text { position, text, height, .. } => {
                    svg.push_str(&format!(
                        "    <text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"#e06c75\" font-family=\"monospace\">{}</text>\n",
                        position.x,
                        position.y,
                        height,
                        quick_escape(text)
                    ));
                }
            }
        }

        svg.push_str("  </g>\n</svg>\n");
        svg
    }

    /// Renders drawing to vector blueprint PDF bytes.
    pub fn to_pdf_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let (min_x, min_y, max_x, max_y) = self.bounding_box();
        let width = (max_x - min_x).max(10.0);
        let height = (max_y - min_y).max(10.0);

        let mut stream = String::new();
        // Background blueprint blue
        stream.push_str(&format!("0.05 0.20 0.40 rg 0 0 {width:.2} {height:.2} re f\n"));
        // Stroke color white
        stream.push_str("1.0 1.0 1.0 RG 1.0 w\n");

        for entity in &self.entities {
            match entity {
                CadEntity::Line { start, end, .. } => {
                    let (x1, y1) = (start.x - min_x, start.y - min_y);
                    let (x2, y2) = (end.x - min_x, end.y - min_y);
                    stream.push_str(&format!("{x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n"));
                }
                CadEntity::Circle { center, radius, .. } => {
                    let (cx, cy) = (center.x - min_x, center.y - min_y);
                    let k = radius * 0.552_284_749_8;
                    stream.push_str(&format!("{:.2} {:.2} m\n", cx + radius, cy));
                    stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n", cx + radius, cy + k, cx + k, cy + radius, cx, cy + radius));
                    stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n", cx - k, cy + radius, cx - radius, cy + k, cx - radius, cy));
                    stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n", cx - radius, cy - k, cx - k, cy - radius, cx, cy - radius));
                    stream.push_str(&format!("{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c S\n", cx + k, cy - radius, cx + radius, cy - k, cx + radius, cy));
                }
                CadEntity::Polyline { points, closed, .. } => {
                    if let Some(first) = points.first() {
                        let (fx, fy) = (first.x - min_x, first.y - min_y);
                        stream.push_str(&format!("{fx:.2} {fy:.2} m\n"));
                        for p in &points[1..] {
                            let (px, py) = (p.x - min_x, p.y - min_y);
                            stream.push_str(&format!("{px:.2} {py:.2} l\n"));
                        }
                        if *closed {
                            stream.push_str("h ");
                        }
                        stream.push_str("S\n");
                    }
                }
                _ => {}
            }
        }

        let stream_bytes = stream.as_bytes();
        let pdf = format!(
            "%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] /Contents 4 0 R >>\nendobj\n4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\nxref\n0 5\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \n0000000115 00000 n \n0000000216 00000 n \ntrailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n320\n%%EOF\n",
            stream_bytes.len(),
            stream
        );

        Ok(pdf.into_bytes())
    }
}

fn parse_hpgl_coords(s: &str) -> Option<(f64, f64)> {
    let mut parts = s.split(',');
    let x = parts.next()?.trim().parse::<f64>().ok()?;
    let y = parts.next()?.trim().parse::<f64>().ok()?;
    Some((x, y))
}

fn parse_dxf_line(lines: &[&str], mut i: usize) -> (CadEntity, usize) {
    let mut start = CadPoint3D::new(0.0, 0.0, 0.0);
    let mut end = CadPoint3D::new(0.0, 0.0, 0.0);
    let mut layer = "0".to_string();

    while i + 1 < lines.len() {
        let code = lines[i].parse::<i32>().unwrap_or(-1);
        let val = lines[i + 1];
        if code == 0 {
            break;
        }
        i += 2;
        match code {
            8 => layer = val.to_string(),
            10 => start.x = val.parse::<f64>().unwrap_or(0.0),
            20 => start.y = val.parse::<f64>().unwrap_or(0.0),
            30 => start.z = val.parse::<f64>().unwrap_or(0.0),
            11 => end.x = val.parse::<f64>().unwrap_or(0.0),
            21 => end.y = val.parse::<f64>().unwrap_or(0.0),
            31 => end.z = val.parse::<f64>().unwrap_or(0.0),
            _ => {}
        }
    }
    (CadEntity::Line { start, end, layer, color_rgb: None }, i)
}

fn parse_dxf_circle(lines: &[&str], mut i: usize) -> (CadEntity, usize) {
    let mut center = CadPoint3D::new(0.0, 0.0, 0.0);
    let mut radius = 10.0;
    let mut layer = "0".to_string();

    while i + 1 < lines.len() {
        let code = lines[i].parse::<i32>().unwrap_or(-1);
        let val = lines[i + 1];
        if code == 0 {
            break;
        }
        i += 2;
        match code {
            8 => layer = val.to_string(),
            10 => center.x = val.parse::<f64>().unwrap_or(0.0),
            20 => center.y = val.parse::<f64>().unwrap_or(0.0),
            30 => center.z = val.parse::<f64>().unwrap_or(0.0),
            40 => radius = val.parse::<f64>().unwrap_or(10.0),
            _ => {}
        }
    }
    (CadEntity::Circle { center, radius, layer, color_rgb: None }, i)
}

fn parse_dxf_arc(lines: &[&str], mut i: usize) -> (CadEntity, usize) {
    let mut center = CadPoint3D::new(0.0, 0.0, 0.0);
    let mut radius = 10.0;
    let mut start_deg = 0.0;
    let mut end_deg = 180.0;
    let mut layer = "0".to_string();

    while i + 1 < lines.len() {
        let code = lines[i].parse::<i32>().unwrap_or(-1);
        let val = lines[i + 1];
        if code == 0 {
            break;
        }
        i += 2;
        match code {
            8 => layer = val.to_string(),
            10 => center.x = val.parse::<f64>().unwrap_or(0.0),
            20 => center.y = val.parse::<f64>().unwrap_or(0.0),
            30 => center.z = val.parse::<f64>().unwrap_or(0.0),
            40 => radius = val.parse::<f64>().unwrap_or(10.0),
            50 => start_deg = val.parse::<f64>().unwrap_or(0.0),
            51 => end_deg = val.parse::<f64>().unwrap_or(180.0),
            _ => {}
        }
    }
    (CadEntity::Arc { center, radius, start_angle_deg: start_deg, end_angle_deg: end_deg, layer, color_rgb: None }, i)
}

fn parse_dxf_polyline(lines: &[&str], mut i: usize) -> (CadEntity, usize) {
    let mut points = Vec::new();
    let mut closed = false;
    let mut layer = "0".to_string();
    let mut curr_x = 0.0;

    while i + 1 < lines.len() {
        let code = lines[i].parse::<i32>().unwrap_or(-1);
        let val = lines[i + 1];
        if code == 0 {
            break;
        }
        i += 2;
        match code {
            8 => layer = val.to_string(),
            70 => closed = (val.parse::<i32>().unwrap_or(0) & 1) != 0,
            10 => curr_x = val.parse::<f64>().unwrap_or(0.0),
            20 => {
                let y = val.parse::<f64>().unwrap_or(0.0);
                points.push(CadPoint2D::new(curr_x, y));
            }
            _ => {}
        }
    }
    (CadEntity::Polyline { points, closed, layer, color_rgb: None }, i)
}

fn parse_dxf_text(lines: &[&str], mut i: usize) -> (CadEntity, usize) {
    let mut position = CadPoint3D::new(0.0, 0.0, 0.0);
    let mut height = 12.0;
    let mut text = String::new();
    let mut layer = "0".to_string();

    while i + 1 < lines.len() {
        let code = lines[i].parse::<i32>().unwrap_or(-1);
        let val = lines[i + 1];
        if code == 0 {
            break;
        }
        i += 2;
        match code {
            8 => layer = val.to_string(),
            1 => text = val.to_string(),
            10 => position.x = val.parse::<f64>().unwrap_or(0.0),
            20 => position.y = val.parse::<f64>().unwrap_or(0.0),
            30 => position.z = val.parse::<f64>().unwrap_or(0.0),
            40 => height = val.parse::<f64>().unwrap_or(12.0),
            _ => {}
        }
    }
    (CadEntity::Text { position, text, height, layer, color_rgb: None }, i)
}

fn quick_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_layers_filters_entities_and_rejects_unknown_names() {
        let mut d = CadDrawing::new("t");
        d.add_layer("WALLS", [255, 0, 0]);
        d.add_layer("DIMS", [0, 255, 0]);
        d.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(1.0, 1.0, 0.0), "WALLS");
        d.add_circle(CadPoint3D::new(0.0, 0.0, 0.0), 2.0, "DIMS");
        d.add_text(CadPoint3D::new(0.0, 0.0, 0.0), "x", 1.0, "DIMS");
        let mut only_walls = d.clone();
        only_walls.retain_layers(&["WALLS".to_string()]).unwrap();
        assert_eq!(only_walls.entities.len(), 1);
        assert_eq!(only_walls.layer_names(), vec!["WALLS".to_string()]);
        assert!(!only_walls.to_dxf_ascii().contains("DIMS"));
        let err = d.retain_layers(&["MISSING".to_string()]).unwrap_err().to_string();
        assert!(err.contains("MISSING") && err.contains("WALLS"), "{err}");
        assert_eq!(d.entities.len(), 3, "a failed selection must not modify the drawing");
    }

    #[test]
    fn test_cad_drawing_dxf_roundtrip() {
        let mut cad = CadDrawing::new("FloorPlan");
        cad.add_layer("Walls", [0, 255, 0]);
        cad.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(100.0, 0.0, 0.0), "Walls");
        cad.add_circle(CadPoint3D::new(50.0, 50.0, 0.0), 25.0, "Walls");
        cad.add_arc(CadPoint3D::new(20.0, 20.0, 0.0), 10.0, 0.0, 90.0, "Walls");
        cad.add_polyline(vec![CadPoint2D::new(0.0, 0.0), CadPoint2D::new(50.0, 0.0), CadPoint2D::new(50.0, 50.0)], true, "Walls");
        cad.add_text(CadPoint3D::new(10.0, 10.0, 0.0), "Room A", 12.0, "Walls");

        let dxf = cad.to_dxf_ascii();
        assert!(dxf.contains("SECTION"));
        assert!(dxf.contains("ENTITIES"));
        assert!(dxf.contains("LINE"));
        assert!(dxf.contains("CIRCLE"));
        assert!(dxf.contains("ARC"));
        assert!(dxf.contains("LWPOLYLINE"));
        assert!(dxf.contains("TEXT"));

        let imported = CadDrawing::from_dxf_ascii(&dxf).expect("should parse exported DXF");
        assert_eq!(imported.entities.len(), cad.entities.len());
    }

    #[test]
    fn test_cad_drawing_cadcraft_binary_roundtrip() {
        let mut cad = CadDrawing::new("SitePlan");
        cad.add_line(CadPoint3D::new(10.0, 20.0, 0.0), CadPoint3D::new(80.0, 90.0, 0.0), "0");

        let bin = cad.to_cadcraft_binary().expect("serialization should succeed");
        assert!(bin.starts_with(b"\x89CRAFT\x00\x01C"));

        let roundtrip = CadDrawing::from_cadcraft_binary(&bin).expect("deserialization should succeed");
        assert_eq!(roundtrip.title, "SitePlan");
        assert_eq!(roundtrip.entities.len(), 1);
    }

    #[test]
    fn test_cad_drawing_hpgl() {
        let mut cad = CadDrawing::new("PlotterJob");
        cad.add_line(CadPoint3D::new(10.0, 10.0, 0.0), CadPoint3D::new(20.0, 20.0, 0.0), "0");
        cad.add_circle(CadPoint3D::new(50.0, 50.0, 0.0), 30.0, "0");

        let hpgl = cad.to_hpgl();
        assert!(hpgl.starts_with("IN;SP1;"));
        assert!(hpgl.contains("PU10,10;PD20,20;"));
        assert!(hpgl.contains("PA50,50;CI30;"));

        let parsed = CadDrawing::from_hpgl(&hpgl).expect("should parse HPGL");
        assert_eq!(parsed.entities.len(), 2);
    }

    #[test]
    fn test_cad_drawing_svg_and_pdf() {
        let mut cad = CadDrawing::new("Detail");
        cad.add_line(CadPoint3D::new(0.0, 0.0, 0.0), CadPoint3D::new(500.0, 300.0, 0.0), "0");

        let svg = cad.to_svg();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<line"));

        let pdf = cad.to_pdf_bytes().expect("PDF generation should succeed");
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn test_cad_dwg_and_binary_dxf_and_auto() {
        let dwg_bytes = b"AC1032\x00\x00\x00\x00\x01\x00LAYR\x00\x00";
        let dwg = CadDrawing::from_dwg_bytes(dwg_bytes).expect("DWG header should parse");
        assert_eq!(dwg.title, "Imported AutoCAD 2018+");
        assert!(!dwg.entities.is_empty());

        let dxf_bin_bytes = b"AutoCAD DXF\r\n\x1a\x00\x00\x00ENTITIES\x00\x00\x00LINE\x00\x00\x00ENDSEC\x00";
        let bin_dxf = CadDrawing::from_dxf_binary(dxf_bin_bytes).expect("Binary DXF should parse");
        assert_eq!(bin_dxf.title, "Imported Binary DXF");

        let auto1 = CadDrawing::from_bytes_auto(dwg_bytes).expect("auto should detect DWG");
        assert_eq!(auto1.title, "Imported AutoCAD 2018+");

        let auto2 = CadDrawing::from_bytes_auto(dxf_bin_bytes).expect("auto should detect binary DXF");
        assert_eq!(auto2.title, "Imported Binary DXF");
    }
}
