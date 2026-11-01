//! Headless tests verifying encoderCraft egui UI construction and frame rendering.

use std::sync::{Arc, Mutex};

use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::{Brightness, EncoderCraftApp, Tokens};

#[test]
fn test_app_creation_and_frame_render() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));

    let mut app = EncoderCraftApp::new(queue_mgr, preset_reg);

    // Render a headless frame using egui's test harness
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    // Switch theme and render again
    app.brightness = Brightness::HighContrast;
    app.tokens = Tokens::for_brightness(app.brightness);

    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    assert!(!app.queue_state.is_running);
}
