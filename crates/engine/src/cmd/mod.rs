//! Command infrastructure: "Everything is a command" (AGENTS.md).
//!
//! Provides a unified interface reachable by GUI, CLI, loopback IPC, and MCP alike.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use encodercraft_core::{EncoderError, SourceDescriptor};
use serde_json::Value;

use crate::queue::QueueManager;

const MAX_APP_DATA_BYTES: u64 = 4 * 1024 * 1024;
static NEXT_JOB_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_WATCH_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Specification and metadata of an executable command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    handler: Box<dyn Fn(&Arc<QueueManager>, Value) -> Result<Value, EncoderError> + Send + Sync>,
}

impl CommandSpec {
    pub fn new<F>(id: &'static str, label: &'static str, description: &'static str, handler: F) -> Self
    where
        F: Fn(&Arc<QueueManager>, Value) -> Result<Value, EncoderError> + Send + Sync + 'static,
    {
        Self { id, label, description, handler: Box::new(handler) }
    }

    pub fn execute(&self, queue: &Arc<QueueManager>, params: Value) -> Result<Value, EncoderError> {
        (self.handler)(queue, params)
    }
}

/// Central registry of commands.
pub struct CommandRegistry {
    commands: HashMap<&'static str, CommandSpec>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    /// Builds the registry with all built-in commands registered.
    pub fn new() -> Self {
        let preset_dir = crate::state::default_config_dir().map(|dir| dir.join("presets"));
        Self::with_preset_registry(Arc::new(Mutex::new(encodercraft_presets::PresetRegistry::new(preset_dir))))
    }

    /// Builds the registry with the preset store shared by the GUI and headless clients.
    pub fn with_preset_registry(presets: Arc<Mutex<encodercraft_presets::PresetRegistry>>) -> Self {
        let mut reg = Self { commands: HashMap::new() };
        reg.register_builtins();
        reg.register_application_commands(presets);
        reg
    }

    /// Registers a command into the registry.
    pub fn register(&mut self, spec: CommandSpec) {
        self.commands.insert(spec.id, spec);
    }

    /// Dispatches a command by ID with JSON parameters.
    pub fn dispatch(&self, queue: &Arc<QueueManager>, cmd_id: &str, params: Value) -> Result<Value, EncoderError> {
        let cmd = self.commands.get(cmd_id).ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown command '{cmd_id}'")))?;
        cmd.execute(queue, params)
    }

    /// Lists all registered command specifications.
    pub fn list(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let mut commands: Vec<_> = self.commands.values().map(|command| (command.id, command.label, command.description)).collect();
        commands.sort_unstable_by_key(|(id, _, _)| *id);
        commands
    }

    /// Returns whether a command ID is registered.
    pub fn contains(&self, command_id: &str) -> bool {
        self.commands.contains_key(command_id)
    }

    fn register_builtins(&mut self) {
        // queue.add
        self.register(CommandSpec::new("queue.add", "Add Job", "Submits a new encoding job to the queue", |q, params| {
            let spec: encodercraft_core::CraftJobSpec =
                serde_json::from_value(params).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse job spec: {e}")))?;
            let job_id = q.submit_job(spec)?;
            Ok(serde_json::json!({ "jobId": job_id }))
        }));

        self.register(CommandSpec::new(
            "queue.add_with_approvals",
            "Add Approved Job",
            "Submits a job with explicit approval for exact lossy conversion pairs",
            |q, params| {
                let spec: encodercraft_core::CraftJobSpec = serde_json::from_value(
                    params.get("spec").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'spec' parameter".into()))?,
                )
                .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse job spec: {e}")))?;
                let approvals: Vec<crate::conversion::LossyConversionKey> =
                    serde_json::from_value(params.get("approvals").cloned().unwrap_or(Value::Array(Vec::new())))
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid approvals: {e}")))?;
                let job_id = q.submit_job_with_approvals(spec, &approvals)?;
                Ok(serde_json::json!({ "jobId": job_id }))
            },
        ));

        self.register(CommandSpec::new(
            "queue.update_with_approvals",
            "Update Approved Job",
            "Updates a job with explicit approval for exact lossy conversion pairs",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let spec: encodercraft_core::CraftJobSpec = serde_json::from_value(
                    params.get("spec").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'spec' parameter".into()))?,
                )
                .map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse job spec: {e}")))?;
                let approvals: Vec<crate::conversion::LossyConversionKey> =
                    serde_json::from_value(params.get("approvals").cloned().unwrap_or(Value::Array(Vec::new())))
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid approvals: {e}")))?;
                q.update_job_spec_with_approvals(job_id, spec, &approvals)?;
                Ok(serde_json::json!({ "success": true, "jobId": job_id }))
            },
        ));

        self.register(CommandSpec::new(
            "queue.save_conversion_decision",
            "Save Conversion Decision",
            "Persists the user's decision for an exact lossy conversion pair",
            |q, params| {
                let key: crate::conversion::LossyConversionKey =
                    serde_json::from_value(params.get("key").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'key' parameter".into()))?)
                        .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid conversion key: {e}")))?;
                let allowed = params
                    .get("allowed")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'allowed' parameter".into()))?;
                q.save_conversion_decision(key, allowed)?;
                Ok(serde_json::json!({ "success": true }))
            },
        ));

        // queue.remove
        self.register(CommandSpec::new("queue.remove", "Remove Job", "Removes a job from the queue by ID", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.remove_job(job_id)?;
            Ok(serde_json::json!({ "success": true }))
        }));

        // queue.start
        self.register(CommandSpec::new("queue.start", "Start Job", "Starts or resumes a queued job", |q, params| {
            if let Some(job_id) = params.get("jobId").and_then(Value::as_str) {
                q.start_job(job_id)?;
                Ok(serde_json::json!({ "jobId": job_id, "status": "started" }))
            } else {
                let running = q.start_queue();
                Ok(serde_json::json!({ "running": running }))
            }
        }));

        // queue.pause
        self.register(CommandSpec::new("queue.pause", "Pause Queue or Job", "Pauses the queue worker or a selected job", |q, params| {
            if let Some(job_id) = params.get("jobId").and_then(Value::as_str) {
                q.pause_job(job_id)?;
                Ok(serde_json::json!({ "jobId": job_id, "status": "paused" }))
            } else {
                q.pause_worker();
                Ok(serde_json::json!({ "success": true, "status": "paused" }))
            }
        }));

        self.register(CommandSpec::new("queue.pause_worker", "Pause Queue", "Pauses the queue worker", |q, _| {
            q.pause_worker();
            Ok(serde_json::json!({ "success": true }))
        }));

        self.register(CommandSpec::new(
            "queue.set_parallelism",
            "Set Queue Parallelism",
            "Sets the maximum number of concurrent jobs",
            |q, params| {
                let workers = params
                    .get("workers")
                    .and_then(Value::as_u64)
                    .and_then(|workers| usize::try_from(workers).ok())
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'workers' parameter".into()))?;
                q.set_parallelism(workers)?;
                Ok(serde_json::json!({ "workers": q.parallelism() }))
            },
        ));

        // queue.stop
        self.register(CommandSpec::new("queue.stop", "Stop/Cancel Job", "Cancels an active or queued job", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.cancel_job(job_id)?;
            Ok(serde_json::json!({ "jobId": job_id, "status": "cancelled" }))
        }));

