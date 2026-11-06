//! Vector icons drawn procedurally in code on a 16×16 design grid (crisp at any DPI,
//! recolourable, zero third-party or proprietary artwork).
//! Follows canonical ArtCraft conventions shared across sibling creative applications.

use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::theme::Tokens;

/// Supported vector icon glyphs for encoderCraft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Stop,
    StepBack,
    StepFwd,
    Plus,
    Minus,
    Trash,
    Duplicate,
    ArrowUp,
    ArrowDown,
    Reset,
    ChevronRight,
    ChevronDown,
    ChevronLeft,
    Folder,
    Gear,
    Search,
    Filter,
    Preset,
    Link,
    Clock,
    Check,
    Close,
    Sliders,
    ListView,
    GridView,
    Info,
}

/// 16×16 grid pixel-snapped vector painter for crisp UI rendering.
pub struct Pen16<'a> {
    painter: &'a Painter,
    rect: Rect,
    color: Color32,
    width: f32,
}

impl Pen16<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        let s = self.rect.width().min(self.rect.height()) / 16.0;
        let o = self.rect.center() - vec2(8.0 * s, 8.0 * s);
        pos2(o.x + x * s, o.y + y * s)
    }

    fn s(&self) -> f32 {
        self.rect.width().min(self.rect.height()) / 16.0
    }

    /// Stroke width in points covering a whole number of device pixels, at least 1.0.
    fn stroke_w(&self) -> f32 {
        let ppp = self.painter.pixels_per_point();
        (self.width * self.s() * ppp - 0.5).ceil().max(1.0) / ppp
    }

    fn stroke(&self) -> PathStroke {
        PathStroke::new(self.stroke_w(), self.color)
    }

    /// Snap one coordinate (points) to device pixel boundary or center.
    fn snap(&self, v: f32, centre: f32, mid: bool) -> f32 {
        let ppp = self.painter.pixels_per_point();
        let half = if mid && (self.stroke_w() * ppp).round() as i32 % 2 == 1 { 0.5 } else { 0.0 };
        let base = (centre * ppp).round() + half;
        (base + ((v - centre) * ppp).round()) / ppp
    }

    fn snap_pos(&self, q: Pos2, mid_x: bool, mid_y: bool) -> Pos2 {
        let c = self.rect.center();
        pos2(self.snap(q.x, c.x, mid_x), self.snap(q.y, c.y, mid_y))
    }

    /// Grid points snapped to device pixels along straight segments for razor sharpness.
    fn crisp(&self, pts: &[(f32, f32)], closed: bool) -> Vec<Pos2> {
        let n = pts.len();
        if n == 0 {
            return Vec::new();
        }
        let ppp = self.painter.pixels_per_point();
        let q: Vec<Pos2> = pts.iter().map(|(x, y)| self.p(*x, *y)).collect();
        let seg = |i: usize, j: usize| {
            let d = (q[i] - q[j]) * ppp;
            (d.x.abs() < 1.0, d.y.abs() < 1.0)
        };
        (0..n)
            .map(|i| {
                let mut nb = Vec::new();
                if i > 0 || closed {
                    nb.push(seg((i + n - 1) % n, i));
                }
                if i + 1 < n || closed {
                    nb.push(seg(i, (i + 1) % n));
                }
                let pt = q[i];
                let vert = nb.iter().any(|s| s.0);
                let horiz = nb.iter().any(|s| s.1);
                if !vert && !horiz {
                    return pt;
                }
                self.snap_pos(pt, vert, horiz)
            })
            .collect()
    }

    fn line(&self, pts: &[(f32, f32)]) {
        let v = self.crisp(pts, false);
        if v.len() >= 2 {
            self.painter.add(PathShape::line(v, self.stroke()));
        }
    }

    fn closed(&self, pts: &[(f32, f32)]) {
        let v = self.crisp(pts, true);
        if v.len() >= 3 {
            self.painter.add(PathShape::closed_line(v, self.stroke()));
        }
    }

    fn fill(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.p(*x, *y)).collect();
        if v.len() >= 3 {
            self.painter.add(PathShape::convex_polygon(v, self.color, Stroke::NONE));
        }
    }

    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_stroke(self.p(x, y), r * self.s(), Stroke::new(self.stroke_w(), self.color));
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(self.p(x, y), r * self.s(), self.color);
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let r = Rect::from_min_max(self.snap_pos(self.p(x0, y0), true, true), self.snap_pos(self.p(x1, y1), true, true));
        self.painter.rect_stroke(r, 1.0 * self.s(), Stroke::new(self.stroke_w(), self.color), egui::StrokeKind::Middle);
    }

    fn rect_fill(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let r = Rect::from_min_max(self.snap_pos(self.p(x0, y0), false, false), self.snap_pos(self.p(x1, y1), false, false));
        self.painter.rect_filled(r, 0.8 * self.s(), self.color);
    }

    fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
        let n = 20;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.line(&pts);
    }
}

