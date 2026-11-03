//! encoderCraft Desktop Application Root State & egui View.

use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use encodercraft_core::CraftJobSpec;
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;

use crate::icons;
use crate::inspector::{InspectorState, render_inspector_modal};
use crate::log_dialog::{LogDialogState, render_log_dialog};
use crate::prefs_dialog::{PrefsDialogState, render_prefs_dialog};
use crate::preset_browser::{PresetBrowserState, render_preset_browser};
use crate::preview_panel::{PreviewState, render_preview_panel};
use crate::queue_panel::{QueuePanelState, render_queue_panel};
use crate::theme::{Brightness, Tokens, apply_theme};
use crate::watch_panel::{WatchPanelState, render_watch_panel, trigger_watch_scan};

/// The primary view tab in the central workspace (AME Parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainViewTab {
    #[default]
    Queue,
    WatchFolders,
    MediaBrowser,
}

/// The root egui application for encoderCraft.
pub struct EncoderCraftApp {
    pub queue_mgr: Arc<QueueManager>,
    pub preset_registry: Arc<Mutex<PresetRegistry>>,
    pub tokens: Tokens,
    pub brightness: Brightness,
    pub active_tab: MainViewTab,
    pub queue_state: QueuePanelState,
    pub shutdown_watch: crate::queue_footer::ShutdownWatch,
    pub shutdown_error: Option<String>,
    pub watch_state: WatchPanelState,
    pub media_state: crate::media_browser::MediaBrowserState,
    pub preset_state: PresetBrowserState,
    pub preview_state: PreviewState,
    pub encoding_state: crate::encoding_panel::EncodingPanelState,
    pub inspector_state: InspectorState,
    pub log_state: LogDialogState,
    pub prefs_state: PrefsDialogState,
    pub show_about_dialog: bool,
    pub last_watch_tick: Option<std::time::Instant>,
    pub was_worker_running: bool,
    pub completion_banner_timer: Option<std::time::Instant>,
    pub workspaces: crate::workspace::WorkspaceSet,
    config_dir: Option<std::path::PathBuf>,
    last_saved_prefs: Option<encodercraft_engine::AppPreferences>,
    last_saved_watch: Option<Vec<encodercraft_watch::WatchFolderConfig>>,
    last_saved_favorites: Option<Vec<std::path::PathBuf>>,
    workspace_name_input: String,
    last_saved_workspaces: Option<crate::workspace::WorkspaceSet>,
    fonts_installed: bool,
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
            active_tab: MainViewTab::Queue,
            queue_state: QueuePanelState::default(),
            shutdown_watch: crate::queue_footer::ShutdownWatch::default(),
            shutdown_error: None,
            watch_state: WatchPanelState::default(),
            media_state: crate::media_browser::MediaBrowserState::default(),
            preset_state: PresetBrowserState::default(),
            preview_state: PreviewState::default(),
            encoding_state: crate::encoding_panel::EncodingPanelState::default(),
            inspector_state: InspectorState::default(),
            log_state: LogDialogState::default(),
            prefs_state: PrefsDialogState::default(),
            show_about_dialog: false,
            last_watch_tick: None,
            was_worker_running: false,
            completion_banner_timer: None,
            workspaces: crate::workspace::WorkspaceSet::default(),
            config_dir: None,
            last_saved_prefs: None,
            last_saved_watch: None,
            last_saved_favorites: None,
            workspace_name_input: String::new(),
            last_saved_workspaces: None,
            fonts_installed: false,
        }
    }

    /// Loads preferences, watch folders and workspaces from `dir` and keeps saving changes back.
    pub fn with_config_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.workspaces = crate::workspace::WorkspaceSet::load(&crate::settings::workspaces_path(&dir));
        self.last_saved_workspaces = Some(self.workspaces.clone());
        match encodercraft_engine::load_preferences(&crate::settings::preferences_path(&dir)) {
            Ok(prefs) => self.prefs_state.prefs = prefs,
            Err(e) => log::warn!("using default preferences: {e}"),
        }
        self.last_saved_prefs = Some(self.prefs_state.prefs.clone());
        let brightness = if self.prefs_state.prefs.theme == "High Contrast" { Brightness::HighContrast } else { Brightness::Dark };
        self.set_brightness(brightness);
        self.watch_state.configs = crate::settings::load_watch_folders(&crate::settings::watch_folders_path(&dir));
        self.last_saved_watch = Some(self.watch_state.configs.clone());
        self.media_state.model.favorites = crate::settings::load_favorites(&crate::settings::favorites_path(&dir));
        self.last_saved_favorites = Some(self.media_state.model.favorites.clone());
        let _ = self.queue_mgr.set_parallelism(self.prefs_state.prefs.max_parallel_jobs);
        self.config_dir = Some(dir);
        self
    }

    fn command_state(&self) -> crate::commands::CommandState {
        let jobs = self.queue_mgr.list_jobs();
        crate::commands::CommandState {
            selected: self.queue_state.selected_job_ids.len(),
            jobs: jobs.len(),
            finished: jobs.iter().filter(|j| j.status == encodercraft_core::JobStatus::Completed).count(),
            worker_running: self.queue_mgr.is_worker_running(),
        }
    }

    fn pick_media(&self) {
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Select Media Files")
            .add_filter(
                "Media Files",
                &[
                    "mov", "mp4", "mkv", "avi", "webm", "mxf", "wav", "aiff", "flac", "mp3", "ogg", "opus", "png", "jpg", "jpeg", "webp", "exr",
                    "tiff", "bmp", "gif", "svg", "pdf", "docx", "idml", "gltf", "glb", "usdz", "obj", "stl", "ply",
                ],
            )
            .pick_files()
        {
            for path in files {
                self.add_file_job(path);
            }
        }
    }

    /// Runs a command from a menu, shortcut or context menu. All entry points share this.
    pub fn run_command(&mut self, cmd: crate::commands::AppCommand) {
        use crate::commands::AppCommand as C;
        if !cmd.enabled(&self.command_state()) {
            return;
        }
        let ids = self.queue_state.selected_ids_list();
        match cmd {
            C::AddMedia => self.pick_media(),
            C::AddStitched => {
                if let Some(files) = rfd::FileDialog::new().set_title("Select Clips to Stitch Together").pick_files()
                    && files.len() > 1
                    && let Some(first) = files.first()
                {
                    let out = first.with_file_name("stitched_sequence.mp4");
                    let _ = self.queue_mgr.stitch_sources_into_job(files, out, "Match Source - High Bitrate".into());
                }
            }
            C::AddWatchFolder => {
                if let Some(watch_dir) = rfd::FileDialog::new().set_title("Select Directory to Watch").pick_folder() {
                    let out_dir = watch_dir.join("_encoded");
                    let id = format!("watch-{}", self.watch_state.configs.len() + 1);
                    let cfg = encodercraft_watch::WatchFolderConfig::new(id, watch_dir, out_dir, "Match Source - High Bitrate");
                    self.watch_state.configs.push(cfg);
                    self.active_tab = MainViewTab::WatchFolders;
                }
            }
            C::AddDemoClip => self.quick_add_demo_job(),
            C::ShowLog => self.log_state.is_open = !self.log_state.is_open,
            C::Preferences => self.prefs_state.is_open = !self.prefs_state.is_open,
            C::ThemeDark => self.set_brightness(Brightness::Dark),
            C::ThemeHighContrast => self.set_brightness(Brightness::HighContrast),
            C::StartQueue => {
                self.queue_mgr.start_queue();
            }
            C::PauseQueue => self.queue_mgr.pause_worker(),
            C::StopQueue => self.queue_mgr.stop_worker(),
            C::ResetSelected => {
                for id in &ids {
                    let _ = self.queue_mgr.reset_job_status(id);
                }
            }
            C::DuplicateSelected => {
                for id in &ids {
                    let _ = self.queue_mgr.duplicate_job(id);
                }
            }
            C::RemoveSelected => {
                for id in &ids {
                    let _ = self.queue_mgr.remove_job(id);
                }
            }
            C::StitchSelected => {
                let out = std::env::temp_dir().join("stitched_batch.mp4");
                let _ = self.queue_mgr.stitch_selected_jobs(&ids, out, "Match Source - High Bitrate".into());
            }
            C::SetDestination => {
                if let Some(folder) = rfd::FileDialog::new().set_title("Select Output Directory").pick_folder() {
                    let _ = self.queue_mgr.set_jobs_output_dir(&ids, folder);
                }
            }
            C::ClearFinished => {
                let _ = self.queue_mgr.clear_completed_jobs();
            }
            C::CreatePreset => self.preset_state.editor.open_new(),
            C::About => self.show_about_dialog = true,
        }
    }

    fn set_brightness(&mut self, brightness: Brightness) {
        self.prefs_state.prefs.theme = if brightness == Brightness::HighContrast { "High Contrast".into() } else { "Dark".into() };
        self.brightness = brightness;
        self.tokens = Tokens::for_brightness(brightness);
    }

    fn window_menu(&mut self, ui: &mut Ui) {
        ui.separator();
        ui.label(RichText::new("Workspace").small().color(self.tokens.text_dim));
        for name in self.workspaces.names() {
            let active = name == self.workspaces.active;
            if ui.radio(active, &name).clicked() {
                self.workspaces.activate(&name);
            }
        }
        ui.separator();
        ui.label(RichText::new("Panels").small().color(self.tokens.text_dim));
        for panel in crate::workspace::PanelId::ALL {
            if let Some(ws) = self.workspaces.current_mut() {
                let mut shown = ws.is_visible(panel);
                if ui.checkbox(&mut shown, panel.label()).changed() {
                    ws.toggle(panel);
                }
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.workspace_name_input).hint_text("Workspace name").desired_width(120.0));
            if ui.button("Save As").clicked() && self.workspaces.save_as(&self.workspace_name_input).is_ok() {
                self.workspace_name_input.clear();
            }
        });
        if ui.button("Rename Active").clicked() && self.workspaces.rename_active(&self.workspace_name_input).is_ok() {
            self.workspace_name_input.clear();
        }
        if ui.button("Reset Active").clicked() {
            self.workspaces.reset_active();
        }
        if ui.button("Delete Active").clicked() {
            let _ = self.workspaces.delete_active();
        }
    }

    fn save_settings_if_changed(&mut self) {
        let Some(dir) = self.config_dir.clone() else { return };
        if self.last_saved_workspaces.as_ref() != Some(&self.workspaces) {
            if let Err(e) = self.workspaces.save(&crate::settings::workspaces_path(&dir)) {
                log::warn!("could not save workspaces: {e}");
            }
            self.last_saved_workspaces = Some(self.workspaces.clone());
        }
        if self.last_saved_prefs.as_ref() != Some(&self.prefs_state.prefs) {
            if let Err(e) = encodercraft_engine::save_preferences_atomic(&crate::settings::preferences_path(&dir), &self.prefs_state.prefs) {
                log::warn!("could not save preferences: {e}");
            }
            self.last_saved_prefs = Some(self.prefs_state.prefs.clone());
        }
        if self.last_saved_favorites.as_deref() != Some(self.media_state.model.favorites.as_slice()) {
            if let Err(e) = crate::settings::save_favorites(&crate::settings::favorites_path(&dir), &self.media_state.model.favorites) {
                log::warn!("could not save favorites: {e}");
            }
            self.last_saved_favorites = Some(self.media_state.model.favorites.clone());
        }
        if self.last_saved_watch.as_deref() != Some(self.watch_state.configs.as_slice()) {
            if let Err(e) = crate::settings::save_watch_folders(&crate::settings::watch_folders_path(&dir), &self.watch_state.configs) {
                log::warn!("could not save watch folders: {e}");
            }
            self.last_saved_watch = Some(self.watch_state.configs.clone());
        }
    }

    fn preview_section(&mut self, ui: &mut Ui, job: Option<&CraftJobSpec>, output_ready: bool) {
        let prev_job_id = self.preview_state.active_job_id.clone();
        let prev_in = self.preview_state.transport.in_point;
        let prev_out = self.preview_state.transport.out_point;

        render_preview_panel(ui, &mut self.preview_state, job, output_ready, &self.tokens);

        // Persist In/Out trim changes back to the selected job in QueueManager
        if let Some(spec) = job
            && prev_job_id.as_deref() == Some(spec.job_id.as_str())
            && (self.preview_state.transport.in_point != prev_in || self.preview_state.transport.out_point != prev_out)
        {
            let _ = self.queue_mgr.set_job_trim_range(
                &spec.job_id,
                self.preview_state.transport.in_point,
                self.preview_state.transport.out_point,
                self.preview_state.transport.fps().round().max(1.0) as u64,
            );
        }
    }

    /// Renders the complete application frame inside an egui Ui.
    pub fn render(&mut self, ui: &mut Ui) {
        if !self.fonts_installed {
            crate::fonts::install(ui.ctx());
            self.fonts_installed = true;
        }
        apply_theme(ui.ctx(), &self.tokens);

        for cmd in crate::commands::triggered(ui.ctx()) {
            self.run_command(cmd);
        }

        // 1. Top Application Menu Bar
        egui::Panel::top("top_menu_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let state = self.command_state();
                let mut chosen = None;
                for menu in crate::commands::Menu::ALL {
                    ui.menu_button(menu.label(), |ui| {
                        for info in crate::commands::menu_commands(menu) {
                            if info.separator_before {
                                ui.separator();
                            }
                            let mut button = egui::Button::new(info.label);
                            if let Some(sc) = info.shortcut {
                                button = button.shortcut_text(sc.display());
                            }
                            if ui.add_enabled(info.command.enabled(&state), button).clicked() {
                                chosen = Some(info.command);
                                ui.close();
                            }
                        }
                        if menu == crate::commands::Menu::Window {
                            self.window_menu(ui);
                        }
                    });
                }
                if let Some(cmd) = chosen {
                    self.run_command(cmd);
                }
            });
        });

        // 2. Bottom Status Bar
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let jobs = self.queue_mgr.list_jobs();
                let completed_count = jobs.iter().filter(|j| j.status == encodercraft_core::JobStatus::Completed).count();
                let watch_count = self.watch_state.configs.iter().filter(|c| c.enabled).count();

                ui.label(
                    RichText::new(format!("Queue: {} jobs ({} finished) · Watch Folders: {} active", jobs.len(), completed_count, watch_count))
                        .small()
                        .color(self.tokens.text_dim),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Pure Rust · Memory Safe · Parity Ready").small().color(self.tokens.text_dim));
                });
            });
        });

        // 3. Central Workspace with Side Panels
        let mut inspect_job = None;

        // Drag-and-drop detection for external media files (AME Dual-Zone Parity)
        let dropped_files = ui.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            let pointer_pos = ui.input(|i| i.pointer.hover_pos());
            let is_stitch_zone = pointer_pos.is_some_and(|pos| pos.x > 450.0);

            let paths: Vec<std::path::PathBuf> =
                dropped_files.into_iter().map(|d| d.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect();

            if paths.len() > 1 && is_stitch_zone {
                if let Some(first) = paths.first() {
                    let out = first.with_file_name("stitched_sequence.mp4");
                    let _ = self.queue_mgr.stitch_sources_into_job(paths, out, "Match Source - High Bitrate".into());
                }
            } else {
                for p in paths {
                    self.add_file_job(p);
                }
            }
        }

        // Periodic Watch Folder Ingest Polling
        let now = std::time::Instant::now();
        let should_poll_watch = self.watch_state.auto_encode_enabled
            && !self.watch_state.configs.is_empty()
            && self.last_watch_tick.is_none_or(|last| now.duration_since(last) > std::time::Duration::from_secs(2));

        if should_poll_watch {
            self.last_watch_tick = Some(now);
            trigger_watch_scan(&mut self.watch_state, &self.queue_mgr, &self.preset_registry);
        }

        let is_worker_active = self.queue_mgr.is_worker_running();
        if self.was_worker_running && !is_worker_active {
            // Queue finished!
            if self.prefs_state.prefs.play_completion_sound {
                self.completion_banner_timer = Some(std::time::Instant::now());
            }
        }
        self.was_worker_running = is_worker_active;
        self.queue_state.is_running = is_worker_active && !self.queue_mgr.is_paused();

        let is_hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
        if self.queue_state.is_running || is_worker_active || is_hovering || should_poll_watch {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }

        let selected_job_spec = self
            .queue_state
            .selected_job_id
            .as_ref()
            .and_then(|id| self.queue_mgr.list_jobs().into_iter().find(|j| &j.spec.job_id == id).map(|j| j.spec));

        let output_ready =
            self.queue_state.selected_job_id.as_ref().is_some_and(|id| {
                self.queue_mgr.list_jobs().iter().any(|j| &j.spec.job_id == id && j.status == encodercraft_core::JobStatus::Completed)
            });

        let (dock_width, dock_panels, dock_name) =
            self.workspaces.current().map(|w| (w.dock_width, w.panels.clone(), w.name.clone())).unwrap_or((360.0, Vec::new(), String::new()));
        if !dock_panels.is_empty() {
            egui::Panel::right(format!("right_side_dock_{dock_name}"))
                .resizable(true)
                .default_size(dock_width)
                .min_size(crate::workspace::MIN_DOCK_WIDTH)
                .max_size(crate::workspace::MAX_DOCK_WIDTH)
                .show(ui, |ui| {
                    for (i, panel) in dock_panels.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                        match panel {
                            crate::workspace::PanelId::Preview => self.preview_section(ui, selected_job_spec.as_ref(), output_ready),
                            crate::workspace::PanelId::Encoding => {
                                if let Some(job_id) =
                                    crate::encoding_panel::render_encoding_panel(ui, &mut self.encoding_state, &self.queue_mgr, &self.tokens)
                                {
                                    let _ = self.queue_mgr.cancel_job(&job_id);
                                }
                            }
                            crate::workspace::PanelId::Presets => render_preset_browser(
                                ui,
                                &mut self.preset_state,
                                &self.preset_registry,
                                &self.queue_mgr,
                                &self.queue_state.selected_ids_list(),
                                &self.tokens,
                            ),
                        }
                    }
                });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(t) = self.completion_banner_timer {
                if t.elapsed().as_secs() < 5 {
                    egui::Frame::new().fill(self.tokens.status_completed.gamma_multiply(0.2)).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            icons::icon_label(ui, icons::Icon::Check, 15.0, self.tokens.status_completed);
                            ui.label(
                                RichText::new("All batch transcode queue jobs completed successfully!").color(self.tokens.status_completed).strong(),
                            );
                        });
                    });
                    ui.add_space(2.0);
                } else {
                    self.completion_banner_timer = None;
                }
            }

            if is_hovering {
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(self.tokens.accent_primary.gamma_multiply(0.15))
                        .stroke(egui::Stroke::new(2.0, self.tokens.accent_primary))
                        .corner_radius(6)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                icons::icon_label(ui, icons::Icon::ListView, 20.0, self.tokens.accent_primary);
                                ui.add_space(2.0);
                                ui.heading(RichText::new("Drop as Separate Sources").color(self.tokens.accent_primary));
                                ui.label("Each file queued as an independent job.");
                            });
                        });

                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(168, 85, 247).gamma_multiply(0.15))
                        .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(168, 85, 247)))
                        .corner_radius(6)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                icons::icon_label(ui, icons::Icon::Link, 20.0, egui::Color32::from_rgb(168, 85, 247));
                                ui.add_space(2.0);
                                ui.heading(RichText::new("Drop to Stitch Clips Together").color(egui::Color32::from_rgb(168, 85, 247)));
                                ui.label("Combines files into one continuous sequence.");
                            });
                        });
                });
                ui.add_space(8.0);
            }

            // AME Tab Selector (Queue vs Watch Folders)
            ui.horizontal(|ui| {
                let queue_count = self.queue_mgr.list_jobs().len();
                let watch_count = self.watch_state.configs.len();

                let queue_label = format!("Queue ({queue_count})");
                let is_queue = self.active_tab == MainViewTab::Queue;
                let q_btn = icons::button_with_label(
                    ui,
                    icons::Icon::ListView,
                    &queue_label,
                    is_queue,
                    &self.tokens,
                    "View and manage batch transcode queue",
                );
                if q_btn.clicked() {
                    self.active_tab = MainViewTab::Queue;
                }

                let watch_label = format!("Watch Folders ({watch_count})");
                let is_watch = self.active_tab == MainViewTab::WatchFolders;
                let w_btn = icons::button_with_label(
                    ui,
                    icons::Icon::Folder,
                    &watch_label,
                    is_watch,
                    &self.tokens,
                    "View and configure automated watch folders",
                );
                if w_btn.clicked() {
                    self.active_tab = MainViewTab::WatchFolders;
                }

                let m_btn = icons::button_with_label(
                    ui,
                    icons::Icon::Search,
                    "Media Browser",
                    self.active_tab == MainViewTab::MediaBrowser,
                    &self.tokens,
                    "Browse folders and add media to the queue",
                );
                if m_btn.clicked() {
                    self.active_tab = MainViewTab::MediaBrowser;
                }
            });

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            match self.active_tab {
                MainViewTab::Queue => {
                    egui::Panel::bottom("queue_footer").show(ui, |ui| {
                        let jobs = self.queue_mgr.list_jobs();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::queue_footer::processed_counter(&jobs)).small());
                            ui.separator();
                            ui.checkbox(&mut self.prefs_state.prefs.auto_shutdown, "Auto Computer Shutdown")
                                .on_hover_text("Shut the computer down 60 seconds after the queue finishes; you can cancel the countdown");
                            ui.separator();
                            ui.label(RichText::new("Renderer:").small());
                            egui::ComboBox::from_id_salt("queue_renderer").selected_text(crate::queue_footer::RENDERERS[0]).show_ui(ui, |ui| {
                                for name in crate::queue_footer::RENDERERS {
                                    let _ = ui.selectable_label(true, *name);
                                }
                            });
                        });
                    });
                    render_queue_panel(ui, &mut self.queue_state, &self.queue_mgr, &self.tokens, &mut inspect_job, &mut self.log_state.is_open);
                }
                MainViewTab::WatchFolders => {
                    render_watch_panel(ui, &mut self.watch_state, &self.queue_mgr, &self.preset_registry, &self.tokens);
                }
                MainViewTab::MediaBrowser => {
                    for path in crate::media_browser::render_media_browser(ui, &mut self.media_state, &self.tokens) {
                        self.add_file_job(path);
                    }
                }
            }
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

        // 5. Log Dialog
        render_log_dialog(&ctx, &mut self.log_state, &self.queue_mgr, &self.tokens);

        // 6. Preferences Dialog
        let mut theme_change = None;
        render_prefs_dialog(&ctx, &mut self.prefs_state, &self.tokens, &mut theme_change);
        if self.queue_mgr.parallelism() != self.prefs_state.prefs.max_parallel_jobs {
            let _ = self.queue_mgr.set_parallelism(self.prefs_state.prefs.max_parallel_jobs);
        }
        if let Some(theme) = theme_change {
            self.set_brightness(if theme == "High Contrast" { Brightness::HighContrast } else { Brightness::Dark });
        }

        self.save_settings_if_changed();
        self.drive_auto_shutdown(&ctx);

        // 7. About Dialog
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

    /// Runs the Auto Computer Shutdown countdown once the whole queue has finished.
    fn drive_auto_shutdown(&mut self, ctx: &egui::Context) {
        use crate::queue_footer::{ShutdownAction, queue_finished, shut_down_computer};
        let finished = queue_finished(&self.queue_mgr.list_jobs());
        match self.shutdown_watch.update(self.prefs_state.prefs.auto_shutdown, finished, std::time::Instant::now()) {
            ShutdownAction::Idle => {}
            ShutdownAction::Countdown { remaining_secs } => {
                let mut cancel = false;
                egui::Window::new("Auto Computer Shutdown").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
                    ctx,
                    |ui| {
                        ui.label(format!("The queue has finished. This computer shuts down in {remaining_secs} s."));
                        if ui.button("Cancel Shutdown").clicked() {
                            cancel = true;
                        }
                    },
                );
                if cancel {
                    self.shutdown_watch.cancel();
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            ShutdownAction::ShutDownNow => {
                if let Err(e) = shut_down_computer() {
                    log::error!("automatic shutdown failed: {e}");
                    self.shutdown_error = Some(e);
                }
            }
        }
        if let Some(err) = self.shutdown_error.clone() {
            let mut open = true;
            egui::Window::new("Shutdown failed").open(&mut open).show(ctx, |ui| {
                ui.label(err);
            });
            if !open {
                self.shutdown_error = None;
            }
        }
    }

    /// Ingests a local file path into the queue with Match Source settings (e.g. from File Picker or Drag & Drop).
    pub fn add_file_job(&self, path: std::path::PathBuf) {
        let stream_kind = encodercraft_demux::classify_stream_kind(&path);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("media");

        let (ext, container, preset_name, custom_width, custom_height, bitrate, lufs) = match stream_kind {
            encodercraft_core::MediaStreamKind::AudioOnly => {
                ("wav", encodercraft_core::ContainerType::Wave, "Broadcast Wave BWF (-23 LUFS)".to_string(), None, None, None, Some(-23.0))
            }
            encodercraft_core::MediaStreamKind::Vector2D => {
                ("svg", encodercraft_core::ContainerType::Svg, "Web Vector SVG (Optimized)".to_string(), None, None, None, None)
            }
            encodercraft_core::MediaStreamKind::Document => {
                ("pdf", encodercraft_core::ContainerType::Pdf, "Print-Ready PDF/X-4".to_string(), None, None, None, None)
            }
            encodercraft_core::MediaStreamKind::Spatial3D => {
                ("glb", encodercraft_core::ContainerType::Gltf, "glTF 2.0 Binary (.glb)".to_string(), None, None, None, None)
            }
            encodercraft_core::MediaStreamKind::StillImage => {
                ("png", encodercraft_core::ContainerType::Png, "Still Image PNG (Lossless)".to_string(), None, None, None, None)
            }
            _ => (
                "mp4",
                encodercraft_core::ContainerType::IsoBmffMp4,
                "Match Source - High Bitrate".to_string(),
                Some(1920),
                Some(1080),
                Some(16000),
                Some(-14.0),
            ),
        };

        let output_path = path.with_file_name(format!("{stem}_encoded.{ext}"));

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: format!("job-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)),
            client_info: encodercraft_core::ClientInfo {
                app_name: "Drag & Drop".into(),
                app_version: "0.1.0".into(),
                pid: std::process::id(),
                user_notes: None,
            },
            priority: encodercraft_core::JobPriority::Normal,
            start_policy: encodercraft_core::StartPolicy::AutoStart,
            source: encodercraft_core::SourceDescriptor::File { path, start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![encodercraft_core::OutputTarget {
                target_id: "target-1".into(),
                output_path,
                preset_name,
                container,
                custom_width,
                custom_height,
                custom_frame_rate: None,
                target_bitrate_kbps: bitrate,
                target_lufs: lufs,
                ..Default::default()
            }],
        };
        let _ = self.queue_mgr.submit_job(spec);
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
                ..Default::default()
            }],
        };
        let _ = self.queue_mgr.submit_job(spec);
    }
}