        // queue.duplicate
        self.register(CommandSpec::new("queue.duplicate", "Duplicate Job", "Duplicates an existing job", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            let new_id = q.duplicate_job(job_id)?;
            Ok(serde_json::json!({ "jobId": new_id }))
        }));

        // queue.reorder
        self.register(CommandSpec::new(
            "queue.reorder",
            "Reorder Queue",
            "Reorders jobs in the queue matching the provided ID array",
            |q, params| {
                let ids: Vec<String> = serde_json::from_value(params.get("order").cloned().unwrap_or(Value::Array(Vec::new())))
                    .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid order list: {e}")))?;
                q.reorder_jobs(&ids)?;
                Ok(serde_json::json!({ "success": true }))
            },
        ));

        // queue.list
        self.register(CommandSpec::new("queue.list", "List Queue", "Returns a snapshot of all queued jobs", |q, _| {
            let jobs = q.list_jobs();
            let json_val = serde_json::to_value(&jobs).map_err(|e| EncoderError::Internal(format!("failed to serialize jobs: {e}")))?;
            Ok(json_val)
        }));

        // queue.move_up
        self.register(CommandSpec::new("queue.move_up", "Move Job Up", "Moves a job up in queue priority", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.move_job_up(job_id)?;
            Ok(serde_json::json!({ "success": true, "jobId": job_id }))
        }));

        // queue.move_down
        self.register(CommandSpec::new("queue.move_down", "Move Job Down", "Moves a job down in queue priority", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.move_job_down(job_id)?;
            Ok(serde_json::json!({ "success": true, "jobId": job_id }))
        }));

        // queue.reset
        self.register(CommandSpec::new("queue.reset", "Reset Job Status", "Resets finished or failed job back to ready", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.reset_job_status(job_id)?;
            Ok(serde_json::json!({ "success": true, "jobId": job_id }))
        }));

        // queue.clear_finished
        self.register(CommandSpec::new("queue.clear_finished", "Clear Finished", "Removes completed jobs from the queue", |q, _| {
            let removed = q.clear_completed_jobs()?;
            Ok(serde_json::json!({ "removedCount": removed }))
        }));

        // queue.stitch
        self.register(CommandSpec::new("queue.stitch", "Stitch Clips", "Combines multiple clips into one transcode job", |q, params| {
            let paths: Vec<std::path::PathBuf> = serde_json::from_value(params.get("paths").cloned().unwrap_or(Value::Array(Vec::new())))
                .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid paths: {e}")))?;
            let output_path = params
                .get("outputPath")
                .and_then(Value::as_str)
                .map(std::path::PathBuf::from)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'outputPath'".into()))?;
            let preset_name = params.get("presetName").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate").to_string();

            let job_id = q.stitch_sources_into_job(paths, output_path, preset_name)?;
            Ok(serde_json::json!({ "jobId": job_id }))
        }));

        self.register(CommandSpec::new(
            "queue.prepare_stitch_sources",
            "Prepare Stitched Sources",
            "Builds a stitched job specification from source paths without submitting it",
            |_, params| {
                let paths: Vec<std::path::PathBuf> = serde_json::from_value(params.get("paths").cloned().unwrap_or(Value::Array(Vec::new())))
                    .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid paths: {e}")))?;
                let output_path = params
                    .get("outputPath")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'outputPath'".into()))?;
                let preset_name = params.get("presetName").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate").to_string();
                let spec = QueueManager::build_stitched_job_spec(paths, output_path, preset_name)?;
                serde_json::to_value(spec).map_err(|e| EncoderError::Internal(format!("failed to serialize stitched job: {e}")))
            },
        ));

        // queue.stitch_selected
        self.register(CommandSpec::new(
            "queue.stitch_selected",
            "Stitch Selected Jobs",
            "Combines selected queue jobs into one stitched job",
            |q, params| {
                let job_ids: Vec<String> = serde_json::from_value(params.get("jobIds").cloned().unwrap_or(Value::Array(Vec::new())))
                    .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid jobIds: {e}")))?;
                let output_path = params
                    .get("outputPath")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'outputPath'".into()))?;
                let preset_name = params.get("presetName").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate").to_string();

                let job_id = q.stitch_selected_jobs(&job_ids, output_path, preset_name)?;
                Ok(serde_json::json!({ "jobId": job_id }))
            },
        ));

        // queue.set_output_dir
        self.register(CommandSpec::new(
            "queue.set_output_dir",
            "Set Jobs Output Directory",
            "Sets destination output folder across selected jobs",
            |q, params| {
                let job_ids: Vec<String> = serde_json::from_value(params.get("jobIds").cloned().unwrap_or(Value::Array(Vec::new())))
                    .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid jobIds: {e}")))?;
                let output_dir = params
                    .get("outputDir")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'outputDir'".into()))?;

                let count = q.set_jobs_output_dir(&job_ids, output_dir)?;
                Ok(serde_json::json!({ "modifiedCount": count }))
            },
        ));

        self.register(CommandSpec::new("queue.add_output_target", "Add Output Target", "Adds an output target to a queued job", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            let target: encodercraft_core::OutputTarget = serde_json::from_value(
                params.get("target").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'target' parameter".into()))?,
            )
            .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid output target: {e}")))?;
            q.add_output_target(job_id, target)?;
            Ok(serde_json::json!({ "success": true, "jobId": job_id }))
        }));

        self.register(CommandSpec::new(
            "queue.prepare_add_output_target",
            "Prepare Output Target",
            "Builds an updated job specification with an additional output target without applying it",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let mut spec = q
                    .list_jobs()
                    .into_iter()
                    .find(|job| job.spec.job_id == job_id)
                    .map(|job| job.spec)
                    .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;
                let target: encodercraft_core::OutputTarget = match params.get("target") {
                    Some(target) => serde_json::from_value(target.clone())
                        .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid output target: {error}")))?,
                    None => {
                        let mut index =
                            spec.targets.len().checked_add(1).ok_or_else(|| EncoderError::InvalidJobSpec("output target count overflow".into()))?;
                        let target_id = loop {
                            let candidate = format!("target-{index}");
                            if !spec.targets.iter().any(|target| target.target_id == candidate) {
                                break candidate;
                            }
                            index = index.checked_add(1).ok_or_else(|| EncoderError::InvalidJobSpec("output target ID overflow".into()))?;
                        };
                        encodercraft_core::OutputTarget {
                            target_id,
                            output_path: std::path::PathBuf::from(format!("output_{index}.mp4")),
                            preset_name: "Match Source - High Bitrate".into(),
                            container: encodercraft_core::ContainerType::IsoBmffMp4,
                            ..Default::default()
                        }
                    }
                };
                spec.targets.push(target);
                spec.validate()?;
                serde_json::to_value(spec).map_err(|e| EncoderError::Internal(format!("failed to serialize updated job: {e}")))
            },
        ));

        self.register(CommandSpec::new(
            "queue.remove_output_target",
            "Remove Output Target",
            "Removes an output target from a queued job",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let target_idx = params
                    .get("targetIndex")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'targetIndex' parameter".into()))?;
                q.remove_output_target(job_id, target_idx)?;
                Ok(serde_json::json!({ "success": true, "jobId": job_id }))
            },
        ));

        self.register(CommandSpec::new(
            "queue.duplicate_output_target",
            "Duplicate Output Target",
            "Duplicates an output target on a queued job",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let target_idx = params
                    .get("targetIndex")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'targetIndex' parameter".into()))?;
                q.duplicate_output_target(job_id, target_idx)?;
                Ok(serde_json::json!({ "success": true, "jobId": job_id }))
            },
        ));

        self.register(CommandSpec::new(
            "queue.update_output_target",
            "Update Output Target",
            "Updates an output target on a queued job",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let target_idx = params
                    .get("targetIndex")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'targetIndex' parameter".into()))?;
                let target: encodercraft_core::OutputTarget = serde_json::from_value(
                    params.get("target").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'target' parameter".into()))?,
                )
                .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid output target: {e}")))?;
                q.update_output_target(job_id, target_idx, target)?;
                Ok(serde_json::json!({ "success": true, "jobId": job_id }))
            },
        ));

        self.register(CommandSpec::new(
            "queue.duplicate_spec",
            "Prepare Job Duplicate",
            "Builds a duplicate job specification without submitting it",
            |q, params| {
                let job_id =
                    params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
                let spec = q.duplicate_job_spec(job_id)?;
                serde_json::to_value(spec).map_err(|e| EncoderError::Internal(format!("failed to serialize duplicated job: {e}")))
            },
        ));

        self.register(CommandSpec::new(
            "queue.prepare_stitch",
            "Prepare Stitched Job",
            "Builds a stitched job specification from selected jobs without submitting it",
            |q, params| {
                let job_ids: Vec<String> = serde_json::from_value(params.get("jobIds").cloned().unwrap_or(Value::Array(Vec::new())))
                    .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid jobIds: {e}")))?;
                let output_path = params
                    .get("outputPath")
                    .and_then(Value::as_str)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'outputPath'".into()))?;
                let preset_name = params.get("presetName").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate").to_string();
                let spec = q.build_stitched_selected_job_spec(&job_ids, output_path, preset_name)?;
                serde_json::to_value(spec).map_err(|e| EncoderError::Internal(format!("failed to serialize stitched job: {e}")))
            },
        ));

        // queue.log
        self.register(CommandSpec::new("queue.log", "Get Transcode Log", "Returns all historical transcode log entries", |q, _| {
            let entries = q.list_log_entries();
            let json_val = serde_json::to_value(&entries).map_err(|e| EncoderError::Internal(format!("failed to serialize log: {e}")))?;
            Ok(json_val)
        }));

        // queue.clear_log
        self.register(CommandSpec::new("queue.clear_log", "Clear Transcode Log", "Clears transcode activity log", |q, _| {
            q.clear_log_entries()?;
            Ok(serde_json::json!({ "success": true }))
        }));

        self.register(CommandSpec::new("queue.set_trim_range", "Set Job Trim Range", "Updates a queued job's in/out frame range", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            let in_frame = match params.get("inFrame") {
                None | Some(Value::Null) => None,
                Some(value) => Some(value.as_u64().ok_or_else(|| EncoderError::InvalidJobSpec("invalid 'inFrame' parameter".into()))?),
            };
            let out_frame = match params.get("outFrame") {
                None | Some(Value::Null) => None,
                Some(value) => Some(value.as_u64().ok_or_else(|| EncoderError::InvalidJobSpec("invalid 'outFrame' parameter".into()))?),
            };
            let fps =
                params.get("fps").and_then(Value::as_u64).ok_or_else(|| EncoderError::InvalidJobSpec("missing or invalid 'fps' parameter".into()))?;
            q.set_job_trim_range(job_id, in_frame, out_frame, fps)?;
            Ok(serde_json::json!({ "success": true, "jobId": job_id }))
        }));

        // queue.is_running
        self.register(CommandSpec::new("queue.is_running", "Is Queue Running", "Checks if background worker is active", |q, _| {
            let running = q.is_worker_running();
            Ok(serde_json::json!({ "isRunning": running }))
        }));

        // queue.stop_worker
        self.register(CommandSpec::new("queue.stop_worker", "Stop Worker", "Stops background queue worker", |q, _| {
            q.stop_worker();
            Ok(serde_json::json!({ "success": true }))
        }));
    }

    fn register_application_commands(&mut self, presets: Arc<Mutex<encodercraft_presets::PresetRegistry>>) {
        self.register(CommandSpec::new(
            "queue.prepare_add_file",
            "Prepare Media Job",
            "Builds a job specification for a local media file without submitting it",
            |_, params| {
                let path = required_path(&params, "path")?;
                let stem = path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("media");
                let (extension, container, preset_name, width, height, bitrate, lufs) = match encodercraft_demux::classify_stream_kind(&path) {
                    encodercraft_core::MediaStreamKind::AudioOnly => {
                        ("wav", encodercraft_core::ContainerType::Wave, "Broadcast Wave BWF (-23 LUFS)", None, None, None, Some(-23.0))
                    }
                    encodercraft_core::MediaStreamKind::Vector2D => {
                        ("svg", encodercraft_core::ContainerType::Svg, "Web Vector SVG (Optimized)", None, None, None, None)
                    }
                    encodercraft_core::MediaStreamKind::Document => {
                        ("pdf", encodercraft_core::ContainerType::Pdf, "Print-Ready PDF/X-4", None, None, None, None)
                    }
                    encodercraft_core::MediaStreamKind::Spatial3D => {
                        ("glb", encodercraft_core::ContainerType::Gltf, "glTF 2.0 Binary (.glb)", None, None, None, None)
                    }
                    encodercraft_core::MediaStreamKind::StillImage => {
                        ("png", encodercraft_core::ContainerType::Png, "Still Image PNG (Lossless)", None, None, None, None)
                    }
                    _ => (
                        "mp4",
                        encodercraft_core::ContainerType::IsoBmffMp4,
                        "Match Source - High Bitrate",
                        Some(1920),
                        Some(1080),
                        Some(16000),
                        Some(-14.0),
                    ),
                };
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| EncoderError::Internal(format!("system clock is before UNIX epoch: {error}")))?
                    .as_millis();
                let id = NEXT_JOB_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let client_app = params.get("clientApp").and_then(Value::as_str).unwrap_or("encoderCraft");
                let spec = encodercraft_core::CraftJobSpec {
                    version: "1.0".into(),
                    job_id: format!("job-{now}-{id}"),
                    client_info: encodercraft_core::ClientInfo {
                        app_name: client_app.to_owned(),
                        app_version: env!("CARGO_PKG_VERSION").into(),
                        pid: std::process::id(),
                        user_notes: None,
                    },
                    priority: encodercraft_core::JobPriority::Normal,
                    start_policy: encodercraft_core::StartPolicy::AutoStart,
                    source: SourceDescriptor::File { path: path.clone(), start_tick: None, duration_ticks: None, timebase: None },
                    targets: vec![encodercraft_core::OutputTarget {
                        target_id: "target-1".into(),
                        output_path: path.with_file_name(format!("{stem}_encoded.{extension}")),
                        preset_name: preset_name.into(),
                        container,
                        custom_width: width,
                        custom_height: height,
                        target_bitrate_kbps: bitrate,
                        target_lufs: lufs,
                        ..Default::default()
                    }],
                };
                serde_json::to_value(spec).map_err(|error| EncoderError::Internal(format!("failed to serialize prepared job: {error}")))
            },
        ));

        self.register(CommandSpec::new("queue.select_all", "Select All Jobs", "Returns all queue job IDs for client-side selection", |q, _| {
            let job_ids: Vec<_> = q.list_jobs().into_iter().map(|job| job.spec.job_id).collect();
            Ok(serde_json::json!({ "jobIds": job_ids }))
        }));

        for (command_id, output) in
            [("queue.reveal_output", true), ("queue.copy_output_path", true), ("queue.reveal_source", false), ("queue.copy_source_path", false)]
        {
            self.register(CommandSpec::new(
                command_id,
                "Get Queue Item Path",
                "Returns a source or output path for the requested queue job",
                move |q, params| {
                    let job_id = required_string(&params, "jobId")?;
                    let job = q
                        .list_jobs()
                        .into_iter()
                        .find(|job| job.spec.job_id == job_id)
                        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;
                    let path = if output {
                        let index = params.get("targetIndex").and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok()).unwrap_or(0);
                        job.spec.targets.get(index).map(|target| target.output_path.clone())
                    } else {
                        match &job.spec.source {
                            SourceDescriptor::File { path, .. } => Some(path.clone()),
                            SourceDescriptor::StitchedClips { clips } => clips.first().map(|clip| clip.path.clone()),
                            SourceDescriptor::ProjectReference { project_path, .. } => Some(project_path.clone()),
                            SourceDescriptor::Pipe { .. } => None,
                        }
                    }
                    .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' has no requested path")))?;
                    Ok(serde_json::json!({ "path": path }))
                },
            ));
        }

        self.register(CommandSpec::new(
            "queue.export_settings",
            "Export Job Settings",
            "Returns the selected job specification as JSON",
            |q, params| {
                let job_id = required_string(&params, "jobId")?;
                let spec = q
                    .list_jobs()
                    .into_iter()
                    .find(|job| job.spec.job_id == job_id)
                    .map(|job| job.spec)
                    .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;
                serde_json::to_value(spec).map_err(|error| EncoderError::Internal(format!("failed to serialize job settings: {error}")))
            },
        ));

        self.register(CommandSpec::new("preferences.get", "Get Preferences", "Returns the application preferences", |q, _| {
            serde_json::to_value(q.app_preferences()?).map_err(|error| EncoderError::Internal(format!("failed to serialize preferences: {error}")))
        }));

        self.register(CommandSpec::new("preferences.update", "Update Preferences", "Validates and persists application preferences", |q, params| {
            let preferences: crate::state::AppPreferences = serde_json::from_value(
                params.get("preferences").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'preferences' parameter".into()))?,
            )
            .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid preferences: {error}")))?;
            q.save_app_preferences(preferences.clone())?;
            q.set_parallelism(preferences.max_parallel_jobs)?;
            serde_json::to_value(q.app_preferences()?)
                .map_err(|error| EncoderError::Internal(format!("failed to serialize saved preferences: {error}")))
        }));

        self.register(CommandSpec::new("preferences.set_theme", "Set Theme", "Persists the selected application theme", |q, params| {
            let theme = required_string(&params, "theme")?;
            if !matches!(theme.as_str(), "Dark" | "Medium Dark" | "Light" | "High Contrast") {
                return Err(EncoderError::InvalidJobSpec(format!("unsupported theme '{theme}'")));
            }
            let mut preferences = q.app_preferences()?;
            preferences.theme = theme.clone();
            q.save_app_preferences(preferences)?;
            Ok(serde_json::json!({ "theme": theme }))
        }));

        self.register(CommandSpec::new(
            "media.open_folder",
            "Open Media Folder",
            "Returns the selected folder for client-side navigation",
            |_, params| {
                let path = required_path(&params, "path")?;
                Ok(serde_json::json!({ "path": path }))
            },
        ));

        self.register(CommandSpec::new("media.list_favorites", "List Favorite Folders", "Returns persisted favorite folders", |q, _| {
            let favorites: Vec<PathBuf> = read_json_file(&q.config_file_path("favorites.json")?)?;
            serde_json::to_value(favorites).map_err(|error| EncoderError::Internal(format!("failed to serialize favorites: {error}")))
        }));

        self.register(CommandSpec::new(
            "media.toggle_favorite",
            "Toggle Favorite Folder",
            "Persists a folder favorite and returns the complete favorites list",
            |q, params| {
                let path = required_path(&params, "path")?;
                let favorites_path = q.config_file_path("favorites.json")?;
                let mut favorites: Vec<PathBuf> = read_json_file(&favorites_path)?;
                if let Some(index) = favorites.iter().position(|favorite| favorite == &path) {
                    favorites.remove(index);
                } else {
                    favorites.push(path);
                }
                write_json_file_atomic(&favorites_path, &favorites)?;
                serde_json::to_value(favorites).map_err(|error| EncoderError::Internal(format!("failed to serialize favorites: {error}")))
            },
        ));

        self.register(CommandSpec::new("media.set_favorites", "Set Favorite Folders", "Persists the supplied folder favorites", |q, params| {
            let favorites: Vec<PathBuf> = serde_json::from_value(
                params.get("favorites").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'favorites' parameter".into()))?,
            )
            .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid favorites: {error}")))?;
            let mut unique = Vec::new();
            for favorite in favorites {
                if !unique.contains(&favorite) {
                    unique.push(favorite);
                }
            }
            let favorites_path = q.config_file_path("favorites.json")?;
            write_json_file_atomic(&favorites_path, &unique)?;
            serde_json::to_value(unique).map_err(|error| EncoderError::Internal(format!("failed to serialize favorites: {error}")))
        }));

        self.register(CommandSpec::new(
            "watch.add_folder",
            "Add Watch Folder",
            "Persists a watch-folder configuration and returns it",
            |q, params| {
                let watch_dir = required_path(&params, "watchDir")?;
                let output_dir = params.get("outputDir").and_then(Value::as_str).map(PathBuf::from).unwrap_or_else(|| watch_dir.join("_encoded"));
                let preset_name = params.get("presetName").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate");
                let path = q.config_file_path("watch_folders.json")?;
                let mut file: WatchFile = read_json_file(&path)?;
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| EncoderError::Internal(format!("system clock is before UNIX epoch: {error}")))?
                    .as_millis();
                let sequence = NEXT_WATCH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let id = format!("watch-{timestamp}-{sequence}");
                let config = encodercraft_watch::WatchFolderConfig::new(id, watch_dir, output_dir, preset_name);
                file.folders.push(config.clone());
                write_json_file_atomic(&path, &file)?;
                serde_json::to_value(config).map_err(|error| EncoderError::Internal(format!("failed to serialize watch folder: {error}")))
            },
        ));

        self.register(CommandSpec::new("watch.list_folders", "List Watch Folders", "Returns persisted watch-folder configurations", |q, _| {
            let file: WatchFile = read_json_file(&q.config_file_path("watch_folders.json")?)?;
            serde_json::to_value(file.folders).map_err(|error| EncoderError::Internal(format!("failed to serialize watch folders: {error}")))
        }));

        self.register(CommandSpec::new(
            "watch.set_folders",
            "Set Watch Folders",
            "Persists the complete watch-folder configuration list",
            |q, params| {
                let folders: Vec<encodercraft_watch::WatchFolderConfig> = serde_json::from_value(
                    params.get("folders").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'folders' parameter".into()))?,
                )
                .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid watch folders: {error}")))?;
                let mut seen = std::collections::HashSet::new();
                if folders.iter().any(|folder| folder.id.is_empty() || !seen.insert(folder.id.clone())) {
                    return Err(EncoderError::InvalidJobSpec("watch folder IDs must be non-empty and unique".into()));
                }
                let file = WatchFile { version: 1, folders };
                let path = q.config_file_path("watch_folders.json")?;
                write_json_file_atomic(&path, &file)?;
                Ok(serde_json::json!({ "count": file.folders.len() }))
            },
        ));

        self.register(CommandSpec::new("preset.list", "List Presets", "Returns all built-in and custom presets", {
            let presets = Arc::clone(&presets);
            move |_, _| {
                let registry = presets.lock().unwrap_or_else(PoisonError::into_inner);
                serde_json::to_value(registry.list()).map_err(|error| EncoderError::Internal(format!("failed to serialize presets: {error}")))
            }
        }));

        self.register(CommandSpec::new("preset.create", "Create Preset", "Validates and saves a custom encoding preset", {
            let presets = Arc::clone(&presets);
            move |_, params| {
                let mut preset: encodercraft_presets::EncodingPreset = serde_json::from_value(
                    params.get("preset").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'preset' parameter".into()))?,
                )
                .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid preset: {error}")))?;
                let name = preset.name.trim().to_owned();
                if name.is_empty() {
                    return Err(EncoderError::InvalidJobSpec("preset name cannot be empty".into()));
                }
                preset.name.clone_from(&name);
                if preset.video.is_none() && preset.audio.is_none() {
                    return Err(EncoderError::InvalidJobSpec("preset must configure video, audio, or both".into()));
                }
                let mut registry = presets.lock().unwrap_or_else(PoisonError::into_inner);
                if registry.find_by_name(&name).is_some_and(|existing| registry.is_factory(&existing.name)) {
                    return Err(EncoderError::InvalidJobSpec(format!("'{name}' is a built-in preset and cannot be overwritten")));
                }
                registry.save_custom_preset(preset.clone())?;
                serde_json::to_value(preset).map_err(|error| EncoderError::Internal(format!("failed to serialize saved preset: {error}")))
            }
        }));

        self.register(CommandSpec::new("preset.delete", "Delete Preset", "Deletes a user-created preset", {
            let presets = Arc::clone(&presets);
            move |_, params| {
                let name = required_string(&params, "name")?;
                presets.lock().unwrap_or_else(PoisonError::into_inner).delete_custom_preset(&name)?;
                Ok(serde_json::json!({ "success": true, "name": name }))
            }
        }));

        self.register(CommandSpec::new("preset.import", "Import Presets", "Imports presets from a JSON preset bundle", {
            let presets = Arc::clone(&presets);
            move |_, params| {
                let path = required_path(&params, "path")?;
                let metadata = std::fs::metadata(&path).map_err(|error| EncoderError::Io(format!("failed to inspect preset bundle: {error}")))?;
                if metadata.len() > MAX_APP_DATA_BYTES {
                    return Err(EncoderError::Io(format!("preset bundle exceeds the {MAX_APP_DATA_BYTES}-byte limit")));
                }
                let text = std::fs::read_to_string(&path).map_err(|error| EncoderError::Io(format!("failed to read preset bundle: {error}")))?;
                let imported = encodercraft_presets::catalog::import_json(&text)?;
                let count = presets.lock().unwrap_or_else(PoisonError::into_inner).import_presets(imported)?;
                Ok(serde_json::json!({ "importedCount": count, "path": path }))
            }
        }));

        self.register(CommandSpec::new("preset.export", "Export Presets", "Exports custom presets to a JSON preset bundle", {
            let presets = Arc::clone(&presets);
            move |_, params| {
                let path = required_path(&params, "path")?;
                let custom = {
                    let registry = presets.lock().unwrap_or_else(PoisonError::into_inner);
                    registry.list().iter().filter(|preset| !registry.is_factory(&preset.name)).cloned().collect::<Vec<_>>()
                };
                let text = encodercraft_presets::catalog::export_json(&custom)?;
                if text.len() as u64 > MAX_APP_DATA_BYTES {
                    return Err(EncoderError::Io(format!("preset bundle exceeds the {MAX_APP_DATA_BYTES}-byte limit")));
                }
                std::fs::write(&path, text).map_err(|error| EncoderError::Io(format!("failed to write preset bundle: {error}")))?;
                Ok(serde_json::json!({ "exportedCount": custom.len(), "path": path }))
            }
        }));

        for (command_id, forward, toggle) in
            [("workspace.focus_next", true, false), ("workspace.focus_previous", false, false), ("workspace.toggle_panel", false, true)]
        {
            self.register(CommandSpec::new(
                command_id,
                "Change Workspace Panel State",
                "Computes a focused or visible panel state transition",
                move |_, params| {
                    let panels: Vec<String> = serde_json::from_value(
                        params
                            .get("visiblePanels")
                            .cloned()
                            .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'visiblePanels' parameter".into()))?,
                    )
                    .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid visiblePanels: {error}")))?;
                    let current = required_string(&params, "currentPanel")?;
                    validate_workspace_panels(&panels)?;
                    validate_workspace_panel(&current)?;
                    if toggle {
                        let mut updated = panels;
                        let visible = if let Some(index) = updated.iter().position(|panel| panel == &current) {
                            updated.remove(index);
                            false
                        } else {
                            updated.push(current.clone());
                            true
                        };
                        Ok(serde_json::json!({ "visiblePanels": updated, "visible": visible, "focusedPanel": current }))
                    } else {
                        if panels.is_empty() {
                            return Err(EncoderError::InvalidJobSpec("cannot focus a panel in an empty workspace".into()));
                        }
                        let index = panels.iter().position(|panel| panel == &current);
                        let next = match (index, forward) {
                            (Some(index), true) => (index + 1) % panels.len(),
                            (Some(index), false) => (index + panels.len() - 1) % panels.len(),
                            (None, true) => 0,
                            (None, false) => panels.len() - 1,
                        };
                        let panel = panels.get(next).ok_or_else(|| EncoderError::Internal("workspace panel selection was out of bounds".into()))?;
                        Ok(serde_json::json!({ "panel": panel }))
                    }
                },
            ));
        }

        self.register(CommandSpec::new("workspace.get", "Get Workspaces", "Returns the persisted workspace layout document", |q, _| {
            let value: Value = read_json_file(&q.config_file_path("workspaces.json")?)?;
            Ok(value)
        }));

        self.register(CommandSpec::new(
            "workspace.save",
            "Save Workspaces",
            "Persists a workspace layout document supplied by the UI client",
            |q, params| {
                let value = workspace_document(&params)?;
                write_json_file_atomic(&q.config_file_path("workspaces.json")?, &value)?;
                Ok(serde_json::json!({ "success": true }))
            },
        ));

        for (command_id, action) in [
            ("workspace.activate", "activate"),
            ("workspace.save_as", "save_as"),
            ("workspace.rename", "rename"),
            ("workspace.reset", "reset"),
            ("workspace.delete", "delete"),
        ] {
            self.register(CommandSpec::new(
                command_id,
                "Manage Workspace",
                "Applies a validated workspace transition and persists the resulting layout document",
                move |q, params| apply_workspace_operation(q, params, action),
            ));
        }

        for (command_id, is_write) in [("keyboard_shortcuts.get", false), ("keyboard_shortcuts.save", true)] {
            self.register(CommandSpec::new(
                command_id,
                "Keyboard Shortcut Settings",
                "Reads or persists keyboard shortcut settings",
                move |q, params| {
                    let path = q.config_file_path("keyboard_shortcuts.json")?;
                    if is_write {
                        let value =
                            params.get("shortcuts").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'shortcuts' parameter".into()))?;
                        if !value.is_object() {
                            return Err(EncoderError::InvalidJobSpec("shortcuts must be a JSON object".into()));
                        }
                        write_json_file_atomic(&path, &value)?;
                        Ok(serde_json::json!({ "success": true }))
                    } else {
                        read_json_file(&path)
                    }
                },
            ));
        }

        self.register(CommandSpec::new(
            "workspace.set_visibility",
            "Set Workspace Panel Visibility",
            "Computes a validated panel visibility transition",
            |_, params| {
                let panels: Vec<String> = serde_json::from_value(
                    params.get("visiblePanels").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'visiblePanels' parameter".into()))?,
                )
                .map_err(|error| EncoderError::InvalidJobSpec(format!("invalid visiblePanels: {error}")))?;
                let panel = required_string(&params, "panel")?;
                validate_workspace_panels(&panels)?;
                validate_workspace_panel(&panel)?;
                let visible = params
                    .get("visible")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'visible' parameter".into()))?;
                let mut updated = panels;
                if visible {
                    if !updated.contains(&panel) {
                        updated.push(panel.clone());
                    }
                } else {
                    updated.retain(|candidate| candidate != &panel);
                    if updated.is_empty() {
                        return Err(EncoderError::InvalidJobSpec("a workspace must keep at least one panel visible".into()));
                    }
                }
                Ok(serde_json::json!({ "visiblePanels": updated, "panel": panel, "visible": visible }))
            },
        ));

        self.register(CommandSpec::new("app.about", "About encoderCraft", "Returns application identity and version information", |_, _| {
            Ok(serde_json::json!({
                "name": "encoderCraft",
                "version": env!("CARGO_PKG_VERSION"),
                "description": "A Rust-native media transcoding and batch queue application"
            }))
        }));

        for (command_id, add_target) in [("queue.prepare_apply_preset", false), ("queue.prepare_add_preset_target", true)] {
            let presets = Arc::clone(&presets);
            self.register(CommandSpec::new(
                command_id,
                "Prepare Preset Job Update",
                "Evaluates a preset against source media and returns the updated job specification",
                move |q, params| {
                    let job_id = required_string(&params, "jobId")?;
                    let preset_name = required_string(&params, "presetName")?;
                    let mut spec = q
                        .list_jobs()
                        .into_iter()
                        .find(|job| job.spec.job_id == job_id)
                        .map(|job| job.spec)
                        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' not found in queue")))?;
                    let preset = presets
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .find_by_name(&preset_name)
                        .cloned()
                        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{preset_name}' not found")))?;
                    let source_path = first_source_path(&spec.source)
                        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' has no local source file")))?;
                    let source_info = encodercraft_source::probe_media_info(&source_path)?;
                    if source_info.video_streams.is_empty() && preset.video.is_some() {
                        return Err(EncoderError::InvalidJobSpec(format!(
                            "cannot apply video preset '{}' to audio-only job '{job_id}'",
                            preset.name
                        )));
                    }
                    let base_path = spec.targets.first().map(|target| target.output_path.clone()).unwrap_or_else(|| source_path.clone());
                    if add_target {
                        let index = spec.targets.len().saturating_add(1);
                        let mut output_path = base_path.clone();
                        let stem = base_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("output");
                        output_path.set_file_name(format!("{stem}_{index}.{}", preset.container.file_extension()));
                        let mut target = encodercraft_presets::evaluate_preset(&preset, &source_info, output_path)?;
                        target.target_id = format!("target-{index}");
                        spec.targets.push(target);
                    } else {
                        let target = spec
                            .targets
                            .first_mut()
                            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("job '{job_id}' has no output target to update")))?;
                        let mut output_path = target.output_path.clone();
                        output_path.set_extension(preset.container.file_extension());
                        let target_id = target.target_id.clone();
                        let mut evaluated = encodercraft_presets::evaluate_preset(&preset, &source_info, output_path)?;
                        evaluated.target_id = target_id;
                        preserve_target_extras(&mut evaluated, target);
                        *target = evaluated;
                    }
                    spec.validate()?;
                    serde_json::to_value(spec).map_err(|error| EncoderError::Internal(format!("failed to serialize updated job: {error}")))
                },
            ));
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchFile {
    version: u32,
    folders: Vec<encodercraft_watch::WatchFolderConfig>,
}

