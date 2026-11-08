//! 1D and 3D Look-Up Table (LUT) parser, serializer, and interpolation engine.
//!
//! Supports:
//! - Adobe / DaVinci Resolve `.cube` (1D and 3D)
//! - Autodesk `.3dl` (Flame / Lustre / Maya)
//! - Adobe Lumetri `.look`
//!
//! Provides both high-speed trilinear interpolation and precision tetrahedral interpolation,
//! as well as arbitrary lattice resampling (e.g. 17x17x17 to 33x33x33 or 65x65x65).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors arising during LUT parsing, serialization, or transformation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LutError {
    #[error("invalid LUT lattice dimension: {0}")]
    InvalidSize(String),

    #[error("LUT data length mismatch: expected {expected} samples, got {actual}")]
    DataLengthMismatch { expected: usize, actual: usize },

    #[error("LUT syntax or parse error: {0}")]
    ParseError(String),

    #[error("invalid LUT domain range: {0}")]
    InvalidDomain(String),
}

/// Algorithm used when interpolating between lattice vertices in a 3D LUT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InterpolationMode {
    /// Industry-standard tetrahedral interpolation.
    ///
    /// Decomposes each sub-cube into 6 tetrahedra, ensuring strict neutrality along
    /// the grey diagonal and eliminating non-linear cross-term color distortion.
    #[default]
    Tetrahedral,

    /// Classic trilinear interpolation blending all 8 adjacent corner vertices.
    Trilinear,
}

/// A 1D Look-Up Table mapping independent red, green, and blue transfer curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lut1D {
    pub title: String,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data: Vec<[f32; 3]>,
}

impl Lut1D {
    /// Creates a 1D LUT directly from a vector of RGB triplets.
    pub fn new(title: impl Into<String>, size: usize, data: Vec<[f32; 3]>) -> Result<Self, LutError> {
        if !(2..=65536).contains(&size) {
            return Err(LutError::InvalidSize(format!("1D LUT size {size} out of range (2..=65536)")));
        }
        if data.len() != size {
            return Err(LutError::DataLengthMismatch { expected: size, actual: data.len() });
        }
        Ok(Self { title: title.into(), size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Parses a 1D LUT from standard Adobe `.cube` format string.
    pub fn parse_cube(cube_str: &str) -> Result<Self, LutError> {
        let mut title = "Untitled 1D LUT".to_string();
        let mut size = 0usize;
        let mut domain_min = [0.0f32, 0.0, 0.0];
        let mut domain_max = [1.0f32, 1.0, 1.0];
        let mut data = Vec::new();

        for line in cube_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "TITLE" => {
                    if parts.len() > 1 {
                        title = parts[1..].join(" ").trim_matches('"').to_string();
                    }
                }
                "LUT_1D_SIZE" => {
                    if parts.len() > 1 {
                        size = parts[1].parse::<usize>().map_err(|e| LutError::ParseError(format!("invalid LUT_1D_SIZE: {e}")))?;
                        if !(2..=65536).contains(&size) {
                            return Err(LutError::InvalidSize(format!("unsupported 1D LUT size {size}")));
                        }
                        data.reserve(size);
                    }
                }
                "DOMAIN_MIN" => {
                    if parts.len() >= 4 {
                        domain_min[0] = parts[1].parse().unwrap_or(0.0);
                        domain_min[1] = parts[2].parse().unwrap_or(0.0);
                        domain_min[2] = parts[3].parse().unwrap_or(0.0);
                    }
                }
                "DOMAIN_MAX" => {
                    if parts.len() >= 4 {
                        domain_max[0] = parts[1].parse().unwrap_or(1.0);
                        domain_max[1] = parts[2].parse().unwrap_or(1.0);
                        domain_max[2] = parts[3].parse().unwrap_or(1.0);
                    }
                }
                _ => {
                    if parts.len() >= 3
                        && let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>())
                    {
                        data.push([r, g, b]);
                    }
                }
            }
        }

        if size == 0 || data.len() < size {
            return Err(LutError::DataLengthMismatch { expected: size, actual: data.len() });
        }

        Ok(Self { title, size, domain_min, domain_max, data })
    }

