//! Geometry for the preview canvas: fit/zoom/pan, the A/B divider, safe margins and aspect guides.

use egui::{Pos2, Rect, Vec2};

/// Title-safe and action-safe areas as a fraction of the frame.
pub const TITLE_SAFE: f32 = 0.8;
pub const ACTION_SAFE: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ZoomMode {
    /// The whole frame fits in the canvas.
    #[default]
    Fit,
    /// One image pixel per screen point.
    Actual,
    /// A multiple of the fitted size.
    Scale(f32),
}

/// Largest rect of `aspect` (width / height) centred in `container`.
pub fn fit_rect(container: Rect, aspect: f32) -> Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 };
    let (cw, ch) = (container.width().max(1.0), container.height().max(1.0));
    let (w, h) = if cw / ch > aspect { (ch * aspect, ch) } else { (cw, cw / aspect) };
    Rect::from_center_size(container.center(), Vec2::new(w, h))
}

/// The rect the frame occupies for a zoom mode and pan offset (in points, applied after zoom).
pub fn view_rect(container: Rect, image_px: (u32, u32), mode: ZoomMode, pan: Vec2) -> Rect {
    let aspect = image_px.0.max(1) as f32 / image_px.1.max(1) as f32;
    let fitted = fit_rect(container, aspect);
    let size = match mode {
        ZoomMode::Fit => fitted.size(),
        ZoomMode::Actual => Vec2::new(image_px.0.max(1) as f32, image_px.1.max(1) as f32),
        ZoomMode::Scale(s) => fitted.size() * s.clamp(0.1, 32.0),
    };
    let max_pan = (size - container.size()).max(Vec2::ZERO) * 0.5 + container.size() * 0.25;
    let pan = Vec2::new(pan.x.clamp(-max_pan.x, max_pan.x), pan.y.clamp(-max_pan.y, max_pan.y));
    Rect::from_center_size(container.center() + pan, size)
}

/// Divider position as a 0..=1 fraction of the frame width for a pointer x.
pub fn split_fraction(pointer_x: f32, frame: Rect) -> f32 {
    ((pointer_x - frame.min.x) / frame.width().max(1.0)).clamp(0.0, 1.0)
}

/// The two halves of `frame` either side of `fraction`: (source side, output side).
pub fn split_rects(frame: Rect, fraction: f32) -> (Rect, Rect) {
    let x = frame.min.x + frame.width() * fraction.clamp(0.0, 1.0);
    (Rect::from_min_max(frame.min, Pos2::new(x, frame.max.y)), Rect::from_min_max(Pos2::new(x, frame.min.y), frame.max))
}

/// Centred safe area covering `fraction` of the frame in each direction.
pub fn safe_rect(frame: Rect, fraction: f32) -> Rect {
    Rect::from_center_size(frame.center(), frame.size() * fraction.clamp(0.0, 1.0))
}

/// Centred crop of `frame` showing where a different aspect ratio would cut.
pub fn aspect_guide(frame: Rect, aspect: f32) -> Rect {
    if !aspect.is_finite() || aspect <= 0.0 {
        return frame;
    }
    let frame_aspect = frame.width() / frame.height().max(1.0);
    if aspect >= frame_aspect {
        Rect::from_center_size(frame.center(), Vec2::new(frame.width(), frame.width() / aspect))
    } else {
        Rect::from_center_size(frame.center(), Vec2::new(frame.height() * aspect, frame.height()))
    }
}

/// UV rect of the part of the texture visible inside `clip` when it is drawn into `frame`.
pub fn uv_for_clip(frame: Rect, clip: Rect) -> Rect {
    let u = |x: f32| ((x - frame.min.x) / frame.width().max(1.0)).clamp(0.0, 1.0);
    let v = |y: f32| ((y - frame.min.y) / frame.height().max(1.0)).clamp(0.0, 1.0);
    Rect::from_min_max(Pos2::new(u(clip.min.x), v(clip.min.y)), Pos2::new(u(clip.max.x), v(clip.max.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    }

    #[test]
    fn fit_letterboxes_and_pillarboxes() {
        let wide = fit_rect(r(0.0, 0.0, 400.0, 400.0), 2.0);
        assert_eq!((wide.width(), wide.height()), (400.0, 200.0));
        let tall = fit_rect(r(0.0, 0.0, 400.0, 100.0), 1.0);
        assert_eq!((tall.width(), tall.height()), (100.0, 100.0));
        assert_eq!(tall.center(), Pos2::new(200.0, 50.0));
        assert_eq!(fit_rect(r(0.0, 0.0, 160.0, 90.0), f32::NAN).width(), 160.0);
    }

    #[test]
    fn actual_size_is_one_point_per_pixel_and_pan_is_bounded() {
        let c = r(0.0, 0.0, 300.0, 200.0);
        let v = view_rect(c, (1920, 1080), ZoomMode::Actual, Vec2::ZERO);
        assert_eq!((v.width(), v.height()), (1920.0, 1080.0));
        let far = view_rect(c, (1920, 1080), ZoomMode::Actual, Vec2::new(1.0e9, 0.0));
        assert!(far.center().x < 1.0e5);
        let doubled = view_rect(c, (300, 200), ZoomMode::Scale(2.0), Vec2::ZERO);
        assert_eq!(doubled.width(), 600.0);
    }

    #[test]
    fn divider_math_round_trips() {
        let f = r(100.0, 0.0, 300.0, 100.0);
        assert_eq!(split_fraction(200.0, f), 0.5);
        assert_eq!(split_fraction(-50.0, f), 0.0);
        let (a, b) = split_rects(f, 0.25);
        assert_eq!((a.max.x, b.min.x), (150.0, 150.0));
        assert_eq!(a.min, f.min);
        assert_eq!(b.max, f.max);
    }

    #[test]
    fn safe_margins_and_guides() {
        let f = r(0.0, 0.0, 1000.0, 500.0);
        let title = safe_rect(f, TITLE_SAFE);
        assert_eq!((title.width(), title.height()), (800.0, 400.0));
        assert_eq!(title.center(), f.center());
        let cinemascope = aspect_guide(f, 2.39);
        assert!((cinemascope.height() - 1000.0 / 2.39).abs() < 0.01);
        let square = aspect_guide(f, 1.0);
        assert_eq!((square.width(), square.height()), (500.0, 500.0));
    }

    #[test]
    fn clipped_uv_matches_the_visible_half() {
        let f = r(0.0, 0.0, 200.0, 100.0);
        let (left, right) = split_rects(f, 0.5);
        let uv = uv_for_clip(f, right);
        assert_eq!((uv.min.x, uv.max.x), (0.5, 1.0));
        assert_eq!((uv_for_clip(f, left).max.x, uv.min.y, uv.max.y), (0.5, 0.0, 1.0));
    }
}