impl Default for WatchFile {
    fn default() -> Self {
        Self { version: 1, folders: Vec::new() }
    }
}

fn required_string(params: &Value, key: &str) -> Result<String, EncoderError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("missing or invalid '{key}' parameter")))
}

fn required_path(params: &Value, key: &str) -> Result<PathBuf, EncoderError> {
    required_string(params, key).map(PathBuf::from)
}

fn workspace_document(params: &Value) -> Result<Value, EncoderError> {
    let value = params.get("workspaces").cloned().ok_or_else(|| EncoderError::InvalidJobSpec("missing 'workspaces' parameter".into()))?;
    validate_workspace_document(&value)?;
    Ok(value)
}

fn apply_workspace_operation(q: &Arc<QueueManager>, params: Value, action: &str) -> Result<Value, EncoderError> {
    let mut workspaces = workspace_document(&params)?;
    let active = required_string(&workspaces, "active")?;
    let next_active;
    {
        let entries = workspaces
            .get_mut("workspaces")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| EncoderError::InvalidJobSpec("workspaces must contain a workspace array".into()))?;
        match action {
            "activate" => {
                let name = required_string(&params, "name")?;
                next_active = name.clone();
                if !entries.iter().any(|workspace| workspace.get("name").and_then(Value::as_str) == Some(name.as_str())) {
                    return Err(EncoderError::InvalidJobSpec(format!("workspace '{name}' does not exist")));
                }
            }
            "save_as" => {
                let name = required_string(&params, "name")?.trim().to_owned();
                if name.is_empty() || is_builtin_workspace(&name) {
                    return Err(EncoderError::InvalidJobSpec(format!("'{name}' is not available as a workspace name")));
                }
                let mut workspace = entries
                    .iter()
                    .find(|workspace| workspace.get("name").and_then(Value::as_str) == Some(active.as_str()))
                    .cloned()
                    .ok_or_else(|| EncoderError::InvalidJobSpec("active workspace is missing".into()))?;
                workspace["name"] = Value::String(name.clone());
                if let Some(existing) = entries.iter_mut().find(|workspace| workspace.get("name").and_then(Value::as_str) == Some(name.as_str())) {
                    *existing = workspace;
                } else {
                    entries.push(workspace);
                }
                next_active = name;
            }
            "rename" => {
                let name = required_string(&params, "name")?.trim().to_owned();
                if is_builtin_workspace(&active)
                    || name.is_empty()
                    || is_builtin_workspace(&name)
                    || entries.iter().any(|workspace| workspace.get("name").and_then(Value::as_str) == Some(name.as_str()))
                {
                    return Err(EncoderError::InvalidJobSpec(format!("'{name}' is not available as a workspace name")));
                }
                let workspace = entries
                    .iter_mut()
                    .find(|workspace| workspace.get("name").and_then(Value::as_str) == Some(active.as_str()))
                    .ok_or_else(|| EncoderError::InvalidJobSpec("active workspace is missing".into()))?;
                workspace["name"] = Value::String(name.clone());
                next_active = name;
            }
            "reset" => {
                let reset_custom_workspace = !is_builtin_workspace(&active);
                next_active = if reset_custom_workspace { "Default".into() } else { active.clone() };
                if reset_custom_workspace {
                    entries.retain(|workspace| workspace.get("name").and_then(Value::as_str) != Some(active.as_str()));
                }
                let mut replacement = params
                    .get("defaultWorkspace")
                    .cloned()
                    .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'defaultWorkspace' parameter".into()))?;
                if !replacement.is_object() || !replacement.get("dock").is_some_and(Value::is_object) {
                    return Err(EncoderError::InvalidJobSpec("defaultWorkspace must contain a dock layout".into()));
                }
                replacement["name"] = Value::String(next_active.clone());
                if let Some(existing) =
                    entries.iter_mut().find(|workspace| workspace.get("name").and_then(Value::as_str) == Some(next_active.as_str()))
                {
                    *existing = replacement;
                } else {
                    entries.push(replacement);
                }
            }
            "delete" => {
                if is_builtin_workspace(&active) {
                    return Err(EncoderError::InvalidJobSpec("built-in workspaces cannot be deleted".into()));
                }
                if !entries.iter().any(|workspace| workspace.get("name").and_then(Value::as_str) == Some("Default")) {
                    return Err(EncoderError::InvalidJobSpec("the Default workspace is missing".into()));
                }
                entries.retain(|workspace| workspace.get("name").and_then(Value::as_str) != Some(active.as_str()));
                next_active = "Default".into();
            }
            _ => return Err(EncoderError::Internal(format!("unknown workspace action '{action}'"))),
        }
    }
    workspaces["active"] = Value::String(next_active);
    validate_workspace_document(&workspaces)?;
    write_json_file_atomic(&q.config_file_path("workspaces.json")?, &workspaces)?;
    Ok(workspaces)
}

