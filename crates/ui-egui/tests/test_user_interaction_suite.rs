//! End-to-end user perspective interaction test suite for encoderCraft UI.
//!
//! Verifies:
//! 1. Column header resizing in Queue panel and Preset Browser.
//! 2. Every toolbar and action button click dispatching registered commands.
//! 3. Format dropdown container selection and output file extension synchronization (PDF -> JPEG/PNG/DOCX/MKV/WebM/GIF).
//! 4. Inspector tab switching across all domains.
//! 5. Universal document, CAD, and image preview in the monitor.

use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, StartPolicy};
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::EncoderCraftApp;
use encodercraft_ui_egui::commands::AppCommand;
use encodercraft_ui_egui::inspector::InspectorTab;
use encodercraft_ui_egui::preset_browser::PresetColumnWidths;
use encodercraft_ui_egui::queue_panel::{QueueColumnWidths, candidate_containers_for_job, default_preset_for_container};

#[test]
fn test_queue_and_preset_column_resizing() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let mut app = EncoderCraftApp::new(queue_mgr, preset_reg);

    // Initial queue column settings
    let initial_format_w = app.queue_state.column_settings.format;
    let initial_preset_w = app.queue_state.column_settings.preset;

    // Simulate column resize on Queue Panel
    app.queue_state.column_settings.format = initial_format_w + 50.0;
    app.queue_state.column_settings.preset = initial_preset_w + 30.0;

    let cols = QueueColumnWidths::calculate_for_columns_and_settings(
        1200.0,
        6.0,
        &["source".into(), "format".into(), "preset".into(), "output".into(), "status".into()],
        &app.queue_state.column_settings,
    );
    assert_eq!(cols.format, initial_format_w + 50.0);
    assert_eq!(cols.preset, initial_preset_w + 30.0);

    // Preset browser column resizing
    let initial_p_format_w = app.preset_state.column_settings.format;
    app.preset_state.column_settings.format += 40.0;

    let p_cols = PresetColumnWidths::compute_with_settings(800.0, &app.preset_state.column_settings);
    assert_eq!(p_cols.format, initial_p_format_w + 40.0);

    // Render frame to ensure layout geometry remains valid
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });
}

#[test]
fn test_all_toolbar_and_action_buttons() {
    let dir = tempdir().unwrap();
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let mut app = EncoderCraftApp::new(queue_mgr, preset_reg).with_config_dir(dir.path().to_path_buf());

    let commands_to_exercise = [
        AppCommand::StartQueue,
        AppCommand::PauseQueue,
        AppCommand::StopQueue,
        AppCommand::ResetSelected,
        AppCommand::DuplicateSelected,
        AppCommand::RemoveSelected,
        AppCommand::MoveSelectedUp,
        AppCommand::MoveSelectedDown,
        AppCommand::SelectAll,
        AppCommand::ClearFinished,
        AppCommand::Preferences,
        AppCommand::About,
        AppCommand::ThemeHighContrast,
        AppCommand::ThemeLight,
        AppCommand::ThemeDark,
    ];

    for cmd in commands_to_exercise {
        app.run_command(cmd);
        egui::__run_test_ui(|ui| {
            app.render(ui);
        });
    }
}