/// Paint `icon` centered in `rect` using `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let pen = Pen16 { painter, rect, color, width: 1.25 };
    use Icon::*;
    match icon {
        Play => pen.fill(&[(4.5, 2.8), (13.0, 8.0), (4.5, 13.2)]),
        Pause => {
            pen.rect_fill(4.0, 3.0, 6.8, 13.0);
            pen.rect_fill(9.2, 3.0, 12.0, 13.0);
        }
        Stop => pen.rect_fill(3.5, 3.5, 12.5, 12.5),
        StepBack => {
            pen.rect_fill(3.5, 3.5, 5.0, 12.5);
            pen.fill(&[(12.5, 3.5), (6.0, 8.0), (12.5, 12.5)]);
        }
        StepFwd => {
            pen.rect_fill(11.0, 3.5, 12.5, 12.5);
            pen.fill(&[(3.5, 3.5), (10.0, 8.0), (3.5, 12.5)]);
        }
        Plus => {
            pen.line(&[(8.0, 3.0), (8.0, 13.0)]);
            pen.line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Minus => {
            pen.line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Trash => {
            pen.line(&[(2.5, 4.0), (13.5, 4.0)]);
            pen.line(&[(6.0, 4.0), (6.5, 2.5), (9.5, 2.5), (10.0, 4.0)]);
            pen.closed(&[(3.8, 4.0), (12.2, 4.0), (11.4, 14.0), (4.6, 14.0)]);
        }
        Duplicate => {
            pen.rect(2.5, 5.0, 10.5, 13.5);
            pen.line(&[(5.5, 5.0), (5.5, 2.5), (13.5, 2.5), (13.5, 10.5), (10.5, 10.5)]);
        }
        ArrowUp => {
            pen.line(&[(8.0, 13.5), (8.0, 3.0)]);
            pen.line(&[(4.0, 7.0), (8.0, 3.0), (12.0, 7.0)]);
        }
        ArrowDown => {
            pen.line(&[(8.0, 2.5), (8.0, 13.0)]);
            pen.line(&[(4.0, 9.0), (8.0, 13.0), (12.0, 9.0)]);
        }
        Reset => {
            pen.arc(8.0, 8.5, 5.0, 200.0, 500.0);
            pen.fill(&[(1.5, 6.0), (5.5, 5.5), (3.0, 9.0)]);
        }
        ChevronRight => pen.line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)]),
        ChevronDown => pen.line(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)]),
        ChevronLeft => pen.line(&[(10.0, 4.0), (6.0, 8.0), (10.0, 12.0)]),
        Folder => pen.closed(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)]),
        Gear => {
            pen.circle(8.0, 8.0, 2.2);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                pen.line(&[(8.0 + 4.0 * a.cos(), 8.0 + 4.0 * a.sin()), (8.0 + 6.0 * a.cos(), 8.0 + 6.0 * a.sin())]);
            }
            pen.circle(8.0, 8.0, 4.2);
        }
        Search => {
            pen.circle(6.8, 6.8, 4.3);
            pen.line(&[(10.0, 10.0), (14.0, 14.0)]);
        }
        Filter => pen.fill(&[(2.0, 3.0), (14.0, 3.0), (9.0, 8.0), (9.0, 13.0), (7.0, 14.0), (7.0, 8.0)]),
        Preset => {
            pen.fill(&[(9.0, 2.0), (4.5, 8.5), (8.0, 8.5), (7.0, 14.0), (11.5, 7.5), (8.0, 7.5)]);
        }
        Link => {
            pen.closed(&[(2.0, 6.0), (8.0, 6.0), (8.0, 10.0), (2.0, 10.0)]);
            pen.closed(&[(8.0, 6.0), (14.0, 6.0), (14.0, 10.0), (8.0, 10.0)]);
        }
        Clock => {
            pen.circle(8.0, 8.0, 6.0);
            pen.line(&[(8.0, 4.5), (8.0, 8.0), (10.5, 9.5)]);
        }
        Check => {
            pen.line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)]);
        }
        Close => {
            pen.line(&[(4.0, 4.0), (12.0, 12.0)]);
            pen.line(&[(12.0, 4.0), (4.0, 12.0)]);
        }
        Sliders => {
            for (y, x) in [(4.5, 6.0), (8.0, 11.0), (11.5, 4.5)] {
                pen.line(&[(2.0, y), (14.0, y)]);
                pen.dot(x, y, 1.4);
            }
        }
        ListView => {
            for y in [4.0, 8.0, 12.0] {
                pen.dot(3.0, y, 0.8);
                pen.line(&[(5.5, y), (14.0, y)]);
            }
        }
        GridView => {
            for y in [2.5, 7.0, 11.5] {
                for x in [2.5, 7.0, 11.5] {
                    pen.rect(x, y, x + 2.0, y + 2.0);
                }
            }
        }
        Info => {
            pen.circle(8.0, 8.0, 6.0);
            pen.line(&[(8.0, 7.0), (8.0, 11.5)]);
            pen.dot(8.0, 4.8, 0.9);
        }
    }
}