fn validate_workspace_document(value: &Value) -> Result<(), EncoderError> {
    let entries = value
        .get("workspaces")
        .and_then(Value::as_array)
        .ok_or_else(|| EncoderError::InvalidJobSpec("workspaces must contain a workspace array".into()))?;
    let active = required_string(value, "active")?;
    let mut names = std::collections::HashSet::new();
    for workspace in entries {
        let name = required_string(workspace, "name")?;
        if name.trim() != name || !names.insert(name.clone()) || !workspace.get("dock").is_some_and(Value::is_object) {
            return Err(EncoderError::InvalidJobSpec("workspace names must be trimmed and unique and layouts must include dock data".into()));
        }
    }
    if !names.contains(&active) {
        return Err(EncoderError::InvalidJobSpec(format!("active workspace '{active}' does not exist")));
    }
    Ok(())
}

fn is_builtin_workspace(name: &str) -> bool {
    matches!(name, "Default" | "Queue Focused" | "Preview Focused")
}

fn validate_workspace_panel(panel: &str) -> Result<(), EncoderError> {
    const PANELS: &[&str] = &["MediaBrowser", "Presets", "Queue", "WatchFolders", "Preview", "Encoding"];
    if PANELS.contains(&panel) { Ok(()) } else { Err(EncoderError::InvalidJobSpec(format!("unknown workspace panel '{panel}'"))) }
}

