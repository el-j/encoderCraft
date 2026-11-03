//! UI font setup. Font binaries are never committed (they live in `storytold/craft-fonts`), so the
//! bundled egui faces cover the Latin UI and installed system faces are appended as a fallback
//! chain for symbols and CJK text found in file and preset names.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily};

/// Every non-ASCII character the UI itself draws. Each must be covered by the default faces.
pub const UI_GLYPHS: &str = "·…©";

const FALLBACKS: &[(&str, &str)] = &[
    ("system-symbols", "/System/Library/Fonts/Apple Symbols.ttf"),
    ("system-cjk", "/System/Library/Fonts/PingFang.ttc"),
    ("system-symbols", "C:\\Windows\\Fonts\\seguisym.ttf"),
    ("system-cjk", "C:\\Windows\\Fonts\\msyh.ttc"),
    ("system-symbols", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ("system-cjk", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ("system-cjk", "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
];

/// Appends the first installed face for each fallback slot to every font family.
pub fn install(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    let mut added: Vec<&str> = Vec::new();
    for (slot, path) in FALLBACKS {
        if added.contains(slot) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else { continue };
        defs.font_data.insert((*slot).to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().push((*slot).to_owned());
        }
        added.push(slot);
    }
    ctx.set_fonts(defs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_faces_cover_every_glyph_the_ui_draws() {
        let ctx = Context::default();
        for _ in 0..2 {
            let mut out = ctx.run_ui(Default::default(), |_| {});
            out.textures_delta.clear();
        }
        let id = egui::FontId::proportional(12.0);
        for c in "Edit File Queue".chars().chain(UI_GLYPHS.chars()) {
            assert!(ctx.fonts_mut(|f| f.has_glyph(&id, c)), "missing glyph {c:?} (U+{:04X})", c as u32);
        }
    }

    #[test]
    fn install_never_fails_without_system_fonts() {
        let ctx = Context::default();
        install(&ctx);
    }
}
