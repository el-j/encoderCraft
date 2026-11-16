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
fn app_renders_at_minimum_size_and_high_dpi_scales() {
    let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));
    let ctx = egui::Context::default();
    for (width, height, ppp) in [(720.0, 480.0, 1.0), (1120.0, 720.0, 1.5), (1440.0, 960.0, 2.0)] {
        ctx.set_pixels_per_point(ppp);
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.render(ui));
        assert!(!output.shapes.is_empty(), "expected visible UI shapes at {width}x{height}, {ppp}x");
        output.textures_delta.clear();
    }
}

#[test]
fn commands_and_workspaces_drive_the_frame() {
    use encodercraft_ui_egui::commands::AppCommand;
    use encodercraft_ui_egui::workspace::{PREVIEW_WORKSPACE, PanelId};

    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let config = tempfile::tempdir().unwrap();
    let mut app = EncoderCraftApp::new(queue_mgr, Arc::new(Mutex::new(PresetRegistry::default()))).with_config_dir(config.path().to_path_buf());

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
        for panel in PanelId::ALL {
            if panel != PanelId::Encoding {
                w.set_visible(panel, false).unwrap();
            }
        }
    }
    egui::__run_test_ui(|ui| app.render(ui));
}

#[test]
fn workspace_file_is_written_and_restored() {
    let dir = tempfile::tempdir().unwrap();
    let path = encodercraft_ui_egui::settings::workspaces_path(dir.path());
    let shortcuts_path = encodercraft_ui_egui::settings::shortcuts_path(dir.path());
    assert!(!path.exists());
    let mut app = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
        .with_config_dir(dir.path().to_path_buf());
    app.workspaces.save_as("Mine").unwrap();
    app.workspace_shortcuts.set_binding(encodercraft_ui_egui::settings::WorkspaceShortcutAction::FocusNext, "Alt+F8".into()).unwrap();
    egui::__run_test_ui(|ui| app.render(ui));
    let again = EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())))
        .with_config_dir(dir.path().to_path_buf());
    assert_eq!(again.workspaces.active, "Mine");
    assert_eq!(again.workspace_shortcuts.focus_next, "Alt+F8");
    assert!(path.exists());
    assert!(shortcuts_path.exists());
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
    app.run_command(encodercraft_ui_egui::commands::AppCommand::ThemeLight);
    assert_eq!(app.prefs_state.prefs.theme, "Light");
    app.watch_state.configs.push(encodercraft_watch::WatchFolderConfig::new("w1", "/in".into(), "/out".into(), "P"));
    app.media_state.model.toggle_favorite(dir.path());
    egui::__run_test_ui(|ui| app.render(ui));
    assert!(settings::preferences_path(dir.path()).exists());
    assert_eq!(encodercraft_engine::load_preferences(&settings::preferences_path(dir.path())).unwrap().theme, "Light");

    let again = new_app();
    assert_eq!(again.prefs_state.prefs.max_parallel_jobs, 5);
    assert!(!again.prefs_state.prefs.play_completion_sound);
    assert_eq!(again.brightness, Brightness::Light);
    assert_eq!(again.watch_state.configs.len(), 1);
    assert_eq!(again.media_state.model.favorites, vec![dir.path().to_path_buf()]);
    assert_eq!(again.queue_mgr.parallelism(), 5);
}

#[test]
fn app_renders_with_every_title_bar_style() {
    use encodercraft_ui_egui::titlebar::TitleBarStyle;
    for style in [TitleBarStyle::System, TitleBarStyle::Custom, TitleBarStyle::Integrated] {
        let mut app =
            EncoderCraftApp::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default()))).with_title_bar(style);
        let ctx = egui::Context::default();
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1120.0, 720.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.render(ui));
        assert!(!output.shapes.is_empty(), "{style:?} rendered nothing");
        output.textures_delta.clear();
    }
}

#[test]
fn app_on_exit_removes_finished_jobs_when_preference_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.json");
    let mut journal = encodercraft_engine::QueueJournal::default();
    let mut job = encodercraft_engine::QueuedJob::new(encodercraft_core::CraftJobSpec {
        version: "1.0".into(),
        job_id: "done-1".into(),
        client_info: encodercraft_core::ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: encodercraft_core::JobPriority::Normal,
        start_policy: encodercraft_core::StartPolicy::AutoStart,
        source: encodercraft_core::SourceDescriptor::File { path: "in.wav".into(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![],
    });
    job.status = encodercraft_core::JobStatus::Completed;
    journal.jobs.push(job);
    encodercraft_engine::save_journal_atomic(&journal_path, &journal).unwrap();

    let queue = Arc::new(QueueManager::new_persisted(journal_path).unwrap());
    assert_eq!(queue.list_jobs().len(), 1);

    let mut app = EncoderCraftApp::new(queue.clone(), Arc::new(Mutex::new(PresetRegistry::default())));
    app.prefs_state.prefs.remove_finished_on_exit = true;
    app.on_exit();
    assert!(queue.list_jobs().is_empty());
}