fn validate_workspace_panels(panels: &[String]) -> Result<(), EncoderError> {
    let mut seen = std::collections::HashSet::new();
    for panel in panels {
        validate_workspace_panel(panel)?;
        if !seen.insert(panel) {
            return Err(EncoderError::InvalidJobSpec(format!("duplicate workspace panel '{panel}'")));
        }
    }
    Ok(())
}

fn read_json_file<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> Result<T, EncoderError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_APP_DATA_BYTES => {
            return Err(EncoderError::Io(format!("configuration file exceeds the {MAX_APP_DATA_BYTES}-byte limit")));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(EncoderError::Io(format!("failed to inspect configuration file: {error}"))),
    }
    let file = File::open(path).map_err(|error| EncoderError::Io(format!("failed to open configuration file: {error}")))?;
    serde_json::from_reader(file).map_err(|error| EncoderError::Internal(format!("failed to deserialize configuration file: {error}")))
}

fn write_json_file_atomic(path: &std::path::Path, value: &impl serde::Serialize) -> Result<(), EncoderError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| EncoderError::Io(format!("failed to create configuration directory: {error}")))?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| EncoderError::Internal(format!("failed to serialize configuration: {error}")))?;
    if bytes.len() as u64 > MAX_APP_DATA_BYTES {
        return Err(EncoderError::Io(format!("configuration data exceeds the {MAX_APP_DATA_BYTES}-byte limit")));
    }
    let temp_path = path.with_extension("json.tmp");
    let mut file = File::create(&temp_path).map_err(|error| EncoderError::Io(format!("failed to create temporary configuration: {error}")))?;
    file.write_all(&bytes).map_err(|error| EncoderError::Io(format!("failed to write temporary configuration: {error}")))?;
    file.sync_all().map_err(|error| EncoderError::Io(format!("failed to sync temporary configuration: {error}")))?;
    std::fs::rename(&temp_path, path).map_err(|error| EncoderError::Io(format!("failed to commit configuration: {error}")))
}