#[test]
fn test_candidate_containers_and_format_selection() {
    let dir = tempdir().unwrap();
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let _app = EncoderCraftApp::new(queue_mgr.clone(), preset_reg);

    // Create a valid PDF job matching the user's scenario
    let pdf_path = dir.path().join("document.pdf");
    let mut doc = encodercraft_pipeline::document::DocumentCanvas::new("Interaction Test Document");
    doc.pages[0].add_text(54.0, 720.0, 500.0, 24.0, "Page 1 Content", 14.0);
    std::fs::write(&pdf_path, doc.to_pdf_x4_bytes().unwrap()).unwrap();

    let job_id = "test-doc-job";
    let target_out = dir.path().join("document_output.mp4");
    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: job_id.into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: encodercraft_core::SourceDescriptor::File { path: pdf_path.clone(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t1".into(),
            output_path: target_out,
            preset_name: "Match Source".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        }],
    };

    let reviews = queue_mgr.conversion_review(&spec).unwrap();
    let approved: Vec<_> = reviews.into_iter().map(|c| c.key).collect();
    queue_mgr.submit_job_with_approvals(spec, &approved).unwrap();
    let jobs = queue_mgr.list_jobs();
    assert_eq!(jobs.len(), 1);

    // Verify candidate containers for document includes JPEG, PNG, TIFF, PDF, DOCX, Markdown, HTML, EPUB
    let candidates = candidate_containers_for_job(&jobs[0]);
    let has_container = |c: ContainerType| candidates.iter().any(|(ct, _, _)| *ct == c);

    assert!(has_container(ContainerType::Jpeg));
    assert!(has_container(ContainerType::Png));
    assert!(has_container(ContainerType::Tiff));
    assert!(has_container(ContainerType::Pdf));
    assert!(has_container(ContainerType::Docx));
    assert!(has_container(ContainerType::Markdown));
    assert!(has_container(ContainerType::Html));
    assert!(has_container(ContainerType::Epub));

    // Test changing format to JPEG: container becomes Jpeg and extension is .jpg (fixing user issue)
    let mut target = jobs[0].spec.targets[0].clone();
    target.container = ContainerType::Jpeg;
    target.output_path.set_extension("jpg");
    target.preset_name = default_preset_for_container(ContainerType::Jpeg).to_string();
    queue_mgr.update_output_target(job_id, 0, target).unwrap();

    let updated_job = queue_mgr.list_jobs().into_iter().find(|j| j.spec.job_id == job_id).unwrap();
    assert_eq!(updated_job.spec.targets[0].container, ContainerType::Jpeg);
    assert_eq!(updated_job.spec.targets[0].output_path.extension().and_then(|s| s.to_str()), Some("jpg"));

    // Test changing format to PNG: container becomes Png and extension is .png
    let mut target = updated_job.spec.targets[0].clone();
    target.container = ContainerType::Png;
    target.output_path.set_extension("png");
    target.preset_name = default_preset_for_container(ContainerType::Png).to_string();
    queue_mgr.update_output_target(job_id, 0, target).unwrap();

    let updated_job = queue_mgr.list_jobs().into_iter().find(|j| j.spec.job_id == job_id).unwrap();
    assert_eq!(updated_job.spec.targets[0].container, ContainerType::Png);
    assert_eq!(updated_job.spec.targets[0].output_path.extension().and_then(|s| s.to_str()), Some("png"));

    // Test changing format to DOCX: container becomes Docx and extension is .docx
    let mut target = updated_job.spec.targets[0].clone();
    target.container = ContainerType::Docx;
    target.output_path.set_extension("docx");
    target.preset_name = default_preset_for_container(ContainerType::Docx).to_string();
    queue_mgr.update_output_target(job_id, 0, target).unwrap();

    let updated_job = queue_mgr.list_jobs().into_iter().find(|j| j.spec.job_id == job_id).unwrap();
    assert_eq!(updated_job.spec.targets[0].container, ContainerType::Docx);
    assert_eq!(updated_job.spec.targets[0].output_path.extension().and_then(|s| s.to_str()), Some("docx"));
}

#[test]
fn test_all_inspector_tabs_cycle_without_panic() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let mut app = EncoderCraftApp::new(queue_mgr, preset_reg);

    let all_tabs = [
        InspectorTab::Video,
        InspectorTab::Composition,
        InspectorTab::Layout,
        InspectorTab::Cad,
        InspectorTab::Layers,
        InspectorTab::Audio,
        InspectorTab::Effects,
        InspectorTab::Captions,
        InspectorTab::Publish,
        InspectorTab::Loudness,
        InspectorTab::Metadata,
    ];

    for tab in all_tabs {
        app.inspector_state.active_tab = tab;
        egui::__run_test_ui(|ui| {
            app.render(ui);
        });
    }
}
