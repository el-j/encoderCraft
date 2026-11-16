//! Automated directory monitoring and ingest service.

use std::path::Path;

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, SourceDescriptor, StartPolicy};
use encodercraft_presets::{PresetRegistry, evaluate_preset};

use crate::config::{PostProcessAction, WatchFolderConfig};
use crate::settle::probe_file_settled;

/// Watch folder manager scanning directories, probing settled files, and preparing job specs.
pub struct WatchFolderService {
    configs: Vec<WatchFolderConfig>,
    preset_registry: PresetRegistry,
}

impl WatchFolderService {
    /// Creates a new service with the given preset registry.
    pub fn new(preset_registry: PresetRegistry) -> Self {
        Self { configs: Vec::new(), preset_registry }
    }

    /// Registers a watch folder configuration.
    pub fn add_config(&mut self, config: WatchFolderConfig) {
        self.configs.push(config);
    }

    /// Returns a slice of all registered watch folder configurations.
    pub fn configs(&self) -> &[WatchFolderConfig] {
        &self.configs
    }

    /// Scans all active watch folders once and returns any new jobs ready for submission.
    pub fn scan_once(&self) -> Result<Vec<(String, CraftJobSpec)>, EncoderError> {
        let mut ready_jobs = Vec::new();

        for cfg in &self.configs {
            if !cfg.enabled || !cfg.watch_dir.exists() {
                continue;
            }

            let entries = match std::fs::read_dir(&cfg.watch_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    // Probe file settle
                    if let Ok(true) = probe_file_settled(&path, cfg.settle_delay_ms)
                        && let Ok(spec) = self.build_job_for_file(cfg, &path)
                    {
                        ready_jobs.push((cfg.id.clone(), spec));
                    }
                }
            }
        }

