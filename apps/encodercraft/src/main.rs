//! encoderCraft desktop application executable.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
// Release builds on Windows are GUI programs: no console window behind the app.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_ipc::IpcServer;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::EncoderCraftApp;

struct DesktopApp {
    inner: EncoderCraftApp,
    _ipc_tcp: Option<IpcServer>,
    #[cfg(unix)]
    _ipc_unix: Option<IpcServer>,
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.inner.render(ui);
    }
}

fn main() -> Result<(), eframe::Error> {
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("encodercraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    env_logger::init();

    let app_dir = get_app_data_dir();
    let _ = std::fs::create_dir_all(&app_dir);
    let journal_file = app_dir.join("queue-state.json");
    let prefs_file = app_dir.join("preferences.json");

    let queue_mgr = Arc::new(QueueManager::new_persisted(journal_file).unwrap_or_else(|_| QueueManager::new_in_memory()));
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));
    let prefs = encodercraft_engine::load_preferences(&prefs_file).unwrap_or_default();

    let handler = Arc::new(EngineIpcHandler::new(queue_mgr.clone(), preset_registry.clone()));

    // Bind TCP daemon on standard port 8998 (or 8999 fallback)
    let ipc_tcp = IpcServer::bind_tcp("127.0.0.1:8998", handler.clone()).or_else(|_| IpcServer::bind_tcp("127.0.0.1:8999", handler.clone())).ok();

    // Bind Unix domain socket on Unix platforms
    #[cfg(unix)]
    let ipc_unix = {
        let sock_path = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let sock_file = PathBuf::from(sock_path).join("encodercraft.sock");
        let _ = std::fs::remove_file(&sock_file);
        IpcServer::bind_unix(&sock_file, handler).ok()
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("encoderCraft")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "encoderCraft",
        options,
        Box::new(|_cc| {
            let mut app = EncoderCraftApp::new(queue_mgr, preset_registry);
            if let Some(path) = encodercraft_ui_egui::settings::default_config_dir() {
                app = app.with_config_dir(path);
            }
            if prefs.theme == "High Contrast" {
                app.brightness = encodercraft_ui_egui::Brightness::HighContrast;
                app.tokens = encodercraft_ui_egui::Tokens::for_brightness(app.brightness);
            }
            app.prefs_state.prefs = prefs;

            Ok(Box::new(DesktopApp {
                inner: app,
                _ipc_tcp: ipc_tcp,
                #[cfg(unix)]
                _ipc_unix: ipc_unix,
            }))
        }),
    )
}

fn get_app_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("EncoderCraft");
    }
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("encodercraft")
    } else if let Ok(home) = std::env::var("HOME") {
        #[cfg(target_os = "macos")]
        {
            PathBuf::from(home).join("Library").join("Application Support").join("encoderCraft")
        }
        #[cfg(not(target_os = "macos"))]
        {
            PathBuf::from(home).join(".config").join("encodercraft")
        }
    } else {
        std::env::temp_dir().join("encodercraft")
    }
}
