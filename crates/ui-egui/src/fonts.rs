//! UI fonts: the font stack every Craft app installs, craft-ui's [`UiFonts`]. Font binaries are never committed (they
//! live in `storytold/craft-fonts`): `build.rs` embeds craft-fonts' faces from `CRAFT_FONTS_DIR` with craft-fonts-build,
//! and characters no embedded face covers come from the installed system fonts, loaded per script when text first needs
//! them. The stack orders the faces for the interface language, so a Japanese interface draws kanji in their Japanese
//! forms, and CJK faces sit on the Latin face's baseline in rows of its height.

use std::sync::{Arc, LazyLock};

use craft_ui::fonts::{EmbeddedFace, Family, I18nScripts, UiFonts};
use egui::{Context, FontId};

/// craft-fonts faces; empty unless built with `CRAFT_FONTS_DIR`.
pub static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// Every non-ASCII character the UI itself draws. Each must be covered by the default faces.
pub const UI_GLYPHS: &str = "·…©";

/// Every language the interface offers, which the stack also keeps faces for.
static LANGUAGES: LazyLock<Vec<&'static str>> = LazyLock::new(|| crate::i18n::languages().into_iter().map(|(code, _)| code).collect());

/// The interface font stack for the UI language `language` (a craft-i18n code such as `ja`).
pub fn ui_fonts(language: &str) -> UiFonts<'static> {
    let fonts = UiFonts::new(&I18nScripts).craft_faces(CRAFT_FONTS).family(Family::monospace()).languages(&LANGUAGES).ui_language(language);
    // The web has no installed fonts to read.
    #[cfg(not(target_arch = "wasm32"))]
    let fonts = fonts.on_demand(Arc::new(craft_ui::fonts::SystemCandidates));
    fonts
}

/// Installs the interface fonts for the UI language `language`; install again when it changes.
pub fn install(ctx: &Context, language: &str) {
    ui_fonts(language).install(ctx);
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
        for language in ["en", "ja", "not-a-language"] {
            install(&ctx, language);
        }
    }

    #[test]
    fn a_japanese_interface_draws_japanese_with_the_craft_faces() {
        // Builds without craft-fonts take Japanese from the installed fonts, which differ per machine.
        if !CRAFT_FONTS.iter().any(|face| face.has_script("Jpan")) {
            return;
        }
        let sample = craft_ui::fonts::Sample::new("ja", "japanese", "メディアファイルを追加… 環境設定");
        craft_ui::fonts::assert_covers(&ui_fonts("ja").stack(), [sample]);
    }

    #[test]
    fn a_menu_mnemonic_sits_on_the_baseline_of_its_japanese_label() {
        // Japanese from the installed fonts grows the row, and epaint moves only the sections that hold it.
        if !CRAFT_FONTS.iter().any(|face| face.has_script("Jpan")) {
            return;
        }
        let ctx = Context::default();
        install(&ctx, "ja");
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        // craft-menu draws "テーマ: ダーク(D)" as three sections: the label, the underlined letter, the rest.
        let mut job = egui::text::LayoutJob::default();
        let format = egui::TextFormat::simple(FontId::proportional(12.5), egui::Color32::WHITE);
        for (part, underline) in [("テーマ: ダーク(", 0.0), ("D", 1.0), (")", 0.0)] {
            job.append(part, 0.0, egui::TextFormat { underline: egui::Stroke::new(underline, egui::Color32::WHITE), ..format.clone() });
        }
        let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
        let baselines: Vec<(char, f32)> = galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|glyph| (glyph.chr, glyph.pos.y))).collect();
        assert_eq!(baselines.len(), 11, "every character of the label is laid out: {baselines:?}");
        let first = baselines[0].1;
        for (character, baseline) in &baselines {
            assert!((baseline - first).abs() <= 0.5, "{character:?} is drawn on {baseline}, the row's baseline is {first}: {baselines:?}");
        }
    }

    #[test]
    fn craft_fonts_provide_multiscript_fallbacks_when_configured() {
        if CRAFT_FONTS.is_empty() {
            return;
        }
        let ctx = Context::default();
        install(&ctx, "en");
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
