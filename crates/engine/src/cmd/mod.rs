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

        // queue.remove
        let remove_res = registry.dispatch(&queue, "queue.remove", serde_json::json!({ "jobId": "cmd-test-1" })).unwrap();
        assert!(remove_res.get("success").unwrap().as_bool().unwrap());
    }
}