/// Draws an icon button: returns the response; `active` draws the highlighted state.
pub fn button(ui: &mut egui::Ui, icon: Icon, size: f32, active: bool, t: &Tokens, tooltip: &str) -> egui::Response {
    let enabled = ui.is_enabled();
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), sense);
    let bg = if enabled && resp.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if enabled && resp.hovered() {
        Some(t.hover)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, t.radius_sm, bg);
    }
    let col = if !enabled {
        t.text_dim
    } else if active {
        t.icon_active
    } else if resp.hovered() {
        t.text_primary
    } else {
        t.icon
    };
    paint(ui.painter(), rect.shrink(size * 0.18), icon, col);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// Draws a small square icon button (18×18 points).
pub fn small_button(ui: &mut egui::Ui, icon: Icon, t: &Tokens, tooltip: &str) -> egui::Response {
    button(ui, icon, 18.0, false, t, tooltip)
}

/// Draws a button with an icon and label text.
pub fn button_with_label(ui: &mut egui::Ui, icon: Icon, label: &str, active: bool, t: &Tokens, tooltip: &str) -> egui::Response {
    button_with_label_tinted(ui, icon, label, ButtonTint { active, enabled: None, icon_color: None }, t, tooltip)
}

pub fn button_with_label_enabled(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    active: bool,
    enabled: bool,
    t: &Tokens,
    tooltip: &str,
) -> egui::Response {
    button_with_label_tinted(ui, icon, label, ButtonTint { active, enabled: Some(enabled), icon_color: None }, t, tooltip)
}

pub fn button_with_label_color_enabled(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    icon_color: Color32,
    t: &Tokens,
    tooltip: &str,
) -> egui::Response {
    button_with_label_tinted(ui, icon, label, ButtonTint { active: false, enabled: Some(enabled), icon_color: Some(icon_color) }, t, tooltip)
}

struct ButtonTint {
    active: bool,
    enabled: Option<bool>,
    icon_color: Option<Color32>,
}