    /// Serializes to Adobe `.cube` 1D specification string.
    pub fn to_cube(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("TITLE \"{}\"\n", self.title));
        out.push_str(&format!("LUT_1D_SIZE {}\n", self.size));
        out.push_str(&format!("DOMAIN_MIN {:.6} {:.6} {:.6}\n", self.domain_min[0], self.domain_min[1], self.domain_min[2]));
        out.push_str(&format!("DOMAIN_MAX {:.6} {:.6} {:.6}\n", self.domain_max[0], self.domain_max[1], self.domain_max[2]));
        for sample in &self.data {
            out.push_str(&format!("{:.6} {:.6} {:.6}\n", sample[0], sample[1], sample[2]));
        }
        out
    }

    /// Evaluates the 1D LUT for an input RGB triplet.
    #[inline]
    pub fn evaluate(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let eval_channel = |val: f32, ch: usize| -> f32 {
            let d_min = self.domain_min[ch];
            let d_max = self.domain_max[ch];
            let norm = ((val - d_min) / (d_max - d_min).max(1e-6)).clamp(0.0, 1.0);
            let scaled = norm * (self.size.saturating_sub(1) as f32);
            let idx0 = (scaled.floor() as usize).min(self.size.saturating_sub(1));
            let idx1 = (idx0 + 1).min(self.size.saturating_sub(1));
            let frac = scaled - (idx0 as f32);
            let v0 = self.data.get(idx0).map(|s| s[ch]).unwrap_or(0.0);
            let v1 = self.data.get(idx1).map(|s| s[ch]).unwrap_or(0.0);
            v0 * (1.0 - frac) + v1 * frac
        };

        [eval_channel(r, 0), eval_channel(g, 1), eval_channel(b, 2)]
    }
}

/// A 3D color lookup table for color grading and gamut transformations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lut3D {
    pub title: String,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data: Vec<[f32; 3]>,
}

