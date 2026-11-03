//! Table view, search and sharing of presets: derived columns, filtering, sorting and a versioned JSON bundle.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

use crate::model::{EncodingPreset, MatchSourceOr, PresetCategory};

pub const BUNDLE_VERSION: u32 = 1;
const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
const MAX_BUNDLE_PRESETS: usize = 1000;
const MAX_NAME_CHARS: usize = 120;

/// One row of the preset table with every column rendered as text.
#[derive(Debug, Clone, PartialEq)]
pub struct PresetRow {
    pub name: String,
    pub format: String,
    pub frame_size: String,
    pub frame_rate: String,
    pub target_rate: String,
    pub comment: String,
    pub category: PresetCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Format,
    FrameSize,
    FrameRate,
    TargetRate,
}

impl PresetRow {
    pub fn from_preset(p: &EncodingPreset) -> Self {
        let (frame_size, frame_rate, target_rate) = match &p.video {
            Some(v) => (
                match (v.width, v.height) {
                    (MatchSourceOr::Exact(w), MatchSourceOr::Exact(h)) => format!("{w}x{h}"),
                    _ => "Match Source".to_string(),
                },
                match v.frame_rate {
                    MatchSourceOr::Exact(r) if r.den > 0 => format!("{:.3} fps", r.num as f64 / r.den as f64),
                    _ => "Match Source".to_string(),
                },
                if v.target_bitrate_kbps > 0 { format!("{:.1} Mbps", f64::from(v.target_bitrate_kbps) / 1000.0) } else { "Auto".to_string() },
            ),
            None => {
                let rate = p.audio.as_ref().and_then(|a| a.bitrate_kbps).map(|k| format!("{k} kbps")).unwrap_or_else(|| "-".into());
                ("-".into(), "-".into(), rate)
            }
        };
        Self {
            name: p.name.clone(),
            format: format!("{:?}", p.container),
            frame_size,
            frame_rate,
            target_rate,
            comment: p.description.clone(),
            category: p.category,
        }
    }
}

pub fn rows(presets: &[EncodingPreset]) -> Vec<PresetRow> {
    presets.iter().map(PresetRow::from_preset).collect()
}

/// Case-insensitive search over name, format and comment, optionally within one category.
pub fn filter_rows(rows: Vec<PresetRow>, query: &str, category: Option<PresetCategory>) -> Vec<PresetRow> {
    let q = query.trim().to_lowercase();
    rows.into_iter()
        .filter(|r| category.is_none_or(|c| r.category == c))
        .filter(|r| q.is_empty() || [&r.name, &r.format, &r.comment].iter().any(|f| f.to_lowercase().contains(&q)))
        .collect()
}

pub fn sort_rows(rows: &mut [PresetRow], column: SortColumn, descending: bool) {
    rows.sort_by(|a, b| {
        let (x, y) = match column {
            SortColumn::Name => (&a.name, &b.name),
            SortColumn::Format => (&a.format, &b.format),
            SortColumn::FrameSize => (&a.frame_size, &b.frame_size),
            SortColumn::FrameRate => (&a.frame_rate, &b.frame_rate),
            SortColumn::TargetRate => (&a.target_rate, &b.target_rate),
        };
        x.to_lowercase().cmp(&y.to_lowercase())
    });
    if descending {
        rows.reverse();
    }
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    version: u32,
    presets: Vec<EncodingPreset>,
}

/// Serialises presets into the shareable encoderCraft JSON bundle. Other vendors' preset formats are not supported.
pub fn export_json(presets: &[EncodingPreset]) -> Result<String, EncoderError> {
    serde_json::to_string_pretty(&Bundle { version: BUNDLE_VERSION, presets: presets.to_vec() })
        .map_err(|e| EncoderError::Internal(format!("failed to serialize presets: {e}")))
}

/// Parses and validates a bundle. Oversized, newer-version or malformed bundles are rejected.
pub fn import_json(text: &str) -> Result<Vec<EncodingPreset>, EncoderError> {
    if text.len() > MAX_BUNDLE_BYTES {
        return Err(EncoderError::Internal("preset bundle is too large".into()));
    }
    let bundle: Bundle = serde_json::from_str(text).map_err(|e| EncoderError::Internal(format!("not an encoderCraft preset bundle: {e}")))?;
    if bundle.version == 0 || bundle.version > BUNDLE_VERSION {
        return Err(EncoderError::Internal(format!("unsupported preset bundle version {}", bundle.version)));
    }
    if bundle.presets.len() > MAX_BUNDLE_PRESETS {
        return Err(EncoderError::Internal("preset bundle has too many presets".into()));
    }
    for p in &bundle.presets {
        let n = p.name.trim();
        if n.is_empty() || n.chars().count() > MAX_NAME_CHARS || n.chars().any(char::is_control) {
            return Err(EncoderError::Internal(format!("invalid preset name '{}'", p.name.escape_debug())));
        }
    }
    Ok(bundle.presets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory_presets;

    #[test]
    fn every_factory_preset_has_a_complete_row() {
        for r in rows(&factory_presets()) {
            assert!(!r.name.is_empty() && !r.format.is_empty() && !r.frame_size.is_empty() && !r.target_rate.is_empty(), "{r:?}");
        }
    }

    #[test]
    fn search_and_category_filter() {
        let all = rows(&factory_presets());
        let hits = filter_rows(all.clone(), "PNG", None);
        assert!(!hits.is_empty() && hits.iter().all(|r| [&r.name, &r.format, &r.comment].iter().any(|f| f.to_lowercase().contains("png"))));
        let audio = filter_rows(all.clone(), "", Some(PresetCategory::Audio));
        assert!(audio.iter().all(|r| r.category == PresetCategory::Audio));
        assert!(filter_rows(all, "zzz-no-such-preset", None).is_empty());
    }

    #[test]
    fn sorting_is_case_insensitive_and_reversible() {
        let mut r = rows(&factory_presets());
        sort_rows(&mut r, SortColumn::Name, false);
        assert!(r.windows(2).all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
        sort_rows(&mut r, SortColumn::Name, true);
        assert!(r.windows(2).all(|w| w[0].name.to_lowercase() >= w[1].name.to_lowercase()));
    }

    #[test]
    fn bundle_round_trip() {
        let presets = factory_presets();
        let back = import_json(&export_json(&presets).unwrap()).unwrap();
        assert_eq!(back, presets);
    }

    #[test]
    fn bad_bundles_are_rejected() {
        assert!(import_json("{").is_err());
        assert!(import_json(r#"{"version":99,"presets":[]}"#).is_err());
        assert!(import_json(&"x".repeat(MAX_BUNDLE_BYTES + 1)).is_err());
        let mut p = factory_presets();
        p.truncate(1);
        p[0].name = "bad\nname".into();
        assert!(import_json(&export_json(&p).unwrap()).is_err());
        p[0].name = " ".into();
        assert!(import_json(&export_json(&p).unwrap()).is_err());
    }
}
