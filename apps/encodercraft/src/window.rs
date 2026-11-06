//! The main window: its options, and its size, position and maximized state across launches.
//!
//! The first launch opens at [`DEFAULT_SIZE`] (maximized when the monitor leaves no room for it).
//! Later launches restore the geometry saved in `window.json` next to the preferences. The layout
//! mirrors vectorCraft's window handling (MIT OR Apache-2.0).

use std::path::{Path, PathBuf};

use encodercraft_ui_egui::titlebar::TitleBarStyle;
use serde::{Deserialize, Serialize};

/// The first window's size, in points.
pub const DEFAULT_SIZE: [f32; 2] = [1440.0, 900.0];
/// The smallest window, in points.
pub const MIN_SIZE: [f32; 2] = [800.0, 500.0];
/// Room a first window leaves for a taskbar, Dock or menu bar; a monitor with less opens it maximized.
const SCREEN_ROOM: [f32; 2] = [64.0, 128.0];
/// Window system application id (Linux/Wayland desktop entry match).
pub const APP_ID: &str = "ai.storyteller.encodercraft";
/// Positions further out than this are treated as a disconnected monitor.
const POSITION_LIMIT: f32 = 20_000.0;

const ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/encodercraft-1024.png");

/// Where the window was and how large it was, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub pos: Option<[f32; 2]>,
    pub size: [f32; 2],
    pub maximized: bool,
}

impl WindowGeometry {
    /// Clamps values from a hand-edited or stale file to something that can be shown.
    pub fn sanitized(mut self) -> Self {
        for (value, min) in self.size.iter_mut().zip(MIN_SIZE) {
            *value = if value.is_finite() { value.clamp(min, 16_384.0) } else { min };
        }
        if let Some(pos) = self.pos {
            let valid = pos.iter().all(|v| v.is_finite() && v.abs() <= POSITION_LIMIT);
            self.pos = valid.then_some(pos);
        }
        self
    }
}

fn geometry_path(config_dir: &Path) -> PathBuf {
    config_dir.join("window.json")
}

/// The saved geometry, or `None` for a first launch or an unreadable file.
pub fn load(config_dir: &Path) -> Option<WindowGeometry> {
    let text = std::fs::read_to_string(geometry_path(config_dir)).ok()?;
    serde_json::from_str::<WindowGeometry>(&text).ok().map(WindowGeometry::sanitized)
}

/// Saves the geometry atomically; a failure is logged and never interrupts the app.
pub fn save(config_dir: &Path, geometry: &WindowGeometry) {
    let path = geometry_path(config_dir);
    let result = serde_json::to_string_pretty(geometry).map_err(std::io::Error::other).and_then(|text| {
        std::fs::create_dir_all(config_dir)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    });
    if let Err(error) = result {
        log::warn!("could not save the window geometry: {error}");
    }
}

/// Whether a first window of the default size fits a monitor of `monitor` points.
pub fn default_fits(monitor: [f32; 2]) -> bool {
    (0..2).all(|i| DEFAULT_SIZE[i] <= monitor[i] - SCREEN_ROOM[i])
}

/// Who draws the window frame: macOS keeps the traffic lights over an integrated strip, Windows
/// draws its own caption buttons, and Linux follows the shared `craft-window` probe (an app-drawn
/// frame only where the compositor leaves it to the app). `system_title_bar` or the
/// `ENCODERCRAFT_TITLEBAR` override force the OS frame / the app's.
pub fn title_bar_style(system_title_bar: bool) -> TitleBarStyle {
    let choice = craft_window::Choice::probe(craft_window::Prefix::new("ENCODERCRAFT"));
    decide_title_bar(cfg!(target_os = "macos"), cfg!(windows), matches!(choice.frame(), craft_window::Frame::App(_)), system_title_bar)
}

fn decide_title_bar(macos: bool, windows: bool, app_frame: bool, system_title_bar: bool) -> TitleBarStyle {
    if system_title_bar {
        TitleBarStyle::System
    } else if macos {
        TitleBarStyle::Integrated
    } else if windows || app_frame {
        TitleBarStyle::Custom
    } else {
        TitleBarStyle::System
    }
}

