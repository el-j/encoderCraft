//! Theme design tokens and egui visual styling for encoderCraft.

use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};

/// Supported UI brightness levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brightness {
    #[default]
    Dark,
    MediumDark,
    Light,
    HighContrast,
}

/// Color tokens used throughout all encoderCraft UI panels.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_panel_darker: Color32,
    pub bg_surface: Color32,
    pub bg_surface_hover: Color32,
    pub bg_surface_active: Color32,
    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_dim: Color32,
    pub accent_primary: Color32,
    pub accent_hover: Color32,
    pub status_queued: Color32,
    pub status_processing: Color32,
    pub status_completed: Color32,
    pub status_failed: Color32,
    pub status_paused: Color32,
    pub status_cancelled: Color32,
    pub icon: Color32,
    pub icon_active: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub radius_sm: f32,
}

impl Tokens {
    /// Dark theme tokens matching ArtCraft standards.
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg_app: Color32::from_rgb(27, 27, 27),
            bg_panel: Color32::from_rgb(35, 35, 35),
            bg_panel_darker: Color32::from_rgb(26, 26, 26),
            bg_surface: Color32::from_rgb(45, 45, 45),
            bg_surface_hover: Color32::from_rgb(58, 58, 58),
            bg_surface_active: Color32::from_rgb(68, 68, 68),
            border_subtle: Color32::from_rgb(56, 56, 56),
            border_strong: Color32::from_rgb(84, 84, 84),
            text_primary: Color32::from_rgb(232, 232, 232),
            text_secondary: Color32::from_rgb(176, 176, 176),
            text_dim: Color32::from_rgb(128, 128, 128),
            accent_primary: Color32::from_rgb(14, 134, 255),
            accent_hover: Color32::from_rgb(45, 155, 255),
            status_queued: Color32::from_rgb(140, 140, 150),
            status_processing: Color32::from_rgb(255, 179, 0),
            status_completed: Color32::from_rgb(52, 199, 89),
            status_failed: Color32::from_rgb(255, 69, 58),
            status_paused: Color32::from_rgb(175, 82, 222),
            status_cancelled: Color32::from_rgb(100, 100, 110),
            icon: Color32::from_rgb(180, 180, 190),
            icon_active: Color32::from_rgb(14, 134, 255),
            hover: Color32::from_rgb(52, 52, 58),
            pressed: Color32::from_rgb(60, 60, 68),
            radius_sm: 4.0,
        }
    }

    /// High contrast tokens for enhanced accessibility.
    pub fn high_contrast() -> Self {
        Self {
            dark: true,
            bg_app: Color32::BLACK,
            bg_panel: Color32::from_rgb(12, 12, 12),
            bg_panel_darker: Color32::BLACK,
            bg_surface: Color32::from_rgb(24, 24, 24),
            bg_surface_hover: Color32::from_rgb(40, 40, 40),
            bg_surface_active: Color32::from_rgb(60, 60, 60),
            border_subtle: Color32::WHITE,
            border_strong: Color32::YELLOW,
            text_primary: Color32::WHITE,
            text_secondary: Color32::from_rgb(220, 220, 220),
            text_dim: Color32::from_rgb(160, 160, 160),
            accent_primary: Color32::YELLOW,
            accent_hover: Color32::from_rgb(255, 255, 100),
            status_queued: Color32::WHITE,
            status_processing: Color32::YELLOW,
            status_completed: Color32::GREEN,
            status_failed: Color32::RED,
            status_paused: Color32::from_rgb(255, 180, 255),
            status_cancelled: Color32::GRAY,
            icon: Color32::WHITE,
            icon_active: Color32::YELLOW,
            hover: Color32::from_rgb(40, 40, 40),
            pressed: Color32::from_rgb(60, 60, 60),
            radius_sm: 2.0,
        }
    }

    pub fn for_brightness(b: Brightness) -> Self {
        match b {
            Brightness::Dark | Brightness::MediumDark => Self::dark(),
            Brightness::Light => {
                let mut d = Self::dark();
                d.dark = false;
                d.bg_app = Color32::from_rgb(240, 240, 245);
                d.bg_panel = Color32::from_rgb(255, 255, 255);
                d.bg_panel_darker = Color32::from_rgb(230, 230, 235);
                d.bg_surface = Color32::from_rgb(245, 245, 250);
                d.border_subtle = Color32::from_rgb(210, 210, 220);
                d.text_primary = Color32::from_rgb(20, 20, 25);
                d.text_secondary = Color32::from_rgb(80, 80, 90);
                d.text_dim = Color32::from_rgb(130, 130, 140);
                d
            }
            Brightness::HighContrast => Self::high_contrast(),
        }
    }
}

/// Applies theme tokens to the egui Context.
pub fn apply_theme(ctx: &egui::Context, tokens: &Tokens) {
    let mut v = if tokens.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(4);

    v.panel_fill = tokens.bg_panel;
    v.window_fill = tokens.bg_panel;
    v.extreme_bg_color = tokens.bg_panel_darker;
    v.faint_bg_color = tokens.bg_surface;
    v.window_stroke = Stroke::new(1.0, tokens.border_subtle);
    v.window_corner_radius = r;
    v.menu_corner_radius = r;

    v.widgets.noninteractive.bg_fill = tokens.bg_panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border_subtle);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text_secondary);

    v.widgets.inactive.bg_fill = tokens.bg_surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border_subtle);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, tokens.text_primary);
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = tokens.bg_surface_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.border_strong);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, tokens.text_primary);
    v.widgets.hovered.corner_radius = r;

    v.widgets.active.bg_fill = tokens.bg_surface_active;
    v.widgets.active.bg_stroke = Stroke::new(1.0, tokens.accent_primary);
    v.widgets.active.fg_stroke = Stroke::new(1.0, tokens.text_primary);
    v.widgets.active.corner_radius = r;

    v.selection.bg_fill = tokens.accent_primary;
    v.selection.stroke = Stroke::new(1.0, tokens.text_primary);
    v.override_text_color = Some(tokens.text_primary);

    ctx.set_visuals(v);

    let text_styles: std::collections::BTreeMap<TextStyle, FontId> = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(12.5)),
        (TextStyle::Button, FontId::proportional(12.0)),
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    ctx.global_style_mut(|s| {
        s.text_styles = text_styles;
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = 20.0;
        s.spacing.menu_margin = egui::Margin::symmetric(6, 4);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_is_neutral_grey_without_accent_fills() {
        let t = Tokens::dark();
        for c in [t.bg_app, t.bg_panel, t.bg_panel_darker, t.bg_surface, t.border_subtle] {
            assert!(c.r() == c.g() && c.g() == c.b(), "{c:?} must be neutral grey");
        }
        assert!(t.bg_panel_darker.r() > 16 && t.bg_panel.r() < 60);
        assert_ne!(t.accent_primary, Color32::YELLOW);
    }

    #[test]
    fn default_brightness_is_dark() {
        assert_eq!(Brightness::default(), Brightness::Dark);
    }
}
