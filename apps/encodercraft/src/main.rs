//! encoderCraft desktop application executable.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
// Release builds on Windows are GUI programs: no console window behind the app.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod window;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_ipc::IpcServer;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::EncoderCraftApp;

struct DesktopApp {
    inner: EncoderCraftApp,
    window: window::Tracker,
    config_dir: PathBuf,
    _ipc_tcp: Option<IpcServer>,
    #[cfg(unix)]
    _ipc_unix: Option<IpcServer>,
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.window.update(ui.ctx(), Some(&self.config_dir));
        self.inner.render(ui);
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("encodercraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("encodercraft {}\n\nUSAGE: encodercraft [--version] [--help]\n\nFor headless use see encodercraft-cli.", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    env_logger::init();

    let app_dir = get_app_data_dir();
    let _ = std::fs::create_dir_all(&app_dir);
    let journal_file = app_dir.join("queue-state.json");
    let config_dir = encodercraft_engine::default_config_dir().unwrap_or_else(|| app_dir.clone());
    let prefs_file = config_dir.join("preferences.json");

    let queue_mgr = Arc::new(QueueManager::new_persisted(journal_file).unwrap_or_else(|_| QueueManager::new_in_memory()));
    queue_mgr.set_conversion_preferences_path(prefs_file.clone());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::new(Some(config_dir.join("presets")))));
    let prefs = encodercraft_engine::load_preferences(&prefs_file).unwrap_or_default();

    let handler = Arc::new(EngineIpcHandler::new(queue_mgr.clone(), preset_registry.clone()));

    // Bind TCP daemon on standard port 8998 (or 8999 fallback)
    let ipc_tcp = IpcServer::bind_tcp("127.0.0.1:8998", handler.clone()).or_else(|_| IpcServer::bind_tcp("127.0.0.1:8999", handler.clone())).ok();

    // Bind Unix domain socket on Unix platforms
    #[cfg(unix)]
    let ipc_unix = {
        let sock_file = encodercraft_dispatch::default_socket_path();
        let _ = std::fs::remove_file(&sock_file);
        IpcServer::bind_unix(&sock_file, handler).ok()
    };

    let title_bar = window::title_bar_style(prefs.system_title_bar);
    let options = window::native_options(window::load(&config_dir), title_bar);
    let saved_window = window::load(&config_dir);
    let window_config_dir = config_dir.clone();

    // A panic in platform or GPU code ends the window with an error instead of aborting the process.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "encoderCraft",
            options,
            Box::new(|_cc| {
                let mut app = EncoderCraftApp::new(queue_mgr, preset_registry);
                app = app.with_config_dir(config_dir).with_title_bar(title_bar);
                app.prefs_state.prefs = prefs;

                Ok(Box::new(DesktopApp {
                    inner: app,
                    window: window::Tracker::new(saved_window),
                    config_dir: window_config_dir,
                    _ipc_tcp: ipc_tcp,
                    #[cfg(unix)]
                    _ipc_unix: ipc_unix,
                }))
            }),
        )
    }));
    outcome.unwrap_or_else(|_| {
        log::error!("the window stopped after an internal error");
        Err(eframe::Error::AppCreation("internal error in the window system".into()))
    })
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