impl Lut3D {
    /// Creates a 3D LUT directly from a raw table of RGB triplets.
    pub fn from_raw_table(size: usize, data: Vec<[f32; 3]>) -> Result<Self, LutError> {
        if !(2..=256).contains(&size) {
            return Err(LutError::InvalidSize(format!("unsupported LUT size {size} (must be 2..=256)")));
        }
        let expected = size.checked_mul(size).and_then(|s| s.checked_mul(size)).ok_or_else(|| LutError::InvalidSize("LUT size overflow".into()))?;
        if data.len() != expected {
            return Err(LutError::DataLengthMismatch { expected, actual: data.len() });
        }
        Ok(Self { title: "Raw Table LUT".into(), size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Generates a neutral identity 3D LUT of the given lattice dimension.
    pub fn identity(size: usize) -> Result<Self, LutError> {
        if !(2..=256).contains(&size) {
            return Err(LutError::InvalidSize(format!("LUT size {size} out of range (2..=256)")));
        }
        let total = size * size * size;
        let mut data = Vec::with_capacity(total);
        let step = 1.0 / (size.saturating_sub(1) as f32);

        // Standard ordering: Red moves fastest, then Green, then Blue
        for b in 0..size {
            let fb = b as f32 * step;
            for g in 0..size {
                let fg = g as f32 * step;
                for r in 0..size {
                    let fr = r as f32 * step;
                    data.push([fr, fg, fb]);
                }
            }
        }

        Ok(Self { title: format!("Identity {size}x{size}x{size}"), size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Parses a 3D LUT from standard Adobe `.cube` file format string.
    pub fn parse_cube(cube_str: &str) -> Result<Self, LutError> {
        let mut title = "Untitled LUT".to_string();
        let mut size = 0usize;
        let mut domain_min = [0.0f32, 0.0, 0.0];
        let mut domain_max = [1.0f32, 1.0, 1.0];
        let mut data = Vec::new();

        for line in cube_str.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "TITLE" => {
                    if parts.len() > 1 {
                        title = parts[1..].join(" ").trim_matches('"').to_string();
                    }
                }
                "LUT_3D_SIZE" => {
                    if parts.len() > 1 {
                        size = parts[1].parse::<usize>().map_err(|e| LutError::ParseError(format!("invalid LUT_3D_SIZE: {e}")))?;
                        if !(2..=256).contains(&size) {
                            return Err(LutError::InvalidSize(format!("unsupported LUT size {size} (must be 2..=256)")));
                        }
                        data.reserve(size.checked_mul(size).and_then(|s| s.checked_mul(size)).unwrap_or(0));
                    }
                }
                "DOMAIN_MIN" => {
                    if parts.len() >= 4 {
                        domain_min[0] = parts[1].parse().unwrap_or(0.0);
                        domain_min[1] = parts[2].parse().unwrap_or(0.0);
                        domain_min[2] = parts[3].parse().unwrap_or(0.0);
                    }
                }
                "DOMAIN_MAX" => {
                    if parts.len() >= 4 {
                        domain_max[0] = parts[1].parse().unwrap_or(1.0);
                        domain_max[1] = parts[2].parse().unwrap_or(1.0);
                        domain_max[2] = parts[3].parse().unwrap_or(1.0);
                    }
                }
                _ => {
                    if parts.len() >= 3
                        && let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>())
                    {
                        data.push([r, g, b]);
                    }
                }
            }
        }

        let expected = size.checked_mul(size).and_then(|s| s.checked_mul(size)).unwrap_or(0);
        if size == 0 || data.len() < expected {
            return Err(LutError::DataLengthMismatch { expected, actual: data.len() });
        }

        Ok(Self { title, size, domain_min, domain_max, data })
    }

    /// Serializes 3D LUT to standard Adobe `.cube` specification string.
    pub fn to_cube(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("TITLE \"{}\"\n", self.title));
        out.push_str(&format!("LUT_3D_SIZE {}\n", self.size));
        out.push_str(&format!("DOMAIN_MIN {:.6} {:.6} {:.6}\n", self.domain_min[0], self.domain_min[1], self.domain_min[2]));
        out.push_str(&format!("DOMAIN_MAX {:.6} {:.6} {:.6}\n", self.domain_max[0], self.domain_max[1], self.domain_max[2]));
        for sample in &self.data {
            out.push_str(&format!("{:.6} {:.6} {:.6}\n", sample[0], sample[1], sample[2]));
        }
        out
    }

    /// Parses an Autodesk `.3dl` 3D LUT format string.
    pub fn parse_3dl(text: &str) -> Result<Self, LutError> {
        let mut mesh_points = Vec::new();
        let mut data = Vec::new();
        let mut max_val = 1.0f32;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with("3DMESH") || trimmed.starts_with("Mesh") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            if mesh_points.is_empty() && parts.iter().all(|p| p.parse::<u32>().is_ok()) && parts.len() >= 2 {
                for p in parts {
                    if let Ok(v) = p.parse::<f32>() {
                        mesh_points.push(v);
                    }
                }
                continue;
            }

            if parts.len() >= 3
                && let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>())
            {
                if r > max_val {
                    max_val = r;
                }
                if g > max_val {
                    max_val = g;
                }
                if b > max_val {
                    max_val = b;
                }
                data.push([r, g, b]);
            }
        }

        let size = if !mesh_points.is_empty() {
            mesh_points.len()
        } else {
            // Infer cube root of data len
            let cube_root = (data.len() as f64).cbrt().round() as usize;
            if cube_root * cube_root * cube_root == data.len() {
                cube_root
            } else {
                return Err(LutError::ParseError("cannot deduce 3D mesh size from .3dl data".into()));
            }
        };

        let scale = if max_val > 1024.0 {
            4095.0
        } else if max_val > 1.0 {
            1023.0
        } else {
            1.0
        };

        if scale > 1.0 {
            for sample in &mut data {
                sample[0] /= scale;
                sample[1] /= scale;
                sample[2] /= scale;
            }
        }

        let expected = size * size * size;
        if data.len() < expected {
            return Err(LutError::DataLengthMismatch { expected, actual: data.len() });
        }

        Ok(Self { title: "Autodesk 3DL Import".into(), size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Serializes 3D LUT to Autodesk `.3dl` format string.
    pub fn to_3dl(&self) -> String {
        let mut out = String::new();
        out.push_str("# Autodesk 3DL LUT generated by encoderCraft\n");
        let step = 1023.0 / (self.size.saturating_sub(1) as f32);
        let mesh_pts: Vec<String> = (0..self.size).map(|i| format!("{:.0}", i as f32 * step)).collect();
        out.push_str(&mesh_pts.join(" "));
        out.push('\n');

        for sample in &self.data {
            let r = (sample[0] * 1023.0).clamp(0.0, 1023.0).round() as u32;
            let g = (sample[1] * 1023.0).clamp(0.0, 1023.0).round() as u32;
            let b = (sample[2] * 1023.0).clamp(0.0, 1023.0).round() as u32;
            out.push_str(&format!("{r} {g} {b}\n"));
        }
        out
    }

    /// Parses an Adobe Lumetri `.look` format string.
    pub fn parse_look(text: &str) -> Result<Self, LutError> {
        let mut title = "Lumetri Look".to_string();
        if let Some(start) = text.find("<Title>")
            && let Some(end) = text[start + 7..].find("</Title>")
        {
            title = text[start + 7..start + 7 + end].trim().to_string();
        }

        let mut size = 0usize;
        if let Some(pos) = text.find("<Lut3D")
            && let Some(attr_pos) = text[pos..].find("size=\"")
        {
            let val_str = &text[pos + attr_pos + 6..];
            if let Some(quote_end) = val_str.find('"') {
                size = val_str[..quote_end].parse().unwrap_or(0);
            }
        }

        let mut data = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('<') || trimmed.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3
                && let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>())
            {
                data.push([r, g, b]);
            }
        }

        if size == 0 {
            size = (data.len() as f64).cbrt().round() as usize;
        }

        let expected = size * size * size;
        if size == 0 || data.len() < expected {
            return Err(LutError::DataLengthMismatch { expected, actual: data.len() });
        }

        Ok(Self { title, size, domain_min: [0.0, 0.0, 0.0], domain_max: [1.0, 1.0, 1.0], data })
    }

    /// Serializes 3D LUT to Adobe Lumetri `.look` XML format string.
    pub fn to_look(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<Look version=\"1.0\">\n");
        out.push_str(&format!("  <Title>{}</Title>\n", self.title));
        out.push_str(&format!("  <Lut3D size=\"{}\">\n", self.size));
        for sample in &self.data {
            out.push_str(&format!("    {:.6} {:.6} {:.6}\n", sample[0], sample[1], sample[2]));
        }
        out.push_str("  </Lut3D>\n");
        out.push_str("</Look>\n");
        out
    }

