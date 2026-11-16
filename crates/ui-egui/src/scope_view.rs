//! Video scopes for the preview monitor. The maths and painters come from `filmcraft-scopes`
//! (shared across the ArtCraft apps); this module only selects the scope and tiles the result.

use egui::ColorImage;
use filmcraft_scopes::{Computed, ParadeType, Params, ScopeKind, Signal, WaveformType, compute, paint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScopeMode {
    #[default]
    Off,
    Waveform,
    Parade,
    Histogram,
    Vectorscope,
}

impl ScopeMode {
    pub const ALL: [ScopeMode; 5] = [ScopeMode::Off, ScopeMode::Waveform, ScopeMode::Parade, ScopeMode::Histogram, ScopeMode::Vectorscope];

    pub fn label(self) -> &'static str {
        match self {
            ScopeMode::Off => "Scopes off",
            ScopeMode::Waveform => "Waveform",
            ScopeMode::Parade => "RGB Parade",
            ScopeMode::Histogram => "Histogram",
            ScopeMode::Vectorscope => "Vectorscope",
        }
    }

    fn kind(self) -> Option<ScopeKind> {
        match self {
            ScopeMode::Off => None,
            ScopeMode::Waveform => Some(ScopeKind::Waveform),
            ScopeMode::Parade => Some(ScopeKind::Parade),
            ScopeMode::Histogram => Some(ScopeKind::Histogram),
            ScopeMode::Vectorscope => Some(ScopeKind::VectorscopeYuv),
        }
    }
}

const RGB: [[f32; 3]; 4] = [[1.0, 0.25, 0.25], [0.25, 1.0, 0.35], [0.35, 0.5, 1.0], [0.9, 0.9, 0.9]];

fn trace_color(name: &str, index: usize) -> [f32; 3] {
    match name {
        "R" => RGB[0],
        "G" => RGB[1],
        "B" => RGB[2],
        "Y" | "C" => RGB[3],
        "Cb" => RGB[2],
        "Cr" => RGB[0],
        _ => RGB[index % RGB.len()],
    }
}

/// Renders the chosen scope for a straight-alpha RGBA8 frame. Returns `None` when scopes are off
/// or the frame is empty.
pub fn render(mode: ScopeMode, width: usize, height: usize, rgba: &[u8]) -> Option<ColorImage> {
    let kind = mode.kind()?;
    let signal = Signal::from_rgba8(width, height, rgba);
    if signal.is_empty() {
        return None;
    }
    let params = Params::default();
    match compute(kind, &signal, &params, WaveformType::Rgb, ParadeType::Rgb) {
        Computed::Waveform(w) => {
            let layers: Vec<_> = w.traces.iter().enumerate().map(|(i, g)| (g, trace_color(g.name, i))).collect();
            let (cols, rows, px) = paint::grids(&layers, paint::waveform_reference(w.per_column), 1.0);
            image(cols, rows, &px)
        }
        Computed::Parade(w) => {
            let painted: Vec<_> = w
                .traces
                .iter()
                .enumerate()
                .map(|(i, g)| paint::grids(&[(g, trace_color(g.name, i))], paint::waveform_reference(w.per_column), 1.0))
                .collect();
            let rows = painted.iter().map(|(_, r, _)| *r).max()?;
            let cols: usize = painted.iter().map(|(c, _, _)| *c).sum();
            let mut px = vec![0u8; cols * rows * 4];
            let mut x0 = 0;
            for (c, r, data) in &painted {
                for y in 0..*r {
                    let dst = (y * cols + x0) * 4;
                    let src = y * c * 4;
                    if let (Some(d), Some(s)) = (px.get_mut(dst..dst + c * 4), data.get(src..src + c * 4)) {
                        d.copy_from_slice(s);
                    }
                }
                x0 += c;
            }
            image(cols, rows, &px)
        }
        Computed::Histogram(h) => {
            const HEIGHT: usize = 128;
            let bins = h.r.len().min(h.g.len()).min(h.b.len());
            let peak = h.r.iter().chain(&h.g).chain(&h.b).copied().max().unwrap_or(0).max(1) as f32;
            let mut px = vec![0u8; bins * HEIGHT * 4];
            for x in 0..bins {
                for (channel, color) in [(&h.r, RGB[0]), (&h.g, RGB[1]), (&h.b, RGB[2])] {
                    let level = ((channel[x] as f32 / peak).sqrt() * HEIGHT as f32) as usize;
                    for y in HEIGHT.saturating_sub(level)..HEIGHT {
                        let i = (y * bins + x) * 4;
                        if let Some(p) = px.get_mut(i..i + 4) {
                            for c in 0..3 {
                                p[c] = p[c].saturating_add((color[c] * 150.0) as u8);
                            }
                            p[3] = 255;
                        }
                    }
                }
            }
            image(bins, HEIGHT, &px)
        }
        Computed::Vectorscope(v) => {
            let (n, _, px) = paint::vectorscope(&v, [1.0, 1.0, 1.0], 1.0, true);
            image(n, n, &px)
        }
    }
}

fn image(width: usize, height: usize, premultiplied: &[u8]) -> Option<ColorImage> {
    if width == 0 || height == 0 || premultiplied.len() < width * height * 4 {
        return None;
    }
    Some(ColorImage::from_rgba_premultiplied([width, height], premultiplied))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(r: u8, g: u8, b: u8) -> Vec<u8> {
        (0..64 * 64).flat_map(|_| [r, g, b, 255]).collect()
    }

    fn lit(img: &ColorImage) -> usize {
        img.pixels.iter().filter(|p| p.a() > 0).count()
    }

    #[test]
    fn off_and_empty_frames_render_nothing() {
        assert!(render(ScopeMode::Off, 64, 64, &flat(1, 2, 3)).is_none());
        assert!(render(ScopeMode::Waveform, 0, 0, &[]).is_none());
        assert!(render(ScopeMode::Waveform, 64, 64, &[0; 10]).is_none());
    }

    #[test]
    fn every_scope_draws_something_for_a_real_frame() {
        let frame: Vec<u8> = (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 128, 255]).collect();
        for mode in &ScopeMode::ALL[1..] {
            let img = render(*mode, 64, 64, &frame).unwrap_or_else(|| panic!("{mode:?} rendered nothing"));
            assert!(lit(&img) > 0, "{mode:?} is blank");
        }
    }

    #[test]
    fn histogram_of_black_peaks_at_the_left_edge() {
        let img = render(ScopeMode::Histogram, 64, 64, &flat(0, 0, 0)).unwrap();
        let w = img.size[0];
        let column = |x: usize| (0..img.size[1]).filter(|y| img.pixels[y * w + x].a() > 0).count();
        assert!(column(0) > 0);
        assert_eq!(column(w - 1), 0);
    }

    #[test]
    fn parade_is_three_panels_wide() {
        let wave = render(ScopeMode::Waveform, 64, 64, &flat(200, 100, 50)).unwrap();
        let parade = render(ScopeMode::Parade, 64, 64, &flat(200, 100, 50)).unwrap();
        assert_eq!(parade.size[0], wave.size[0] * 3);
    }
}
