//! The window's own title bar. The menu bar doubles as the title bar:
//!
//! * [`TitleBarStyle::Custom`] (Windows, and Linux where the compositor leaves the frame to the
//!   app): the window has no OS decorations, empty space in the bar drags it, a double-click
//!   maximizes it, caption buttons at the right end minimize, maximize/restore and close, and thin
//!   invisible zones along the edges resize it.
//! * [`TitleBarStyle::Integrated`] (macOS): the OS keeps its traffic lights over a transparent
//!   title strip; the bar leaves room for them and only its drag area is used.
//! * [`TitleBarStyle::System`]: the OS draws the frame and the bar is a plain menu bar.
//!
//! The behaviour follows vectorCraft's title bar (MIT OR Apache-2.0).

use egui::{Color32, CursorIcon, Id, LayerId, Order, PointerButton, Pos2, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportCommand, pos2, vec2};

use crate::theme::Tokens;

/// One caption button: as wide as the native Windows ones.
pub const BUTTON_WIDTH: f32 = 46.0;
/// The three caption buttons together.
pub const WIDTH: f32 = 3.0 * BUTTON_WIDTH;
/// Room the macOS traffic lights take at the left end of the bar.
pub const TRAFFIC_LIGHT_INSET: f32 = 78.0;
/// Height of the bar.
pub const BAR_HEIGHT: f32 = 30.0;
/// Resize zone thickness along an edge, and the side of a corner zone.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;
const CLOSE_RED: Color32 = Color32::from_rgb(232, 17, 35);

/// Who draws the window frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleBarStyle {
    #[default]
    System,
    Custom,
    Integrated,
}

impl TitleBarStyle {
    /// Whether the window has no OS decorations.
    pub fn is_custom(self) -> bool {
        self == Self::Custom
    }

    /// Room to keep free at the left end of the bar.
    pub fn leading_inset(self) -> f32 {
        if self == Self::Integrated { TRAFFIC_LIGHT_INSET } else { 0.0 }
    }

    /// Room to keep free at the right end of the bar.
    pub fn trailing_inset(self) -> f32 {
        if self == Self::Custom { WIDTH } else { 0.0 }
    }

    /// Whether the bar can move the window (anything but the OS-drawn frame).
    pub fn drags_window(self) -> bool {
        self != Self::System
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

const CAPTIONS: [Caption; 3] = [Caption::Minimize, Caption::Maximize, Caption::Close];

pub fn maximized(ctx: &egui::Context) -> bool {
    let id = Id::NULL.with("app_window_maximized_cached");
    let cur = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ctx.input(|i| i.viewport().maximized.unwrap_or(cur))
}

pub fn toggle_maximize(ctx: &egui::Context) {
    let id = Id::NULL.with("app_window_maximized_cached");
    let current_max = maximized(ctx);
    let next = !current_max;
    ctx.data_mut(|d| d.insert_temp(id, next));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(next));
}

/// The bar's background: a press-and-drag on bare space moves the window, a double-click toggles
/// maximize. Call it before laying out the bar so its widgets, registered later, take the clicks.
pub fn drag_area(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    let resp = ui.interact(rect, ui.make_persistent_id("titlebar-drag"), Sense::CLICK | Sense::DRAG);
    // A drag that starts on a click-only widget (a menu title) is handed to the background below
    // it, so remember whether the press itself landed on bare background.
    if ctx.input(|i| i.pointer.any_pressed()) {
        let bare = ctx.viewport(|v| v.hits.click.is_some_and(|w| w.id == resp.id));
        ctx.data_mut(|d| d.insert_temp(resp.id, bare));
    }
    if resp.double_clicked() {
        toggle_maximize(ctx);
    } else if resp.drag_started_by(PointerButton::Primary) && ctx.data(|d| d.get_temp(resp.id).unwrap_or(false)) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        handed_to_os(ctx);
    }
}

/// The window manager runs the move or resize from here and keeps the button release, so end the
/// drag in egui now; otherwise the next press would start no drag.
fn handed_to_os(ctx: &egui::Context) {
    ctx.stop_dragging();
    ctx.input_mut(|i| i.pointer = Default::default());
}

/// Minimize, Maximize/Restore and Close at the right end of `bar`, flush with the window's edge so
/// a maximized window's corner hits Close. Close asks the window to close like the OS button would.
pub fn caption_buttons(ui: &mut Ui, tokens: &Tokens, bar: Rect) {
    let max = maximized(ui.ctx());
    let mut clicked = None;
    for (i, caption) in CAPTIONS.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(bar.right() - WIDTH + i as f32 * BUTTON_WIDTH, bar.top()), vec2(BUTTON_WIDTH, bar.height()));
        let resp = ui.interact(r, ui.make_persistent_id(("titlebar-caption", i)), Sense::click());
        let close = caption == Caption::Close;
        let fill = match (resp.is_pointer_button_down_on(), resp.hovered()) {
            (true, _) if close => CLOSE_RED.gamma_multiply(0.8),
            (_, true) if close => CLOSE_RED,
            (true, _) => tokens.bg_surface_active,
            (_, true) => tokens.bg_surface_hover,
            _ => Color32::TRANSPARENT,
        };
        ui.painter().rect_filled(r, 0.0, fill);
        let ink = if close && resp.hovered() { Color32::WHITE } else { tokens.icon };
        paint_glyph(ui, caption, max, r.center(), Stroke::new(1.0, ink));
        let tip = match caption {
            Caption::Minimize => "Minimize",
            Caption::Maximize if max => "Restore",
            Caption::Maximize => "Maximize",
            Caption::Close => "Close",
        };
        if resp.on_hover_text(tip).clicked() {
            clicked = Some(caption);
        }
    }
    match clicked {
        Some(Caption::Minimize) => ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true)),
        Some(Caption::Maximize) => toggle_maximize(ui.ctx()),
        Some(Caption::Close) => ui.ctx().send_viewport_cmd(ViewportCommand::Close),
        None => {}
    }
}