fn button_with_label_tinted(ui: &mut egui::Ui, icon: Icon, label: &str, tint: ButtonTint, t: &Tokens, tooltip: &str) -> egui::Response {
    let icon_size = 14.0;
    let padding = vec2(7.0, 3.5);
    let gap = 5.0;
    let enabled = ui.is_enabled() && tint.enabled.unwrap_or(true);
    let label_color = if !enabled {
        t.text_dim
    } else if tint.active {
        t.icon_active
    } else {
        t.text_primary
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(12.0), label_color);
    let desired_size = vec2(padding.x * 2.0 + icon_size + gap + galley.size().x, (galley.size().y.max(icon_size) + padding.y * 2.0).max(22.0));
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(desired_size, sense);
    let bg = if enabled && resp.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if enabled && resp.hovered() {
        Some(t.hover)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, t.radius_sm, bg);
    }
    let icon_color = if !enabled {
        t.text_dim
    } else if let Some(color) = tint.icon_color {
        color
    } else if tint.active {
        t.icon_active
    } else if resp.hovered() {
        t.text_primary
    } else {
        t.icon
    };
    let icon_rect = Rect::from_min_size(pos2(rect.min.x + padding.x, rect.center().y - icon_size * 0.5), vec2(icon_size, icon_size));
    paint(ui.painter(), icon_rect, icon, icon_color);

    let text_pos = pos2(rect.min.x + padding.x + icon_size + gap, rect.center().y - galley.size().y * 0.5);
    let text_color = if !enabled {
        t.text_dim
    } else if tint.active {
        t.icon_active
    } else if resp.hovered() {
        t.text_primary
    } else {
        t.text_secondary
    };
    ui.painter().galley(text_pos, galley, text_color);

    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// Paints a standalone icon inline with specified size.
pub fn icon_label(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    paint(ui.painter(), rect, icon, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{ColorMode, Shape};

    const ALL_ICONS: &[Icon] = &[
        Icon::Play,
        Icon::Pause,
        Icon::Stop,
        Icon::StepBack,
        Icon::StepFwd,
        Icon::Plus,
        Icon::Minus,
        Icon::Trash,
        Icon::Duplicate,
        Icon::ArrowUp,
        Icon::ArrowDown,
        Icon::Reset,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::ChevronLeft,
        Icon::Folder,
        Icon::Gear,
        Icon::Search,
        Icon::Filter,
        Icon::Preset,
        Icon::Link,
        Icon::Clock,
        Icon::Check,
        Icon::Close,
        Icon::Sliders,
        Icon::ListView,
        Icon::GridView,
        Icon::Info,
    ];

    fn drawn_shapes(icon: Icon, ppp: f32, size: f32) -> Vec<Shape> {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(ppp);
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint(ui.painter(), Rect::from_min_size(pos2(10.0, 8.0), vec2(size, size)), icon, Color32::WHITE);
            });
            out.textures_delta.clear();
            shapes = out.shapes.into_iter().map(|c| c.shape).collect();
        }
        fn flat(s: Shape, out: &mut Vec<Shape>) {
            match s {
                Shape::Vec(v) => v.into_iter().for_each(|s| flat(s, out)),
                s => out.push(s),
            }
        }
        let mut out = Vec::new();
        shapes.into_iter().for_each(|s| flat(s, &mut out));
        out
    }

    #[test]
    fn all_icons_render_without_panicking() {
        for ppp in [1.0, 1.5, 2.0] {
            for &icon in ALL_ICONS {
                let shapes = drawn_shapes(icon, ppp, 16.0);
                assert!(!shapes.is_empty(), "Icon {icon:?} must render at {ppp}x");
            }
        }
    }

    #[test]
    fn icons_have_valid_strokes() {
        for ppp in [1.0, 1.5, 2.0] {
            for &icon in ALL_ICONS {
                for shape in drawn_shapes(icon, ppp, 16.0) {
                    if let Shape::Path(p) = shape
                        && p.stroke.width > 0.0
                        && !matches!(p.stroke.color, ColorMode::Solid(c) if c == Color32::TRANSPARENT)
                    {
                        assert!(p.stroke.width >= 0.5, "Stroke width too thin for {icon:?} at {ppp}x");
                    }
                }
            }
        }
    }
}
