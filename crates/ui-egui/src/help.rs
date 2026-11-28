//! The Help menu's community links, Reveal Log Files and the System Compatibility Report window, as the other Craft
//! apps have them. The engine answers what to open (`app.help_link`, `app.log_files`, `app.system_report`), so the CLI
//! and MCP reach the same; this module opens it. encoderCraft has no page on getartcraft.com yet, so that row waits
//! for one.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, sync_channel};

use serde_json::Value;

use crate::commands::AppCommand;
use crate::i18n::tl;

/// The `link` an `app.help_link` request names for a Help link row; `None` for every other command.
pub fn link_name(command: AppCommand) -> Option<&'static str> {
    match command {
        AppCommand::JoinDiscord => Some("discord"),
        AppCommand::ArtCraftWebsite => Some("website"),
        AppCommand::OpenGitHub => Some("github"),
        AppCommand::ReportIssue => Some("issues"),
        _ => None,
    }
}

/// The desktop app's log folder: `logs` in its settings folder, where it writes `encodercraft.log` and crash logs.
pub fn log_folder(config_dir: &Path) -> PathBuf {
    config_dir.join("logs")
}

/// What Reveal Log Files shows, from the engine's `app.log_files` answer: the current log file among the others,
/// else the log folder, else why there is nothing to show.
pub fn log_to_reveal(files: &Value) -> Result<PathBuf, String> {
    if let Some(current) = files["current"].as_str() {
        return Ok(PathBuf::from(current));
    }
    let folder = files["folder"].as_str().ok_or_else(|| tl!("encoderCraft has no settings folder here, so it keeps no log files.").to_owned())?;
    let folder = PathBuf::from(folder);
    if folder.is_dir() {
        Ok(folder)
    } else {
        Err(crate::i18n::fill("There is no log folder at {path} yet.", &[("path", &folder.display().to_string())]))
    }
}

/// Reveals `path` in the system's file manager on a thread of its own, since that can wait up to two seconds for
/// the file manager to answer; the receiver gets the outcome.
pub fn reveal_in_background(path: PathBuf) -> Receiver<Result<(), String>> {
    let (outcome, receiver) = sync_channel(1);
    let started = std::thread::Builder::new().name("encodercraft-reveal".into()).spawn({
        let outcome = outcome.clone();
        move || {
            let _ = outcome.send(craft_platform::open::reveal(&path));
        }
    });
    if let Err(error) = started {
        let _ = outcome.send(Err(error.to_string()));
    }
    receiver
}

/// The report's rows as the window shows them: a label in the interface language and the value.
fn rows(report: &Value) -> Vec<(&'static str, String)> {
    let text = |value: &Value| value.as_str().map_or_else(|| value.to_string(), str::to_owned);
    let hardware = &report["hardware"];
    let codecs = hardware["codecs"]
        .as_array()
        .map(|codecs| codecs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
        .filter(|codecs| !codecs.is_empty())
        .unwrap_or_else(|| tl!("None").to_owned());
    vec![
        (tl!("Version"), text(&report["app"]["version"])),
        (tl!("Operating system"), format!("{} ({})", text(&report["os"]["name"]), text(&report["os"]["arch"]))),
        (tl!("CPU threads"), text(&report["cpuThreads"])),
        (tl!("Hardware encoder"), format!("{} — {}", text(&hardware["backend"]), text(&hardware["device"]))),
        (tl!("Hardware codecs"), codecs),
        (tl!("Largest frame"), format!("{} × {}", text(&hardware["maxWidth"]), text(&hardware["maxHeight"]))),
        (tl!("Log folder"), text(&report["logFolder"])),
    ]
}

/// The System Compatibility Report window while `report` holds one. Copy Report puts the whole report on the
/// clipboard as JSON, for a bug report.
pub fn render_system_report(ctx: &egui::Context, report: &mut Option<Value>) {
    let Some(value) = report.as_ref() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    egui::Window::new(tl!("System Compatibility Report")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("system_report").num_columns(2).striped(true).show(ui, |ui| {
            for (label, text) in rows(value) {
                ui.label(label);
                ui.label(text);
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(tl!("Copy Report")).clicked() {
                ui.ctx().copy_text(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()));
            }
            if ui.button(tl!("Close")).clicked() {
                close = true;
            }
        });
    });
    if !open || close {
        *report = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_link_row_names_a_link_the_engine_opens() {
        let registry = encodercraft_engine::cmd::CommandRegistry::new();
        let queue = std::sync::Arc::new(encodercraft_engine::QueueManager::new_in_memory());
        for command in [AppCommand::JoinDiscord, AppCommand::ArtCraftWebsite, AppCommand::OpenGitHub, AppCommand::ReportIssue] {
            let link = link_name(command).unwrap_or_default();
            let answer = registry.dispatch(&queue, "app.help_link", serde_json::json!({ "link": link }));
            assert!(answer.is_ok_and(|value| value["url"].as_str().is_some_and(|url| url.starts_with("https://"))), "{command:?}");
        }
        let repository = encodercraft_engine::system::REPOSITORY.trim_start_matches("https://github.com/");
        assert_eq!(repository, crate::app::ENCODERCRAFT_APP.repo, "the updater and the GitHub rows name one repository");
    }

    #[test]
    fn reveal_log_files_shows_the_current_log_else_the_folder_else_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let folder = log_folder(dir.path());
        let files = |folder: Option<PathBuf>| encodercraft_engine::system::log_files(folder);
        assert!(log_to_reveal(&files(None)).is_err(), "no settings folder");
        assert!(log_to_reveal(&files(Some(folder.clone()))).is_err(), "no log folder yet");
        std::fs::create_dir_all(&folder).unwrap();
        assert_eq!(log_to_reveal(&files(Some(folder.clone()))).unwrap(), folder);
        std::fs::write(folder.join("encodercraft.log"), "x").unwrap();
        assert_eq!(log_to_reveal(&files(Some(folder.clone()))).unwrap(), folder.join("encodercraft.log"));
    }

    #[test]
    fn the_report_window_lists_every_part_of_the_engine_report() {
        let report = encodercraft_engine::system::system_report();
        let rows = rows(&report);
        assert!(rows.iter().all(|(_, text)| !text.is_empty() && text != "null"), "{rows:?}");
        assert!(rows.iter().any(|(_, text)| text.contains(encodercraft_platform::detect_hardware().backend.display_name())), "{rows:?}");
    }
}