/// The 10 pt line glyphs of the caption buttons.
fn paint_glyph(ui: &Ui, caption: Caption, maximized: bool, at: Pos2, stroke: Stroke) {
    let p = ui.painter();
    let g = Rect::from_center_size(at, vec2(10.0, 10.0));
    match caption {
        Caption::Minimize => {
            p.line_segment([g.left_center(), g.right_center()], stroke);
        }
        Caption::Maximize if maximized => {
            let front = Rect::from_min_max(g.min + vec2(0.0, 2.0), g.max - vec2(2.0, 0.0));
            p.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
            p.line(vec![g.min + vec2(2.0, 2.0), g.min + vec2(2.0, 0.0), g.right_top(), g.max - vec2(0.0, 2.0), g.max - vec2(2.0, 2.0)], stroke);
        }
        Caption::Maximize => {
            p.rect_stroke(g, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Caption::Close => {
            p.line_segment([g.left_top(), g.right_bottom()], stroke);
            p.line_segment([g.right_top(), g.left_bottom()], stroke);
        }
    }
}

/// The edge and corner zones of a window with content rect `w`; corners come last so they win
/// where they overlap the edges.
fn zones(w: Rect) -> [(Rect, ResizeDirection, CursorIcon); 8] {
    use ResizeDirection::*;
    let (e, c) = (EDGE, CORNER);
    [
        (Rect::from_min_max(w.left_top(), pos2(w.right(), w.top() + e)), North, CursorIcon::ResizeVertical),
        (Rect::from_min_max(pos2(w.left(), w.bottom() - e), w.right_bottom()), South, CursorIcon::ResizeVertical),
        (Rect::from_min_max(w.left_top(), pos2(w.left() + e, w.bottom())), West, CursorIcon::ResizeHorizontal),
        (Rect::from_min_max(pos2(w.right() - e, w.top()), w.right_bottom()), East, CursorIcon::ResizeHorizontal),
        (Rect::from_min_size(w.left_top(), vec2(c, c)), NorthWest, CursorIcon::ResizeNwSe),
        (Rect::from_min_size(w.right_top() - vec2(c, 0.0), vec2(c, c)), NorthEast, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.left_bottom() - vec2(0.0, c), vec2(c, c)), SouthWest, CursorIcon::ResizeNeSw),
        (Rect::from_min_size(w.right_bottom() - vec2(c, c), vec2(c, c)), SouthEast, CursorIcon::ResizeNwSe),
    ]
}

/// Invisible resize zones along the window's edges, above everything else; a press in one starts
/// an OS resize in that direction. None while the window is maximized or fullscreen.
pub fn resize_zones(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if maximized(&ctx) || ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let w = ctx.content_rect();
    let layer = LayerId::new(Order::Foreground, ui.make_persistent_id("titlebar-resize"));
    let zone_ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(w));
    for (i, (r, dir, cursor)) in zones(w).into_iter().enumerate() {
        let resp = zone_ui.interact(r, ui.make_persistent_id(("titlebar-resize", i)), Sense::DRAG).on_hover_cursor(cursor);
        if resp.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
            handed_to_os(&ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_follow_the_style() {
        assert_eq!(TitleBarStyle::System.leading_inset(), 0.0);
        assert_eq!(TitleBarStyle::System.trailing_inset(), 0.0);
        assert_eq!(TitleBarStyle::Integrated.leading_inset(), TRAFFIC_LIGHT_INSET);
        assert_eq!(TitleBarStyle::Integrated.trailing_inset(), 0.0);
        assert_eq!(TitleBarStyle::Custom.leading_inset(), 0.0);
        assert_eq!(TitleBarStyle::Custom.trailing_inset(), WIDTH);
        assert!(TitleBarStyle::Custom.is_custom() && !TitleBarStyle::Integrated.is_custom());
        assert!(!TitleBarStyle::System.drags_window());
    }

    #[test]
    fn test_toggle_maximize_updates_cached_state() {
        let ctx = egui::Context::default();
        assert!(!maximized(&ctx));
        toggle_maximize(&ctx);
        assert!(maximized(&ctx));
        toggle_maximize(&ctx);
        assert!(!maximized(&ctx));
    }

    #[test]
    fn corners_win_over_edges_and_all_zones_stay_inside() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let zones = zones(window);
        assert!(zones.iter().all(|(r, _, _)| window.contains_rect(*r)));
        assert!(matches!(zones.last(), Some((_, ResizeDirection::SouthEast, _))));
    }

    /// Runs one frame and returns the viewport commands it sent.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, tokens: &Tokens, bar: Rect) -> Vec<ViewportCommand> {
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))), events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| caption_buttons(ui, tokens, bar));
        output.textures_delta.clear();
        output.viewport_output.values().flat_map(|v| v.commands.iter().cloned()).collect()
    }

    #[test]
    fn close_button_requests_a_window_close() {
        let ctx = egui::Context::default();
        let tokens = crate::theme::Tokens::dark();
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, BAR_HEIGHT));
        let close_at = pos2(900.0 - BUTTON_WIDTH / 2.0, BAR_HEIGHT / 2.0);
        let click = |pressed| egui::Event::PointerButton { pos: close_at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, vec![egui::Event::PointerMoved(close_at)], &tokens, bar);
        frame(&ctx, vec![click(true)], &tokens, bar);
        let commands = frame(&ctx, vec![click(false)], &tokens, bar);
        let closed = commands.iter().any(|c| matches!(c, ViewportCommand::Close));
        assert!(closed, "clicking Close must send ViewportCommand::Close");
    }
}
