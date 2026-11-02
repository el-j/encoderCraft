//! Command infrastructure: "Everything is a command" (AGENTS.md).
//!
//! Provides a unified interface reachable by GUI, CLI, loopback IPC, and MCP alike.

use std::collections::HashMap;

use encodercraft_core::EncoderError;
use serde_json::Value;

use crate::queue::QueueManager;

/// Specification and metadata of an executable command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    handler: Box<dyn Fn(&QueueManager, Value) -> Result<Value, EncoderError> + Send + Sync>,
}

impl CommandSpec {
    pub fn new<F>(id: &'static str, label: &'static str, description: &'static str, handler: F) -> Self
    where
        F: Fn(&QueueManager, Value) -> Result<Value, EncoderError> + Send + Sync + 'static,
    {
        Self { id, label, description, handler: Box::new(handler) }
    }

    pub fn execute(&self, queue: &QueueManager, params: Value) -> Result<Value, EncoderError> {
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
        let mut reg = Self { commands: HashMap::new() };
        reg.register_builtins();
        reg
    }

    /// Registers a command into the registry.
    pub fn register(&mut self, spec: CommandSpec) {
        self.commands.insert(spec.id, spec);
    }

    /// Dispatches a command by ID with JSON parameters.
    pub fn dispatch(&self, queue: &QueueManager, cmd_id: &str, params: Value) -> Result<Value, EncoderError> {
        let cmd = self.commands.get(cmd_id).ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown command '{cmd_id}'")))?;
        cmd.execute(queue, params)
    }

    /// Lists all registered command specifications.
    pub fn list(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        self.commands.values().map(|c| (c.id, c.label, c.description)).collect()
    }

    fn register_builtins(&mut self) {
        // queue.add
        self.register(CommandSpec::new("queue.add", "Add Job", "Submits a new encoding job to the queue", |q, params| {
            let spec: encodercraft_core::CraftJobSpec =
                serde_json::from_value(params).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to parse job spec: {e}")))?;
            let job_id = q.submit_job(spec)?;
            Ok(serde_json::json!({ "jobId": job_id }))
        }));

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
                // Process next job
                let processed = q.process_next_job()?;
                Ok(serde_json::json!({ "processed": processed }))
            }
        }));

        // queue.pause
        self.register(CommandSpec::new("queue.pause", "Pause Job", "Pauses an active or queued job", |q, params| {
            let job_id =
                params.get("jobId").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing 'jobId' parameter".into()))?;
            q.pause_job(job_id)?;
            Ok(serde_json::json!({ "jobId": job_id, "status": "paused" }))
        }));

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
    use std::path::PathBuf;

    #[test]
    fn test_command_registry_dispatch() {
        let queue = QueueManager::new_in_memory();
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
}