fn first_source_path(source: &SourceDescriptor) -> Option<PathBuf> {
    match source {
        SourceDescriptor::File { path, .. } => Some(path.clone()),
        SourceDescriptor::StitchedClips { clips } => clips.first().map(|clip| clip.path.clone()),
        SourceDescriptor::ProjectReference { project_path, .. } => Some(project_path.clone()),
        SourceDescriptor::Pipe { .. } => None,
    }
}

fn preserve_target_extras(target: &mut encodercraft_core::OutputTarget, old: &encodercraft_core::OutputTarget) {
    target.lut_path.clone_from(&old.lut_path);
    target.burn_timecode = old.burn_timecode;
    target.caption_path.clone_from(&old.caption_path);
    target.caption_format = old.caption_format;
    target.burn_captions = old.burn_captions;
    target.caption_offset_ms = old.caption_offset_ms;
    target.audio_stems = old.audio_stems;
    target.metadata.clone_from(&old.metadata);
    target.hardware_acceleration = old.hardware_acceleration;
    target.image_quality = old.image_quality;
    target.selection = old.selection.clone();
    target.transform = old.transform;
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
    use std::path::PathBuf;

    fn sample_job(job_id: &str, source: &str) -> CraftJobSpec {
        CraftJobSpec {
            version: "1.0".into(),
            job_id: job_id.into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from(source), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: format!("{job_id}-target"),
                output_path: PathBuf::from(format!("{job_id}.mp4")),
                preset_name: "Web".into(),
                container: ContainerType::IsoBmffMp4,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn test_command_registry_dispatch() {
        let queue = Arc::new(QueueManager::new_in_memory());
        let registry = CommandRegistry::new();

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: "cmd-test-1".into(),
            client_info: ClientInfo { app_name: "test".into(), app_version: "1.0".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("in.mov"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "t1".into(),
                output_path: PathBuf::from("out.mp4"),
                preset_name: "Web".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            }],
        };

        // queue.add
        let res = registry.dispatch(&queue, "queue.add", serde_json::to_value(&spec).unwrap()).unwrap();
        assert_eq!(res.get("jobId").unwrap().as_str().unwrap(), "cmd-test-1");

        // queue.list
        let list_res = registry.dispatch(&queue, "queue.list", serde_json::json!({})).unwrap();
        let arr = list_res.as_array().unwrap();
        assert_eq!(arr.len(), 1);

        // queue.pause
        let pause_res = registry.dispatch(&queue, "queue.pause", serde_json::json!({ "jobId": "cmd-test-1" })).unwrap();
        assert_eq!(pause_res.get("status").unwrap().as_str().unwrap(), "paused");

        // queue.duplicate
        let dup_res = registry.dispatch(&queue, "queue.duplicate", serde_json::json!({ "jobId": "cmd-test-1" })).unwrap();
        assert_eq!(dup_res.get("jobId").unwrap().as_str().unwrap(), "cmd-test-1-copy");

        // queue.move_up & move_down
        let up_res = registry.dispatch(&queue, "queue.move_up", serde_json::json!({ "jobId": "cmd-test-1-copy" })).unwrap();
        assert!(up_res.get("success").unwrap().as_bool().unwrap());
        let down_res = registry.dispatch(&queue, "queue.move_down", serde_json::json!({ "jobId": "cmd-test-1-copy" })).unwrap();
        assert!(down_res.get("success").unwrap().as_bool().unwrap());

        // queue.reset
        let reset_res = registry.dispatch(&queue, "queue.reset", serde_json::json!({ "jobId": "cmd-test-1" })).unwrap();
        assert!(reset_res.get("success").unwrap().as_bool().unwrap());

        // queue.remove
        let remove_res = registry.dispatch(&queue, "queue.remove", serde_json::json!({ "jobId": "cmd-test-1" })).unwrap();
        assert!(remove_res.get("success").unwrap().as_bool().unwrap());

        // queue.clear_finished
        let clear_res = registry.dispatch(&queue, "queue.clear_finished", serde_json::json!({})).unwrap();
        assert!(clear_res.get("removedCount").is_some());
    }

    #[test]
    fn pause_worker_command_pauses_the_queue() {
        let queue = Arc::new(QueueManager::new_in_memory());
        let registry = CommandRegistry::new();

        registry.dispatch(&queue, "queue.pause_worker", serde_json::json!({})).unwrap();

        assert!(queue.is_paused());
    }

    #[test]
    fn shared_application_commands_persist_preferences_favorites_watch_folders_and_presets() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let presets = Arc::new(Mutex::new(encodercraft_presets::PresetRegistry::new(Some(dir.path().join("presets")))));
        let registry = CommandRegistry::with_preset_registry(Arc::clone(&presets));

        registry.dispatch(&queue, "preferences.set_theme", serde_json::json!({ "theme": "Light" })).unwrap();
        assert_eq!(registry.dispatch(&queue, "preferences.get", serde_json::json!({})).unwrap()["theme"], "Light");

        let favorite = dir.path().join("media");
        let toggled = registry.dispatch(&queue, "media.toggle_favorite", serde_json::json!({ "path": favorite })).unwrap();
        assert_eq!(toggled.as_array().unwrap().len(), 1);
        assert_eq!(registry.dispatch(&queue, "media.list_favorites", serde_json::json!({})).unwrap(), toggled);

        let watch_dir = dir.path().join("watch");
        let added = registry.dispatch(&queue, "watch.add_folder", serde_json::json!({ "watchDir": watch_dir })).unwrap();
        assert_eq!(registry.dispatch(&queue, "watch.list_folders", serde_json::json!({})).unwrap()[0]["id"], added["id"]);

        let mut preset = presets.lock().unwrap().list()[0].clone();
        preset.name = "Command Test Preset".into();
        preset.category = encodercraft_presets::PresetCategory::Custom;
        registry.dispatch(&queue, "preset.create", serde_json::json!({ "preset": preset })).unwrap();
        assert!(
            registry
                .dispatch(&queue, "preset.list", serde_json::json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["name"] == "Command Test Preset")
        );
        registry.dispatch(&queue, "preset.delete", serde_json::json!({ "name": "Command Test Preset" })).unwrap();
    }

    #[test]
    fn workspace_commands_apply_and_persist_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Arc::new(QueueManager::new_in_memory());
        queue.set_conversion_preferences_path(dir.path().join("preferences.json"));
        let registry = CommandRegistry::new();
        let workspaces = serde_json::json!({
            "active": "Default",
            "workspaces": [
                { "name": "Default", "dock": { "layout": "default" } },
                { "name": "Queue Focused", "dock": { "layout": "queue" } },
                { "name": "Preview Focused", "dock": { "layout": "preview" } }
            ]
        });
        registry.dispatch(&queue, "workspace.save", serde_json::json!({ "workspaces": workspaces.clone() })).unwrap();

        let saved_as = registry.dispatch(&queue, "workspace.save_as", serde_json::json!({ "workspaces": workspaces, "name": "Studio" })).unwrap();
        assert_eq!(saved_as["active"], "Studio");
        assert_eq!(saved_as["workspaces"].as_array().unwrap().len(), 4);

        let renamed = registry.dispatch(&queue, "workspace.rename", serde_json::json!({ "workspaces": saved_as, "name": "Edit Suite" })).unwrap();
        assert_eq!(renamed["active"], "Edit Suite");
        let reset_custom = registry
            .dispatch(
                &queue,
                "workspace.reset",
                serde_json::json!({
                    "workspaces": renamed.clone(),
                    "defaultWorkspace": { "name": "Default", "dock": { "layout": "factory-default" } }
                }),
            )
            .unwrap();
        assert_eq!(reset_custom["active"], "Default");
        assert_eq!(reset_custom["workspaces"].as_array().unwrap().len(), 3);

        let activated =
            registry.dispatch(&queue, "workspace.activate", serde_json::json!({ "workspaces": renamed.clone(), "name": "Preview Focused" })).unwrap();
        assert_eq!(activated["active"], "Preview Focused");

        let reset = registry
            .dispatch(
                &queue,
                "workspace.reset",
                serde_json::json!({
                    "workspaces": activated,
                    "defaultWorkspace": { "name": "Preview Focused", "dock": { "layout": "factory-preview" } }
                }),
            )
            .unwrap();
        assert_eq!(reset["workspaces"][2]["dock"]["layout"], "factory-preview");

        let deleted = registry.dispatch(&queue, "workspace.delete", serde_json::json!({ "workspaces": renamed })).unwrap();
        assert_eq!(deleted["active"], "Default");
        assert_eq!(deleted["workspaces"].as_array().unwrap().len(), 3);
        assert_eq!(registry.dispatch(&queue, "workspace.get", serde_json::json!({})).unwrap(), deleted);
        assert!(registry.dispatch(&queue, "workspace.delete", serde_json::json!({ "workspaces": deleted })).is_err());
    }

    #[test]
    fn prepare_commands_return_specs_without_submitting_jobs() {
        let queue = Arc::new(QueueManager::new_in_memory());
        let registry = CommandRegistry::new();
        queue.submit_job(sample_job("prepare-one", "one.mov")).unwrap();
        queue.submit_job(sample_job("prepare-two", "two.mov")).unwrap();

        let duplicate = registry.dispatch(&queue, "queue.duplicate_spec", serde_json::json!({ "jobId": "prepare-one" })).unwrap();
        let duplicate: CraftJobSpec = serde_json::from_value(duplicate).unwrap();
        assert_eq!(
            duplicate.source,
            SourceDescriptor::File { path: PathBuf::from("one.mov"), start_tick: None, duration_ticks: None, timebase: None }
        );

        let stitched = registry
            .dispatch(
                &queue,
                "queue.prepare_stitch",
                serde_json::json!({
                    "jobIds": ["prepare-one", "prepare-two"],
                    "outputPath": "stitched.mp4",
                    "presetName": "Web",
                }),
            )
            .unwrap();
        let stitched: CraftJobSpec = serde_json::from_value(stitched).unwrap();
        assert!(matches!(stitched.source, SourceDescriptor::StitchedClips { .. }));
        assert_eq!(queue.list_jobs().len(), 2);

        let stitched_sources = registry
            .dispatch(
                &queue,
                "queue.prepare_stitch_sources",
                serde_json::json!({
                    "paths": ["one.mov", "two.mov"],
                    "outputPath": "stitched-sources.mp4",
                    "presetName": "Web",
                }),
            )
            .unwrap();
        let stitched_sources: CraftJobSpec = serde_json::from_value(stitched_sources).unwrap();
        assert!(matches!(stitched_sources.source, SourceDescriptor::StitchedClips { .. }));
    }

    #[test]
    fn queue_commands_apply_approved_updates_and_target_actions() {
        let queue = Arc::new(QueueManager::new_in_memory());
        let registry = CommandRegistry::new();
        let spec = sample_job("queue-command-test", "source.mov");

        registry.dispatch(&queue, "queue.add_with_approvals", serde_json::json!({ "spec": spec, "approvals": [] })).unwrap();

        registry.dispatch(&queue, "queue.set_parallelism", serde_json::json!({ "workers": 3 })).unwrap();
        assert_eq!(queue.parallelism(), 3);

        registry
            .dispatch(&queue, "queue.set_trim_range", serde_json::json!({ "jobId": "queue-command-test", "inFrame": 2, "outFrame": 7, "fps": 24 }))
            .unwrap();
        let queued = queue.list_jobs().into_iter().find(|job| job.spec.job_id == "queue-command-test").unwrap();
        assert!(matches!(queued.spec.source, SourceDescriptor::File { start_tick: Some(2), duration_ticks: Some(6), .. }));

        let target = OutputTarget {
            target_id: "second-target".into(),
            output_path: PathBuf::from("second.mp4"),
            preset_name: "Web".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        };
        let prepared = registry
            .dispatch(&queue, "queue.prepare_add_output_target", serde_json::json!({ "jobId": "queue-command-test", "target": target }))
            .unwrap();
        let prepared: CraftJobSpec = serde_json::from_value(prepared).unwrap();
        assert_eq!(prepared.targets.len(), 2);
        assert_eq!(queue.list_jobs()[0].spec.targets.len(), 1);
        let generated = registry.dispatch(&queue, "queue.prepare_add_output_target", serde_json::json!({ "jobId": "queue-command-test" })).unwrap();
        let generated: CraftJobSpec = serde_json::from_value(generated).unwrap();
        assert_eq!(generated.targets.len(), 2);
        assert_eq!(generated.targets[1].target_id, "target-2");
        assert_eq!(generated.targets[1].output_path, PathBuf::from("output_2.mp4"));

        let updated_target = OutputTarget { output_path: PathBuf::from("renamed.mp4"), ..prepared.targets[0].clone() };
        registry
            .dispatch(
                &queue,
                "queue.update_output_target",
                serde_json::json!({ "jobId": "queue-command-test", "targetIndex": 0, "target": updated_target }),
            )
            .unwrap();
        registry.dispatch(&queue, "queue.duplicate_output_target", serde_json::json!({ "jobId": "queue-command-test", "targetIndex": 0 })).unwrap();
        assert_eq!(queue.list_jobs()[0].spec.targets.len(), 2);
        registry.dispatch(&queue, "queue.remove_output_target", serde_json::json!({ "jobId": "queue-command-test", "targetIndex": 1 })).unwrap();
        assert_eq!(queue.list_jobs()[0].spec.targets[0].output_path, PathBuf::from("renamed.mp4"));

        let updated = sample_job("queue-command-test", "source.mov");
        registry
            .dispatch(&queue, "queue.update_with_approvals", serde_json::json!({ "jobId": "queue-command-test", "spec": updated, "approvals": [] }))
            .unwrap();
    }
}
