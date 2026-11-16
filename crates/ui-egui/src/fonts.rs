//! UI font setup. Font binaries are never committed (they live in `storytold/craft-fonts`), so the
//! bundled egui faces cover the Latin UI and installed system faces are appended as a fallback
//! chain for symbols and CJK text found in file and preset names.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};

struct CraftFont {
    family: &'static str,
    style: &'static str,
    scripts: &'static [&'static str],
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

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
    for (index, font) in CRAFT_FONTS.iter().enumerate() {
        let name = format!("craft-font-{}-{}-{}-{index}", font.family, font.style, font.scripts.join("-"));
        defs.font_data.insert(name.clone(), Arc::new(FontData::from_static(font.bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
    }
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

pub fn check_static_glyphs(ctx: &Context) {
    let presets = encodercraft_presets::PresetRegistry::default();
    let mut labels: Vec<String> = crate::commands::Menu::ALL.into_iter().map(|menu| menu.label().to_owned()).collect();
    labels.extend(crate::commands::COMMANDS.iter().map(|command| command.label.to_owned()));
    labels.extend(crate::workspace::PanelId::ALL.into_iter().map(|panel| panel.label().to_owned()));
    labels.extend(presets.list().iter().map(|preset| preset.name.clone()));
    labels.extend(["Edit File Queue", "Match Source - High Bitrate", UI_GLYPHS].into_iter().map(str::to_owned));

    let font_id = FontId::proportional(12.5);
    ctx.fonts_mut(|fonts| {
        for label in &labels {
            for character in label.chars() {
                if !fonts.has_glyph(&font_id, character) {
                    log::warn!("UI font fallback is missing U+{:04X} in {label:?}", character as u32);
                }
            }
        }
    });
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

    #[test]
    fn craft_fonts_provide_multiscript_fallbacks_when_configured() {
        if CRAFT_FONTS.is_empty() {
            return;
        }
        let ctx = Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.label("日本語 한국어 עברית العربية ไทย ქართული हिन्दी ✕◀✓");
        });
        output.textures_delta.clear();
        let id = FontId::proportional(12.5);
        for character in "日本語한국어עבריתالعربيةไทยქართულიहिन्दी✕◀✓".chars() {
            assert!(ctx.fonts_mut(|fonts| fonts.has_glyph(&id, character)), "missing fallback glyph U+{:04X}", character as u32);
        }
    }
}
