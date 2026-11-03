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

#[test]
fn commands_and_workspaces_drive_the_frame() {
    use encodercraft_ui_egui::commands::AppCommand;
    use encodercraft_ui_egui::workspace::{PREVIEW_WORKSPACE, PanelId};

    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let mut app = EncoderCraftApp::new(queue_mgr, Arc::new(Mutex::new(PresetRegistry::default())));

    app.run_command(AppCommand::ThemeHighContrast);
    assert_eq!(app.brightness, Brightness::HighContrast);
    app.run_command(AppCommand::About);
    assert!(app.show_about_dialog);
    app.run_command(AppCommand::Preferences);
    assert!(app.prefs_state.is_open);

    for name in app.workspaces.names() {
        app.workspaces.activate(&name);
        egui::__run_test_ui(|ui| app.render(ui));
    }
    app.workspaces.activate(PREVIEW_WORKSPACE);
    if let Some(w) = app.workspaces.current_mut() {
        w.panels.retain(|p| *p == PanelId::Encoding);
    }
    egui::__run_test_ui(|ui| app.render(ui));
}

#[test]
fn workspace_file_is_written_and_restored() {
    let dir = tempfile::tempdir().unwrap();
    let path = encodercraft_ui_egui::settings::workspaces_path(dir.path());
    assert!(!path.exists());
    let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
        .with_config_dir(dir.path().to_path_buf());
    app.workspaces.save_as("Mine").unwrap();
    egui::__run_test_ui(|ui| app.render(ui));
    let again = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
        .with_config_dir(dir.path().to_path_buf());
    assert_eq!(again.workspaces.active, "Mine");
    assert!(path.exists());
}

#[test]
fn preferences_watch_folders_and_favorites_persist() {
    use encodercraft_ui_egui::settings;
    let dir = tempfile::tempdir().unwrap();
    let new_app = || {
        EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
            .with_config_dir(dir.path().to_path_buf())
    };
    let mut app = new_app();
    app.prefs_state.prefs.max_parallel_jobs = 5;
    app.prefs_state.prefs.play_completion_sound = false;
    app.watch_state.configs.push(encodercraft_watch::WatchFolderConfig::new("w1", "/in".into(), "/out".into(), "P"));
    app.media_state.model.toggle_favorite(dir.path());
    egui::__run_test_ui(|ui| app.render(ui));
    assert!(settings::preferences_path(dir.path()).exists());

    let again = new_app();
    assert_eq!(again.prefs_state.prefs.max_parallel_jobs, 5);
    assert!(!again.prefs_state.prefs.play_completion_sound);
    assert_eq!(again.watch_state.configs.len(), 1);
    assert_eq!(again.media_state.model.favorites, vec![dir.path().to_path_buf()]);
    assert_eq!(again.queue_mgr.parallelism(), 5);
}
