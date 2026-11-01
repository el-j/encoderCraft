//! encoderCraft desktop application executable.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::EncoderCraftApp;

struct DesktopApp {
    inner: EncoderCraftApp,
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.inner.render(ui);
    }
}

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("encoderCraft").with_inner_size([1120.0, 720.0]).with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native("encoderCraft", options, Box::new(|_cc| Ok(Box::new(DesktopApp { inner: EncoderCraftApp::new(queue_mgr, preset_registry) }))))
}