/// Window options for [`eframe::run_native`].
pub fn native_options(saved: Option<WindowGeometry>, title_bar: TitleBarStyle) -> eframe::NativeOptions {
    let geometry = saved.unwrap_or(WindowGeometry { pos: None, size: DEFAULT_SIZE, maximized: false });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("encoderCraft")
        .with_app_id(APP_ID)
        .with_inner_size(geometry.size)
        .with_min_inner_size(MIN_SIZE)
        .with_maximized(geometry.maximized)
        .with_drag_and_drop(true)
        .with_decorations(!title_bar.is_custom());
    if title_bar == TitleBarStyle::Integrated {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(pos) = geometry.pos {
        viewport = viewport.with_position(pos);
    }
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(error) => log::warn!("could not load the window icon: {error}"),
    }
    eframe::NativeOptions { viewport, ..Default::default() }
}

/// Follows the window and saves it when it settles, so a crash never loses more than a moment.
#[derive(Default)]
pub struct Tracker {
    current: Option<WindowGeometry>,
    saved: Option<WindowGeometry>,
    fitted: bool,
}

impl Tracker {
    pub fn new(saved: Option<WindowGeometry>) -> Self {
        Self { current: saved, saved, fitted: saved.is_some() }
    }

    /// Call once per frame.
    pub fn update(&mut self, ctx: &egui::Context, config_dir: Option<&Path>) {
        let (inner, outer, maximized, fullscreen, monitor) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false), v.monitor_size)
        });
        if !self.fitted {
            self.fitted = true;
            if monitor.is_some_and(|m| !default_fits([m.x, m.y])) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        if fullscreen {
            return;
        }
        let previous = self.current;
        let mut geometry = previous.unwrap_or(WindowGeometry { pos: None, size: DEFAULT_SIZE, maximized: false });
        geometry.maximized = maximized;
        // A maximized window keeps the restored size so un-maximizing returns to it.
        if !maximized {
            if let Some(inner) = inner {
                geometry.size = [inner.width(), inner.height()];
            }
            if let Some(outer) = outer {
                geometry.pos = Some([outer.min.x, outer.min.y]);
            }
        }
        self.current = Some(geometry.sanitized());
        let settled = !ctx.input(|i| i.pointer.any_down());
        if let (Some(dir), true, Some(current)) = (config_dir, settled, self.current)
            && self.saved != Some(current)
        {
            save(dir, &current);
            self.saved = Some(current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_geometry_is_clamped() {
        let geometry = WindowGeometry { pos: Some([f32::NAN, 5.0]), size: [f32::INFINITY, 1.0], maximized: false }.sanitized();
        assert_eq!(geometry.pos, None);
        assert_eq!(geometry.size, [MIN_SIZE[0], MIN_SIZE[1]]);
        let far = WindowGeometry { pos: Some([1.0e9, 0.0]), size: [1000.0, 700.0], maximized: true }.sanitized();
        assert_eq!(far.pos, None);
        assert!(far.maximized);
    }

    #[test]
    fn geometry_round_trips_and_bad_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), None);
        let geometry = WindowGeometry { pos: Some([40.0, 60.0]), size: [1200.0, 800.0], maximized: false };
        save(dir.path(), &geometry);
        assert_eq!(load(dir.path()), Some(geometry));
        std::fs::write(dir.path().join("window.json"), "{ not json").unwrap();
        assert_eq!(load(dir.path()), None);
    }

    #[test]
    fn small_monitors_do_not_fit_the_default() {
        assert!(default_fits([2560.0, 1440.0]));
        assert!(!default_fits([1440.0, 900.0]));
        assert!(!default_fits([1366.0, 768.0]));
    }

    #[test]
    fn title_bar_decision_table() {
        // macOS: integrated strip; Windows: own caption; Linux: only when the compositor says so.
        assert_eq!(decide_title_bar(true, false, false, false), TitleBarStyle::Integrated);
        assert_eq!(decide_title_bar(false, true, false, false), TitleBarStyle::Custom);
        assert_eq!(decide_title_bar(false, false, true, false), TitleBarStyle::Custom);
        assert_eq!(decide_title_bar(false, false, false, false), TitleBarStyle::System);
        // The preference always forces the OS frame.
        assert_eq!(decide_title_bar(true, false, false, true), TitleBarStyle::System);
        assert_eq!(decide_title_bar(false, true, true, true), TitleBarStyle::System);
    }

    #[test]
    fn the_icon_decodes() {
        assert!(eframe::icon_data::from_png_bytes(ICON_PNG).is_ok());
    }
}
