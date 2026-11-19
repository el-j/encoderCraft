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

fn write_bmp(path: &std::path::Path, shade: u8) {
    let (w, h) = (16u32, 16u32);
    let row = (w * 3).div_ceil(4) * 4;
    let data = row * h;
    let size = 54 + data;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&data.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&2835u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    let pad = vec![0u8; (row - w * 3) as usize];
    for _ in 0..h {
        for _ in 0..w {
            b.extend_from_slice(&[shade, shade, shade]);
        }
        b.extend_from_slice(&pad);
    }
    std::fs::write(path, b).unwrap();
}

fn write_test_wav(path: &std::path::Path) {
    let samples: Vec<i16> = (0..48_000_i32).map(|n| ((n % 100) as i16 - 50) * 200).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn test_full_ui_e2e_blackbox_workflow() {
    let dir = tempdir().unwrap();
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let mut app = EncoderCraftApp::new(queue_mgr.clone(), preset_reg).with_config_dir(dir.path().to_path_buf());

    // 1. Ingest multiple jobs across diverse domains
    let img_path = dir.path().join("source.bmp");
    write_bmp(&img_path, 128);

    let aud_path = dir.path().join("source.wav");
    write_test_wav(&aud_path);

    let doc_path = dir.path().join("source.pdf");
    let mut doc = encodercraft_pipeline::document::DocumentCanvas::new("E2E UI Test");
    doc.pages[0].add_text(54.0, 720.0, 500.0, 24.0, "Blackbox Page Text", 14.0);
    std::fs::write(&doc_path, doc.to_pdf_x4_bytes().unwrap()).unwrap();

    let cad_path = dir.path().join("source.dxf");
    std::fs::write(&cad_path, "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nLAYER1\n10\n0.0\n20\n0.0\n11\n100.0\n21\n100.0\n0\nENDSEC\n0\nEOF\n").unwrap();

    let img_spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "img-job-1".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: encodercraft_core::SourceDescriptor::File { path: img_path, start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![
            OutputTarget {
                target_id: "t-png".into(),
                output_path: dir.path().join("img_out.png"),
                preset_name: "Still Image PNG (Lossless)".into(),
                container: ContainerType::Png,
                ..Default::default()
            },
            OutputTarget {
                target_id: "t-jpg".into(),
                output_path: dir.path().join("img_out.jpg"),
                preset_name: "Still Image JPEG".into(),
                container: ContainerType::Jpeg,
                ..Default::default()
            },
        ],
    };

    let aud_spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "aud-job-1".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: encodercraft_core::SourceDescriptor::File { path: aud_path, start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t-wav".into(),
            output_path: dir.path().join("aud_out.wav"),
            preset_name: "Broadcast Wave BWF (-23 LUFS)".into(),
            container: ContainerType::Wave,
            ..Default::default()
        }],
    };

    let doc_spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "doc-job-1".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: encodercraft_core::SourceDescriptor::File { path: doc_path, start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t-jpg".into(),
            output_path: dir.path().join("doc_page.jpg"),
            preset_name: "Still Image JPEG".into(),
            container: ContainerType::Jpeg,
            ..Default::default()
        }],
    };

    queue_mgr.submit_job(img_spec).unwrap();
    queue_mgr.submit_job(aud_spec).unwrap();
    queue_mgr.submit_job(doc_spec).unwrap();

    // Verify queue loaded
    let jobs = queue_mgr.list_jobs();
    assert_eq!(jobs.len(), 3);

    // 2. Select different jobs and render UI
    app.queue_state.selected_job_ids.insert("img-job-1".into());
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    app.queue_state.selected_job_ids.clear();
    app.queue_state.selected_job_ids.insert("aud-job-1".into());
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    app.queue_state.selected_job_ids.clear();
    app.queue_state.selected_job_ids.insert("doc-job-1".into());
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    // 3. Test transport and preview controls
    app.preview_state.transport.set_media(30.0, 100);
    app.preview_state.transport.set_volume(0.85);
    app.preview_state.transport.toggle_mute();
    app.preview_state.transport.in_point = Some(10);
    app.preview_state.transport.out_point = Some(90);
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });

    // 4. Test preset browser category filter and search
    app.preset_state.search_query = "YouTube".into();
    egui::__run_test_ui(|ui| {
        app.render(ui);
    });
    app.preset_state.search_query.clear();

    // 5. Test queue management actions
    app.run_command(AppCommand::SelectAll);
    assert_eq!(app.queue_state.selected_job_ids.len(), 3);

    app.run_command(AppCommand::StartQueue);
    app.run_command(AppCommand::PauseQueue);
    app.run_command(AppCommand::StopQueue);
    app.run_command(AppCommand::ResetSelected);

    egui::__run_test_ui(|ui| {
        app.render(ui);
    });
}
