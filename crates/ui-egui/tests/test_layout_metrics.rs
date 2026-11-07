//! Automated visual regression & layout metrics tests verifying panel insets,
//! row heights, and table column boundary alignments (AME Parity).
//!
//! Validates issues #138 and #150 acceptance criteria.

use std::sync::{Arc, Mutex};

use encodercraft_core::{ClientInfo, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use encodercraft_ui_egui::media_browser::MediaBrowserColumnWidths;
use encodercraft_ui_egui::queue_panel::QueueColumnWidths;
use encodercraft_ui_egui::{Brightness, EncoderCraftApp, Tokens};

#[test]
fn test_theme_tokens_layout_metrics() {
    for brightness in [Brightness::Dark, Brightness::MediumDark, Brightness::Light, Brightness::HighContrast] {
        let tokens = Tokens::for_brightness(brightness);

        // AME Reference layout metrics
        assert_eq!(tokens.panel_inset, 8.0, "Panel inset must be 8.0px across all themes");
        assert_eq!(tokens.row_height, 22.0, "Standard row pitch must be 22.0px across all themes");
        assert_eq!(tokens.toolbar_gap, 4.0, "Toolbar button gap must be 4.0px");
        assert_eq!(tokens.section_gap, 6.0, "Section gap must be 6.0px");
        assert_eq!(tokens.panel_margin(), egui::Margin::same(8), "Panel margin must match panel_inset");
    }
}

#[test]
fn test_queue_column_widths_strict_alignment() {
    // Header boundaries and table cell boundaries must align exactly.
    for available_width in [500.0, 720.0, 1024.0, 1440.0, 1920.0] {
        let spacing = 4.0;
        let cols = QueueColumnWidths::calculate(available_width, spacing);

        // Fixed columns adhere to reference widths
        assert_eq!(cols.format, QueueColumnWidths::FORMAT);
        assert_eq!(cols.preset, QueueColumnWidths::PRESET);
        assert_eq!(cols.output, QueueColumnWidths::OUTPUT);
        assert_eq!(cols.status, QueueColumnWidths::STATUS);

        // Source column expands to fill available width, minimum 160px
        let fixed_total = cols.format + cols.preset + cols.output + cols.status + spacing * 4.0;
        let expected_src = (available_width - fixed_total).max(160.0);
        assert_eq!(cols.src, expected_src);
        assert!(cols.src >= 160.0);
    }
}

#[test]
fn test_media_browser_column_widths_strict_alignment() {
    // Media browser table header boundaries and file row cell boundaries must align.
    for available_width in [400.0f32, 600.0f32, 900.0f32, 1280.0f32] {
        let cols = MediaBrowserColumnWidths::compute(available_width);

        // Fixed columns adhere to reference widths
        assert_eq!(cols.kind, 70.0);
        assert_eq!(cols.size, 65.0);
        assert_eq!(cols.date, 115.0);

        // Name column expands to fill remainder, minimum 120px
        let expected_name = (available_width - 70.0 - 65.0 - 115.0 - 24.0).max(120.0);
        assert_eq!(cols.name, expected_name);
        assert!(cols.name >= 120.0);
    }
}

#[test]
fn test_headless_frame_renders_without_margin_overflow() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());

    // Populate queue with multi-target jobs
    let job1 = CraftJobSpec {
        version: "1.0".into(),
        job_id: "test-job-001".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
        priority: JobPriority::High,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: "directselect_video_caption.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![
            OutputTarget { output_path: "output_1080p.mp4".into(), preset_name: "YouTube 1080p".into(), ..Default::default() },
            OutputTarget { output_path: "output_prores.mov".into(), preset_name: "Apple ProRes 422 HQ".into(), ..Default::default() },
        ],
    };
    let _ = queue_mgr.submit_job(job1);

    let job2 = CraftJobSpec {
        version: "1.0".into(),
        job_id: "test-job-002".into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 2, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: "b_roll_nature.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget { output_path: "b_roll_master.mxf".into(), preset_name: "Broadcast MXF OP-1a".into(), ..Default::default() }],
    };
    let _ = queue_mgr.submit_job(job2);

    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let mut app = EncoderCraftApp::new(queue_mgr, preset_reg);

    let ctx = egui::Context::default();
    let width = 1280.0;
    let height = 800.0;
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))), ..Default::default() };

    let mut output = ctx.run_ui(input, |ui| {
        app.render(ui);
    });

    assert!(!output.shapes.is_empty(), "Headless render should produce visual shapes");

    // Scan shapes for text boundaries
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    for clipped_shape in &output.shapes {
        if let egui::epaint::Shape::Text(text_shape) = &clipped_shape.shape {
            let bbox = text_shape.galley.rect.translate(text_shape.pos.to_vec2());
            // Only test text shapes that are within the visible clip rect
            if clipped_shape.clip_rect.intersects(screen) && clipped_shape.clip_rect.contains(bbox.min) {
                assert!(bbox.min.x >= -1.0 && bbox.min.y >= -1.0, "Text shape at {:?} has negative coordinates outside screen area", bbox);
                assert!(screen.contains(bbox.min), "Text shape min {:?} is outside screen bounds {:?}", bbox.min, screen);
            }
        }
    }

    output.textures_delta.clear();
}
