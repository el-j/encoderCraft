//! Read-only inspection of project-like sources: which sequences, pages and layers can be selected.

use std::io::Read;
use std::path::Path;

use crate::document::DocumentCanvas;
use crate::project::{MotionProject, TimelineProject};

/// Largest source file inspected; bigger files are reported as having no selectable content.
pub const MAX_INSPECT_BYTES: u64 = 256 * 1024 * 1024;

/// One selectable sequence or composition.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_secs: Option<f64>,
}

/// Everything in a source that a job can restrict itself to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceInventory {
    pub sequences: Vec<SequenceInfo>,
    pub pages: Option<usize>,
    pub layers: Vec<String>,
}

impl SourceInventory {
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty() && self.pages.is_none() && self.layers.is_empty()
    }
}

/// Inspects a file on disk. Unreadable, oversized or unrecognised files yield an empty inventory.
pub fn inspect_source(path: &Path) -> SourceInventory {
    let Ok(meta) = std::fs::metadata(path) else {
        return SourceInventory::default();
    };
    if !meta.is_file() || meta.len() > MAX_INSPECT_BYTES {
        return SourceInventory::default();
    }
    let mut bytes = Vec::new();
    let Ok(file) = std::fs::File::open(path) else {
        return SourceInventory::default();
    };
    if file.take(MAX_INSPECT_BYTES).read_to_end(&mut bytes).is_err() {
        return SourceInventory::default();
    }
    inspect_bytes(&bytes)
}

/// Inspects an in-memory source.
pub fn inspect_bytes(bytes: &[u8]) -> SourceInventory {
    if bytes.starts_with(b"\x89CRAFT\x00\x01F") {
        if let Ok(p) = TimelineProject::from_filmcraft_binary(bytes) {
            return timeline_inventory(&p);
        }
    } else if bytes.starts_with(&[0x1f, 0x8b]) || bytes.windows(13).any(|w| w == b"<PremiereData") {
        if let Ok(p) = TimelineProject::from_prproj(bytes) {
            return timeline_inventory(&p);
        }
    } else if bytes.starts_with(b"\x89CRAFT\x00\x01E") || bytes.starts_with(b"RIFX") {
        let project = if bytes.starts_with(b"RIFX") { MotionProject::from_aep(bytes) } else { MotionProject::from_effectcraft_binary(bytes) };
        if let Ok(p) = project {
            return SourceInventory {
                sequences: p
                    .comps
                    .iter()
                    .enumerate()
                    .map(|(index, c)| SequenceInfo {
                        index,
                        name: c.name.clone(),
                        width: c.width,
                        height: c.height,
                        fps: c.fps,
                        duration_secs: (c.fps > 0.0).then(|| c.duration_frames as f64 / c.fps),
                    })
                    .collect(),
                ..SourceInventory::default()
            };
        }
    } else if let Ok(doc) = DocumentCanvas::from_bytes_auto(bytes) {
        return SourceInventory { pages: Some(doc.pages.len()), ..SourceInventory::default() };
    } else if let Ok(cad) = encodercraft_spatial_io::CadDrawing::from_bytes_auto(bytes) {
        return SourceInventory { layers: cad.layer_names(), ..SourceInventory::default() };
    } else if let Ok(image) = encodercraft_image_codecs::LayeredImage::from_bytes_auto(bytes) {
        return SourceInventory { layers: image.layer_names(), ..SourceInventory::default() };
    }
    SourceInventory::default()
}

fn timeline_inventory(p: &TimelineProject) -> SourceInventory {
    SourceInventory {
        sequences: p
            .sequences
            .iter()
            .enumerate()
            .map(|(index, s)| SequenceInfo {
                index,
                name: s.name.clone(),
                width: s.width,
                height: s.height,
                fps: s.fps,
                duration_secs: (s.timebase > 0 && s.duration_ticks > 0).then(|| s.duration_ticks as f64 / s.timebase as f64),
            })
            .collect(),
        ..SourceInventory::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{MotionComp, TimelineSequence};

    #[test]
    fn timeline_and_motion_sequences_are_listed() {
        let mut t = TimelineProject::new("P");
        t.add_sequence(TimelineSequence::new("Main", 1920, 1080, 25.0));
        t.add_sequence(TimelineSequence::new("Social", 1080, 1920, 30.0));
        let inv = inspect_bytes(&t.to_prproj());
        assert_eq!(inv.sequences.len(), 2);
        assert_eq!(inv.sequences[1].name, "Social");
        assert_eq!(inv.sequences[1].height, 1920);
        let inv = inspect_bytes(&t.to_filmcraft_binary().unwrap());
        assert_eq!(inv.sequences.len(), 2);

        let mut m = MotionProject::new("M");
        m.add_comp(MotionComp::new("A", 1280, 720, 24.0, 48));
        m.add_comp(MotionComp::new("B", 640, 360, 30.0, 60));
        let inv = inspect_bytes(&m.to_effectcraft_binary().unwrap());
        assert_eq!(inv.sequences.len(), 2);
        assert_eq!(inv.sequences[0].duration_secs, Some(2.0));
    }

    #[test]
    fn document_pages_and_layers_are_listed() {
        let mut d = DocumentCanvas::new("D");
        d.add_page();
        let inv = inspect_bytes(&d.to_designcraft_binary().unwrap());
        assert_eq!(inv.pages, Some(d.pages.len()));

        let mut cad = encodercraft_spatial_io::CadDrawing::new("C");
        cad.add_line(encodercraft_spatial_io::CadPoint3D::new(0.0, 0.0, 0.0), encodercraft_spatial_io::CadPoint3D::new(1.0, 1.0, 0.0), "WALLS");
        let inv = inspect_bytes(&cad.to_cadcraft_binary().unwrap());
        assert_eq!(inv.layers, vec!["0".to_string(), "WALLS".to_string()]);
    }

    #[test]
    fn unrecognised_and_missing_files_are_empty() {
        assert!(inspect_bytes(b"hello world").is_empty());
        assert!(inspect_bytes(&[]).is_empty());
        assert!(inspect_source(Path::new("/definitely/not/here.xyz")).is_empty());
    }
}
