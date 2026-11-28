//! The Help menu's engine side: the community links (`app.help_link`), where the desktop app logs (`app.log_files`)
//! and what encoderCraft found on this machine (`app.system_report`, the first lines of a bug report), so the CLI and
//! MCP reach them as the GUI does.

use std::path::PathBuf;

use serde_json::{Value, json};

/// The ArtCraft community on Discord.
pub const DISCORD: &str = "https://discord.gg/artcraft";
/// The ArtCraft website.
pub const WEBSITE: &str = "https://getartcraft.com";
/// encoderCraft's repository.
pub const REPOSITORY: &str = "https://github.com/el-j/encoderCraft";

/// The page a Help link opens: `discord`, `website`, `github` or `issues`.
pub fn help_link(name: &str) -> Option<String> {
    match name {
        "discord" => Some(DISCORD.into()),
        "website" => Some(WEBSITE.into()),
        "github" => Some(REPOSITORY.into()),
        "issues" => Some(format!("{REPOSITORY}/issues")),
        _ => None,
    }
}

/// The folder the desktop app writes `encodercraft.log`, its earlier runs' logs and crash logs to: `logs` in the
/// settings folder.
pub fn log_folder() -> Option<PathBuf> {
    crate::default_config_dir().map(|dir| dir.join("logs"))
}

/// The log folder and, when there is one, the current log file in it.
pub fn log_files(folder: Option<PathBuf>) -> Value {
    let current = folder.as_ref().map(|folder| folder.join("encodercraft.log")).filter(|file| file.is_file());
    json!({
        "folder": folder.map(|folder| folder.display().to_string()),
        "current": current.map(|file| file.display().to_string()),
    })
}

/// The report: the app and its version, the operating system, the CPU threads, the hardware encoder
/// encoderCraft would use and the codecs it offers, and the folder the desktop app writes its logs to.
pub fn system_report() -> Value {
    let hardware = encodercraft_platform::detect_hardware();
    let codecs: Vec<&str> =
        [("H.264", hardware.supports_h264), ("HEVC", hardware.supports_hevc), ("ProRes", hardware.supports_prores), ("AV1", hardware.supports_av1)]
            .into_iter()
            .filter_map(|(name, offered)| offered.then_some(name))
            .collect();
    json!({
        "app": { "name": "encoderCraft", "version": env!("CARGO_PKG_VERSION") },
        "os": { "name": std::env::consts::OS, "family": std::env::consts::FAMILY, "arch": std::env::consts::ARCH },
        "cpuThreads": std::thread::available_parallelism().map(std::num::NonZeroUsize::get).unwrap_or(1),
        "hardware": {
            "backend": hardware.backend.display_name(),
            "accelerated": hardware.backend.is_hardware(),
            "device": hardware.device_name,
            "codecs": codecs,
            "maxWidth": hardware.max_width,
            "maxHeight": hardware.max_height,
        },
        "logFolder": log_folder().map(|folder| folder.display().to_string()),
    })
}
