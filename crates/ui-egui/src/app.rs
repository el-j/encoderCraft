//! encoderCraft Desktop Application Root State & egui View.

use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use encodercraft_core::CraftJobSpec;
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;

use crate::inspector::{InspectorState, render_inspector_modal};
use crate::preset_browser::{PresetBrowserState, render_preset_browser};
use crate::preview_panel::{PreviewState, render_preview_panel};
use crate::queue_panel::{QueuePanelState, render_queue_panel};
use crate::theme::{Brightness, Tokens, apply_theme};

/// The root egui application for encoderCraft.
pub struct EncoderCraftApp {
    pub queue_mgr: Arc<QueueManager>,
    pub preset_registry: Arc<Mutex<PresetRegistry>>,
    pub tokens: Tokens,
    pub brightness: Brightness,
    pub queue_state: QueuePanelState,
    pub preset_state: PresetBrowserState,
    pub preview_state: PreviewState,
    pub inspector_state: InspectorState,
    pub show_about_dialog: bool,
}

impl EncoderCraftApp {
    /// Creates a new encoderCraft application instance.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        let brightness = Brightness::Dark;
        let tokens = Tokens::for_brightness(brightness);

        Self {
            queue_mgr,
            preset_registry,
            tokens,
            brightness,
            queue_state: QueuePanelState::default(),
            preset_state: PresetBrowserState::default(),
            preview_state: PreviewState::default(),
            inspector_state: InspectorState::default(),
            show_about_dialog: false,
        }
    }

    /// Renders the complete application frame inside an egui Ui.
    pub fn render(&mut self, ui: &mut Ui) {
        apply_theme(ui.ctx(), &self.tokens);

        // 1. Top Application Menu Bar
        egui::Panel::top("top_menu_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Add Media File...").clicked() {
                        self.quick_add_demo_job();
                        ui.close();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    ui.menu_button("Theme", |ui| {
                        if ui.button("Dark").clicked() {
                            self.brightness = Brightness::Dark;
                            self.tokens = Tokens::for_brightness(self.brightness);
                        }
                        if ui.button("High Contrast").clicked() {
                            self.brightness = Brightness::HighContrast;
                            self.tokens = Tokens::for_brightness(self.brightness);
                        }
                    });
                });

                ui.menu_button("Queue", |ui| {
                    if ui.button("Start Queue").clicked() {
                        self.queue_state.is_running = true;
                        let _ = self.queue_mgr.process_next_job();
                    }
                    if ui.button("Pause Queue").clicked() {
                        self.queue_state.is_running = false;
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About encoderCraft").clicked() {
                        self.show_about_dialog = true;
                        ui.close();
                    }
                });
            });
        });

        // 2. Bottom Status Bar
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let jobs = self.queue_mgr.list_jobs();
                let completed_count = jobs.iter().filter(|j| j.status == encodercraft_core::JobStatus::Completed).count();
                ui.label(RichText::new(format!("Queue: {} jobs ({} finished)", jobs.len(), completed_count)).small().color(self.tokens.text_dim));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Pure Rust · Memory Safe · Parity Ready").small().color(self.tokens.text_dim));
                });
            });
        });

        // 3. Central Workspace with Side Panels
        let mut inspect_job = None;

        egui::Panel::right("right_side_dock").resizable(true).default_size(360.0).show(ui, |ui| {
            render_preview_panel(ui, &mut self.preview_state, None, &self.tokens);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            render_preset_browser(
                ui,
                &mut self.preset_state,
                &self.preset_registry,
                &self.queue_mgr,
                self.queue_state.selected_job_id.as_deref(),
                &self.tokens,
            );
        });

        egui::CentralPanel::default().show(ui, |ui| {
            render_queue_panel(ui, &mut self.queue_state, &self.queue_mgr, &self.tokens, &mut inspect_job);
        });

        // 4. Handle Inspector Modal
        if let Some(spec) = inspect_job {
            self.inspector_state.job_spec = Some(spec);
            self.inspector_state.is_open = true;
        }

        let mut saved_spec = None;
        let ctx = ui.ctx().clone();
        render_inspector_modal(&ctx, &mut self.inspector_state, &self.tokens, &mut saved_spec);

        if let Some(new_spec) = saved_spec {
            let job_id = new_spec.job_id.clone();
            let _ = self.queue_mgr.remove_job(&job_id);
            let _ = self.queue_mgr.submit_job(new_spec);
        }

        // 5. About Dialog
        if self.show_about_dialog {
            egui::Window::new("About encoderCraft").collapsible(false).resizable(false).open(&mut self.show_about_dialog).show(&ctx, |ui| {
                ui.heading("encoderCraft 0.1.0");
                ui.label("A pure-Rust, universal media transcoding & batch queue system.");
                ui.label("Designed for seamless ArtCraft upstream integration.");
                ui.add_space(4.0);
                ui.label(RichText::new("Clean-Room Adobe Media Encoder Parity").strong());
            });
        }
    }

    fn quick_add_demo_job(&self) {
        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: format!("job-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)),
            client_info: encodercraft_core::ClientInfo {
                app_name: "encoderCraft GUI".into(),
                app_version: "0.1.0".into(),
                pid: std::process::id(),
                user_notes: None,
            },
            priority: encodercraft_core::JobPriority::Normal,
            start_policy: encodercraft_core::StartPolicy::AutoStart,
            source: encodercraft_core::SourceDescriptor::File {
                path: "sample_footage.mov".into(),
                start_tick: None,
                duration_ticks: None,
                timebase: None,
            },
            targets: vec![encodercraft_core::OutputTarget {
                target_id: "target-1".into(),
                output_path: "sample_footage_youtube.mp4".into(),
                preset_name: "YouTube 1080p Full HD".into(),
                container: encodercraft_core::ContainerType::IsoBmffMp4,
                custom_width: Some(1920),
                custom_height: Some(1080),
                custom_frame_rate: None,
                target_bitrate_kbps: Some(16000),
                target_lufs: Some(-14.0),
            }],
        };
        let _ = self.queue_mgr.submit_job(spec);
    }
}