        Ok(ready_jobs)
    }

    /// Builds a `CraftJobSpec` for a settled file using the configured presets.
    pub fn build_job_for_file(&self, cfg: &WatchFolderConfig, file_path: &Path) -> Result<CraftJobSpec, EncoderError> {
        let media_info = encodercraft_source::probe_media_info(file_path)?;
        let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");

        let targets = if !cfg.targets.is_empty() {
            let mut list = Vec::with_capacity(cfg.targets.len());
            for (idx, target_cfg) in cfg.targets.iter().enumerate() {
                let preset = self
                    .preset_registry
                    .find_by_name(&target_cfg.preset_name)
                    .ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{}' not found in registry", target_cfg.preset_name)))?;

                let ext = preset.container.file_extension();
                let out_dir = target_cfg.output_dir.as_ref().unwrap_or(&cfg.output_dir);
                let shares_dir = cfg
                    .targets
                    .iter()
                    .enumerate()
                    .any(|(other_idx, other)| other_idx != idx && other.output_dir.as_ref().unwrap_or(&cfg.output_dir) == out_dir);
                let output_filename = if shares_dir { format!("{file_stem}_{idx}.{ext}") } else { format!("{file_stem}.{ext}") };
                let target_path = out_dir.join(output_filename);
                let target = evaluate_preset(preset, &media_info, target_path)?;
                list.push(target);
            }
            list
        } else {
            let preset = self
                .preset_registry
                .find_by_name(&cfg.preset_name)
                .ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{}' not found in registry", cfg.preset_name)))?;
            let ext = preset.container.file_extension();
            let output_filename = format!("{file_stem}.{ext}");
            let target_path = cfg.output_dir.join(output_filename);
            let target = evaluate_preset(preset, &media_info, target_path)?;
            vec![target]
        };

        let job_id = format!("watch-{}-{}", cfg.id, file_stem);

        Ok(CraftJobSpec {
            version: "1.0".into(),
            job_id,
            client_info: ClientInfo {
                app_name: "encoderCraft Watcher".into(),
                app_version: "0.1.0".into(),
                pid: std::process::id(),
                user_notes: Some(format!("Ingested from watch folder: {}", cfg.watch_dir.display())),
            },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: file_path.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
            targets,
        })
    }

    /// Applies the configured post-processing action to a source file after job completion.
    pub fn handle_post_process(&self, cfg_id: &str, file_path: &Path, success: bool) -> Result<(), EncoderError> {
        let cfg = self
            .configs
            .iter()
            .find(|c| c.id == cfg_id)
            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown watch folder config '{cfg_id}'")))?;

        if !file_path.exists() {
            return Ok(());
        }

        match &cfg.post_action {
            PostProcessAction::LeaveInPlace => Ok(()),
            PostProcessAction::DeleteSource => {
                if success {
                    std::fs::remove_file(file_path).map_err(|e| EncoderError::Io(format!("failed to delete source file: {e}")))?;
                }
                Ok(())
            }
            PostProcessAction::MoveToProcessed(dest_dir) => {
                if success {
                    std::fs::create_dir_all(dest_dir).map_err(|e| EncoderError::Io(format!("failed to create processed dir: {e}")))?;
                    let filename = file_path.file_name().unwrap_or_default();
                    let dest_path = dest_dir.join(filename);
                    std::fs::rename(file_path, dest_path).map_err(|e| EncoderError::Io(format!("failed to move source file: {e}")))?;
                }
                Ok(())
            }
            PostProcessAction::MoveToArchive(archive_dir) => {
                if success {
                    let date_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let target_subfolder = archive_dir.join(date_str);
                    std::fs::create_dir_all(&target_subfolder).map_err(|e| EncoderError::Io(format!("failed to create archive subfolder: {e}")))?;
                    let filename = file_path.file_name().unwrap_or_default();
                    let dest_path = target_subfolder.join(filename);
                    if std::fs::rename(file_path, &dest_path).is_err() {
                        std::fs::copy(file_path, &dest_path).map_err(|e| EncoderError::Io(format!("failed to copy to archive: {e}")))?;
                        let _ = std::fs::remove_file(file_path);
                    }
                }
                Ok(())
            }
            PostProcessAction::MoveToErrors(err_dir) => {
                if !success {
                    std::fs::create_dir_all(err_dir).map_err(|e| EncoderError::Io(format!("failed to create error dir: {e}")))?;
                    let filename = file_path.file_name().unwrap_or_default();
                    let dest_path = err_dir.join(filename);
                    std::fs::rename(file_path, dest_path).map_err(|e| EncoderError::Io(format!("failed to move failed file: {e}")))?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_watch_service_scan() {
        let dir = tempdir().unwrap();
        let watch_dir = dir.path().join("watch");
        let out_dir = dir.path().join("output");
        let processed_dir = dir.path().join("processed");

        std::fs::create_dir_all(&watch_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let mut service = WatchFolderService::new(PresetRegistry::default());
        let mut cfg = WatchFolderConfig::new("wf-1", watch_dir.clone(), out_dir, "Match Source - High Bitrate");
        cfg.settle_delay_ms = 100;
        cfg.post_action = PostProcessAction::MoveToProcessed(processed_dir.clone());
        service.add_config(cfg);

        // Place a mock PNG file into the watch directory
        let test_png = watch_dir.join("input.png");
        let mut f = std::fs::File::create(&test_png).unwrap();
        // PNG header
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        f.write_all(&[0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R']).unwrap();
        f.write_all(&16u32.to_be_bytes()).unwrap(); // width
        f.write_all(&16u32.to_be_bytes()).unwrap(); // height
        f.write_all(&[8, 2, 0, 0, 0]).unwrap(); // 8-bit RGB
        f.write_all(&[0, 0, 0, 0]).unwrap(); // CRC
        f.flush().unwrap();
        drop(f);

        let jobs = service.scan_once().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0, "wf-1");

        // Test post-process move
        service.handle_post_process("wf-1", &test_png, true).unwrap();
        assert!(!test_png.exists());
        assert!(processed_dir.join("input.png").exists());
    }

    #[test]
    fn test_watch_service_multi_target_output_routing() {
        let dir = tempdir().unwrap();
        let watch_dir = dir.path().join("watch_multi");
        let out_dir = dir.path().join("output_main");
        let sub_out_dir = dir.path().join("output_sub");

        std::fs::create_dir_all(&watch_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::create_dir_all(&sub_out_dir).unwrap();

        let mut service = WatchFolderService::new(PresetRegistry::default());
        let mut cfg = WatchFolderConfig::new("wf-multi", watch_dir.clone(), out_dir.clone(), "Match Source - High Bitrate");
        cfg.settle_delay_ms = 100;
        cfg.targets.clear();
        cfg.add_target("Match Source - High Bitrate", None);
        cfg.add_target("YouTube 4K Ultra HD", Some(sub_out_dir.clone()));
        service.add_config(cfg.clone());

        // Place a mock PNG file
        let test_png = watch_dir.join("clip.png");
        let mut f = std::fs::File::create(&test_png).unwrap();
        f.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        f.write_all(&[0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R']).unwrap();
        f.write_all(&16u32.to_be_bytes()).unwrap();
        f.write_all(&16u32.to_be_bytes()).unwrap();
        f.write_all(&[8, 2, 0, 0, 0]).unwrap();
        f.write_all(&[0, 0, 0, 0]).unwrap();
        f.flush().unwrap();
        drop(f);

        let jobs = service.scan_once().unwrap();
        assert_eq!(jobs.len(), 1);
        let (_, spec) = &jobs[0];
        assert_eq!(spec.targets.len(), 2);
        assert_eq!(spec.targets[0].output_path, out_dir.join("clip.mp4"));
        assert_eq!(spec.targets[1].output_path, sub_out_dir.join("clip.mp4"));
    }

    #[test]
    fn test_watch_service_archive_action() {
        let dir = tempdir().unwrap();
        let watch_dir = dir.path().join("watch_arch");
        let out_dir = dir.path().join("out");
        let archive_dir = dir.path().join("archive");

        std::fs::create_dir_all(&watch_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let mut service = WatchFolderService::new(PresetRegistry::default());
        let mut cfg = WatchFolderConfig::new("wf-arch", watch_dir.clone(), out_dir, "Match Source - High Bitrate");
        cfg.post_action = PostProcessAction::MoveToArchive(archive_dir.clone());
        service.add_config(cfg);

        let test_file = watch_dir.join("done.mov");
        std::fs::write(&test_file, b"content").unwrap();

        service.handle_post_process("wf-arch", &test_file, true).unwrap();
        assert!(!test_file.exists());

        let date_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let expected_archived = archive_dir.join(date_str).join("done.mov");
        assert!(expected_archived.exists());
    }
}