    /// Autodetects LUT format (.cube, .3dl, or .look) and parses.
    pub fn parse_auto(text: &str) -> Result<Self, LutError> {
        let trimmed = text.trim();
        if trimmed.starts_with("<?xml") || trimmed.contains("<Look") || trimmed.contains("<Lut3D") {
            Self::parse_look(text)
        } else if trimmed.contains("LUT_3D_SIZE") || trimmed.contains("LUT_1D_SIZE") {
            Self::parse_cube(text)
        } else if trimmed.starts_with("3DMESH")
            || trimmed.starts_with('#')
            || trimmed.lines().next().is_some_and(|l| l.split_whitespace().count() > 5)
        {
            Self::parse_3dl(text).or_else(|_| Self::parse_cube(text))
        } else {
            Self::parse_cube(text).or_else(|_| Self::parse_3dl(text))
        }
    }

    /// Evaluates using the chosen interpolation mode.
    #[inline]
    pub fn evaluate(&self, r: f32, g: f32, b: f32, mode: InterpolationMode) -> [f32; 3] {
        match mode {
            InterpolationMode::Tetrahedral => self.evaluate_tetrahedral(r, g, b),
            InterpolationMode::Trilinear => self.evaluate_trilinear(r, g, b),
        }
    }

    /// Evaluates the 3D LUT using precision tetrahedral interpolation.
    #[inline]
    pub fn evaluate_tetrahedral(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let n = self.size as f32;
        let d_min = self.domain_min;
        let d_max = self.domain_max;

        let nr = ((r - d_min[0]) / (d_max[0] - d_min[0]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let ng = ((g - d_min[1]) / (d_max[1] - d_min[1]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let nb = ((b - d_min[2]) / (d_max[2] - d_min[2]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);

        let r0 = (nr.floor() as usize).min(self.size.saturating_sub(1));
        let g0 = (ng.floor() as usize).min(self.size.saturating_sub(1));
        let b0 = (nb.floor() as usize).min(self.size.saturating_sub(1));

        let r1 = (r0 + 1).min(self.size.saturating_sub(1));
        let g1 = (g0 + 1).min(self.size.saturating_sub(1));
        let b1 = (b0 + 1).min(self.size.saturating_sub(1));

        let dr = nr - (r0 as f32);
        let dg = ng - (g0 as f32);
        let db = nb - (b0 as f32);

        let s = self.size;
        let get = |rx: usize, gx: usize, bx: usize| -> [f32; 3] {
            let idx = rx + s * (gx + s * bx);
            self.data.get(idx).copied().unwrap_or([0.0, 0.0, 0.0])
        };

        let c000 = get(r0, g0, b0);
        let c111 = get(r1, g1, b1);

        let blend = |w0: f32, c0: [f32; 3], w1: f32, c1: [f32; 3], w2: f32, c2: [f32; 3], w3: f32, c3: [f32; 3]| -> [f32; 3] {
            [
                w0 * c0[0] + w1 * c1[0] + w2 * c2[0] + w3 * c3[0],
                w0 * c0[1] + w1 * c1[1] + w2 * c2[1] + w3 * c3[1],
                w0 * c0[2] + w1 * c1[2] + w2 * c2[2] + w3 * c3[2],
            ]
        };

        // Determine which of the 6 tetrahedra contains (dr, dg, db)
        if dr >= dg {
            if dg >= db {
                // dr >= dg >= db
                let c100 = get(r1, g0, b0);
                let c110 = get(r1, g1, b0);
                blend(1.0 - dr, c000, dr - dg, c100, dg - db, c110, db, c111)
            } else if dr >= db {
                // dr >= db > dg
                let c100 = get(r1, g0, b0);
                let c101 = get(r1, g0, b1);
                blend(1.0 - dr, c000, dr - db, c100, db - dg, c101, dg, c111)
            } else {
                // db > dr >= dg
                let c001 = get(r0, g0, b1);
                let c101 = get(r1, g0, b1);
                blend(1.0 - db, c000, db - dr, c001, dr - dg, c101, dg, c111)
            }
        } else if dg >= db {
            if dr >= db {
                // dg > dr >= db
                let c010 = get(r0, g1, b0);
                let c110 = get(r1, g1, b0);
                blend(1.0 - dg, c000, dg - dr, c010, dr - db, c110, db, c111)
            } else {
                // dg >= db > dr
                let c010 = get(r0, g1, b0);
                let c011 = get(r0, g1, b1);
                blend(1.0 - dg, c000, dg - db, c010, db - dr, c011, dr, c111)
            }
        } else {
            // db > dg > dr
            let c001 = get(r0, g0, b1);
            let c011 = get(r0, g1, b1);
            blend(1.0 - db, c000, db - dg, c001, dg - dr, c011, dr, c111)
        }
    }

    /// Evaluates the 3D LUT using classic trilinear interpolation.
    #[inline]
    pub fn evaluate_trilinear(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let n = self.size as f32;
        let d_min = self.domain_min;
        let d_max = self.domain_max;

        let nr = ((r - d_min[0]) / (d_max[0] - d_min[0]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let ng = ((g - d_min[1]) / (d_max[1] - d_min[1]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);
        let nb = ((b - d_min[2]) / (d_max[2] - d_min[2]).max(1e-6)).clamp(0.0, 1.0) * (n - 1.0);

        let r0 = (nr.floor() as usize).min(self.size.saturating_sub(1));
        let g0 = (ng.floor() as usize).min(self.size.saturating_sub(1));
        let b0 = (nb.floor() as usize).min(self.size.saturating_sub(1));

        let r1 = (r0 + 1).min(self.size.saturating_sub(1));
        let g1 = (g0 + 1).min(self.size.saturating_sub(1));
        let b1 = (b0 + 1).min(self.size.saturating_sub(1));

        let dr = nr - (r0 as f32);
        let dg = ng - (g0 as f32);
        let db = nb - (b0 as f32);

        let s = self.size;
        let get = |rx: usize, gx: usize, bx: usize| -> [f32; 3] {
            let idx = rx + s * (gx + s * bx);
            self.data.get(idx).copied().unwrap_or([0.0, 0.0, 0.0])
        };

        let c000 = get(r0, g0, b0);
        let c100 = get(r1, g0, b0);
        let c010 = get(r0, g1, b0);
        let c110 = get(r1, g1, b0);
        let c001 = get(r0, g0, b1);
        let c101 = get(r1, g0, b1);
        let c011 = get(r0, g1, b1);
        let c111 = get(r1, g1, b1);

        let mut out = [0.0f32; 3];
        for i in 0..3 {
            let c00 = c000[i] * (1.0 - dr) + c100[i] * dr;
            let c10 = c010[i] * (1.0 - dr) + c110[i] * dr;
            let c01 = c001[i] * (1.0 - dr) + c101[i] * dr;
            let c11 = c011[i] * (1.0 - dr) + c111[i] * dr;

            let c0 = c00 * (1.0 - dg) + c10 * dg;
            let c1 = c01 * (1.0 - dg) + c11 * dg;

            out[i] = c0 * (1.0 - db) + c1 * db;
        }

        out
    }

    /// Evaluates 8-bit integer RGB samples through the 3D LUT.
    #[inline]
    pub fn evaluate_u8(&self, r: u8, g: u8, b: u8, mode: InterpolationMode) -> (u8, u8, u8) {
        let fr = r as f32 / 255.0;
        let fg = g as f32 / 255.0;
        let fb = b as f32 / 255.0;

        let [or, og, ob] = self.evaluate(fr, fg, fb, mode);
        ((or * 255.0 + 0.5).clamp(0.0, 255.0) as u8, (og * 255.0 + 0.5).clamp(0.0, 255.0) as u8, (ob * 255.0 + 0.5).clamp(0.0, 255.0) as u8)
    }

    /// Resamples the 3D LUT to a new target lattice resolution (e.g. 17 to 33 or 65).
    pub fn resample(&self, target_size: usize, mode: InterpolationMode) -> Result<Self, LutError> {
        if !(2..=256).contains(&target_size) {
            return Err(LutError::InvalidSize(format!("target size {target_size} out of range (2..=256)")));
        }

        let total = target_size * target_size * target_size;
        let mut new_data = Vec::with_capacity(total);
        let step = 1.0 / (target_size.saturating_sub(1) as f32);

        for b in 0..target_size {
            let fb = b as f32 * step;
            for g in 0..target_size {
                let fg = g as f32 * step;
                for r in 0..target_size {
                    let fr = r as f32 * step;
                    new_data.push(self.evaluate(fr, fg, fb, mode));
                }
            }
        }

        Ok(Self {
            title: format!("{} (Resampled {}x{}x{})", self.title, target_size, target_size, target_size),
            size: target_size,
            domain_min: self.domain_min,
            domain_max: self.domain_max,
            data: new_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_3d_lut() {
        let lut = Lut3D::identity(4).unwrap();
        assert_eq!(lut.size, 4);
        assert_eq!(lut.data.len(), 64);

        let out = lut.evaluate(0.5, 0.5, 0.5, InterpolationMode::Tetrahedral);
        assert!((out[0] - 0.5).abs() < 1e-4);
        assert!((out[1] - 0.5).abs() < 1e-4);
        assert!((out[2] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_cube_and_3dl_and_look_roundtrip() {
        let lut = Lut3D::identity(2).unwrap();

        // 1. .cube roundtrip
        let cube_str = lut.to_cube();
        assert!(cube_str.contains("LUT_3D_SIZE 2"));
        let parsed_cube = Lut3D::parse_cube(&cube_str).unwrap();
        assert_eq!(parsed_cube.size, 2);

        // 2. .3dl roundtrip
        let d3l_str = lut.to_3dl();
        assert!(d3l_str.contains("0 1023"));
        let parsed_3dl = Lut3D::parse_3dl(&d3l_str).unwrap();
        assert_eq!(parsed_3dl.size, 2);

        // 3. .look roundtrip
        let look_str = lut.to_look();
        assert!(look_str.contains("<Look version=\"1.0\">"));
        let parsed_look = Lut3D::parse_look(&look_str).unwrap();
        assert_eq!(parsed_look.size, 2);

        // 4. parse_auto
        let auto1 = Lut3D::parse_auto(&cube_str).unwrap();
        assert_eq!(auto1.size, 2);
        let auto2 = Lut3D::parse_auto(&look_str).unwrap();
        assert_eq!(auto2.size, 2);
    }

    #[test]
    fn test_tetrahedral_vs_trilinear_interpolation() {
        let cube_content = "\
TITLE \"Test Lut\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
        let lut = Lut3D::parse_cube(cube_content).unwrap();

        let tetra = lut.evaluate(0.3, 0.4, 0.5, InterpolationMode::Tetrahedral);
        let trilinear = lut.evaluate(0.3, 0.4, 0.5, InterpolationMode::Trilinear);

        assert!((tetra[0] - 0.3).abs() < 1e-4);
        assert!((tetra[1] - 0.4).abs() < 1e-4);
        assert!((tetra[2] - 0.5).abs() < 1e-4);

        assert!((trilinear[0] - 0.3).abs() < 1e-4);
        assert!((trilinear[1] - 0.4).abs() < 1e-4);
        assert!((trilinear[2] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_resample_lut() {
        let lut = Lut3D::identity(2).unwrap();
        let resampled = lut.resample(4, InterpolationMode::Tetrahedral).unwrap();
        assert_eq!(resampled.size, 4);
        assert_eq!(resampled.data.len(), 64);

        let out = resampled.evaluate(0.5, 0.5, 0.5, InterpolationMode::Tetrahedral);
        assert!((out[0] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_1d_lut_roundtrip() {
        let cube_1d = "\
TITLE \"Linear 1D\"
LUT_1D_SIZE 3
0.0 0.0 0.0
0.5 0.5 0.5
1.0 1.0 1.0
";
        let lut = Lut1D::parse_cube(cube_1d).unwrap();
        assert_eq!(lut.size, 3);
        let out = lut.evaluate(0.25, 0.5, 0.75);
        assert!((out[0] - 0.25).abs() < 1e-4);
        assert!((out[1] - 0.5).abs() < 1e-4);
        assert!((out[2] - 0.75).abs() < 1e-4);

        let serialized = lut.to_cube();
        assert!(serialized.contains("LUT_1D_SIZE 3"));
    }
}
