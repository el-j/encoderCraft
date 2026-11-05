//! Standard IPC dispatch handler connecting the queue engine to inter-app protocols.

use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{CraftJobSpec, JobStatus, StartPolicy};
use encodercraft_dispatch::{IpcRequest, IpcResponse};
use encodercraft_ipc::IpcHandler;
use encodercraft_presets::PresetRegistry;
use serde_json::json;

use crate::cmd::CommandRegistry;
use crate::queue::QueueManager;

/// Reusable IPC handler for encoderCraft engine.
pub struct EngineIpcHandler {
    pub queue_mgr: Arc<QueueManager>,
    pub preset_registry: Arc<Mutex<PresetRegistry>>,
    command_registry: CommandRegistry,
}

impl EngineIpcHandler {
    /// Creates a new `EngineIpcHandler` instance.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        let command_registry = CommandRegistry::with_preset_registry(Arc::clone(&preset_registry));
        Self { queue_mgr, preset_registry, command_registry }
    }
}

impl IpcHandler for EngineIpcHandler {
    /// `queue.subscribe` acknowledges, then pushes one `queue.event` per job whenever its status or
    /// whole-percent progress changes, until the client disconnects.
    fn stream(&self, req: &IpcRequest, emit: &mut dyn FnMut(IpcResponse) -> bool) -> bool {
        if req.method != "queue.subscribe" {
            return false;
        }
        if !emit(IpcResponse::success(req.id, json!({ "subscribed": true }))) {
            return true;
        }
        let mut seen: std::collections::HashMap<String, (JobStatus, u32, Option<u64>)> = std::collections::HashMap::new();
        loop {
            let jobs = self.queue_mgr.list_jobs();
            for job in &jobs {
                let key = (job.status.clone(), job.progress_percent.clamp(0.0, 100.0) as u32, job.target_eta_seconds);
                if seen.get(&job.spec.job_id) != Some(&key) {
                    let event = json!({
                        "event": "queue.event",
                        "jobId": job.spec.job_id,
                        "status": job.status,
                        "progressPercent": job.progress_percent,
                        "fps": job.current_fps,
                        "targetEtaSeconds": job.target_eta_seconds,
                    });
                    if !emit(IpcResponse::success(req.id, event)) {
                        return true;
                    }
                    seen.insert(job.spec.job_id.clone(), key);
                }
            }
            seen.retain(|id, _| jobs.iter().any(|j| &j.spec.job_id == id));
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    fn handle(&self, req: &IpcRequest) -> IpcResponse {
        if req.method == "command.list" {
            let commands: Vec<_> = self
                .command_registry
                .list()
                .into_iter()
                .map(|(id, label, description)| json!({ "id": id, "label": label, "description": description }))
                .collect();
            return IpcResponse::success(req.id, json!(commands));
        }
        if req.method == "command.execute" {
            let Some(command_id) = req.params.get("commandId").and_then(|value| value.as_str()) else {
                return IpcResponse::error(req.id, "missing commandId parameter");
            };
            let params = req.params.get("params").cloned().unwrap_or_else(|| json!({}));
            return match self.command_registry.dispatch(&self.queue_mgr, command_id, params) {
                Ok(result) => IpcResponse::success(req.id, result),
                Err(error) => IpcResponse::error(req.id, error.to_string()),
            };
        }
        if self.command_registry.contains(&req.method) {
            return match self.command_registry.dispatch(&self.queue_mgr, &req.method, req.params.clone()) {
                Ok(result) => IpcResponse::success(req.id, result),
                Err(error) => IpcResponse::error(req.id, error.to_string()),
            };
        }
        match req.method.as_str() {
            "app.ping" => IpcResponse::success(req.id, json!({ "pong": true, "app": "encoderCraft", "version": "0.1.0" })),
            "app.version" => IpcResponse::success(req.id, json!({ "app": "encoderCraft", "version": "0.1.0", "engine": "ArtCraft-Rust" })),
            "queue.submit" => {
                let spec: CraftJobSpec = match serde_json::from_value(req.params.clone()) {
                    Ok(s) => s,
                    Err(e) => return IpcResponse::error(req.id, format!("invalid job spec: {e}")),
                };
                let policy = spec.start_policy;
                match self.queue_mgr.submit_job(spec) {
                    Ok(id) => {
                        match policy {
                            StartPolicy::AutoStart => {
                                self.queue_mgr.start_queue();
                            }
                            StartPolicy::Paused => {
                                if let Err(e) = self.queue_mgr.pause_job(&id) {
                                    return IpcResponse::error(req.id, format!("job '{id}' queued but could not be paused: {e}"));
                                }
                            }
                        }
                        IpcResponse::success(req.id, json!(id))
                    }
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.status" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                let jobs = self.queue_mgr.list_jobs();
                if let Some(j) = jobs.iter().find(|j| j.spec.job_id == job_id) {
                    IpcResponse::success(req.id, json!(j.status))
                } else {
                    IpcResponse::error(req.id, format!("job '{job_id}' not found"))
                }
            }
            "queue.list" => {
                let jobs = self.queue_mgr.list_jobs();
                IpcResponse::success(req.id, json!(jobs))
            }
            "queue.cancel" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                match self.queue_mgr.cancel_job(job_id) {
                    Ok(()) => IpcResponse::success(req.id, json!(true)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.start" => IpcResponse::success(req.id, json!(self.queue_mgr.start_queue())),
            "queue.pause" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                if !job_id.is_empty() {
                    let _ = self.queue_mgr.pause_job(job_id);
                }
                IpcResponse::success(req.id, json!(true))
            }
            "queue.duplicate" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                match self.queue_mgr.duplicate_job(job_id) {
                    Ok(new_id) => IpcResponse::success(req.id, json!(new_id)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.move_up" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                match self.queue_mgr.move_job_up(job_id) {
                    Ok(()) => IpcResponse::success(req.id, json!(true)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.move_down" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                match self.queue_mgr.move_job_down(job_id) {
                    Ok(()) => IpcResponse::success(req.id, json!(true)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.reset" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                match self.queue_mgr.reset_job_status(job_id) {
                    Ok(()) => IpcResponse::success(req.id, json!(true)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.clear_finished" => match self.queue_mgr.clear_completed_jobs() {
                Ok(count) => IpcResponse::success(req.id, json!({ "removedCount": count })),
                Err(e) => IpcResponse::error(req.id, e.to_string()),
            },
            "queue.stitch" => {
                let paths: Vec<std::path::PathBuf> =
                    match serde_json::from_value(req.params.get("paths").cloned().unwrap_or(serde_json::Value::Array(Vec::new()))) {
                        Ok(p) => p,
                        Err(e) => return IpcResponse::error(req.id, format!("invalid paths: {e}")),
                    };
                let output_path = match req.params.get("outputPath").and_then(|v| v.as_str()) {
                    Some(p) => std::path::PathBuf::from(p),
                    None => return IpcResponse::error(req.id, "missing outputPath parameter"),
                };
                let preset_name = req.params.get("presetName").and_then(|v| v.as_str()).unwrap_or("Match Source - High Bitrate").to_string();

                match self.queue_mgr.stitch_sources_into_job(paths, output_path, preset_name) {
                    Ok(job_id) => IpcResponse::success(req.id, json!(job_id)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.stitch_selected" => {
                let job_ids: Vec<String> =
                    match serde_json::from_value(req.params.get("jobIds").cloned().unwrap_or(serde_json::Value::Array(Vec::new()))) {
                        Ok(ids) => ids,
                        Err(e) => return IpcResponse::error(req.id, format!("invalid jobIds: {e}")),
                    };
                let output_path = match req.params.get("outputPath").and_then(|v| v.as_str()) {
                    Some(p) => std::path::PathBuf::from(p),
                    None => return IpcResponse::error(req.id, "missing outputPath parameter"),
                };
                let preset_name = req.params.get("presetName").and_then(|v| v.as_str()).unwrap_or("Match Source - High Bitrate").to_string();

                match self.queue_mgr.stitch_selected_jobs(&job_ids, output_path, preset_name) {
                    Ok(job_id) => IpcResponse::success(req.id, json!(job_id)),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.set_output_dir" => {
                let job_ids: Vec<String> =
                    match serde_json::from_value(req.params.get("jobIds").cloned().unwrap_or(serde_json::Value::Array(Vec::new()))) {
                        Ok(ids) => ids,
                        Err(e) => return IpcResponse::error(req.id, format!("invalid jobIds: {e}")),
                    };
                let output_dir = match req.params.get("outputDir").and_then(|v| v.as_str()) {
                    Some(p) => std::path::PathBuf::from(p),
                    None => return IpcResponse::error(req.id, "missing outputDir parameter"),
                };

                match self.queue_mgr.set_jobs_output_dir(&job_ids, output_dir) {
                    Ok(count) => IpcResponse::success(req.id, json!({ "modifiedCount": count })),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "queue.log" => {
                let entries = self.queue_mgr.list_log_entries();
                IpcResponse::success(req.id, json!(entries))
            }
            "queue.clear_log" => match self.queue_mgr.clear_log_entries() {
                Ok(()) => IpcResponse::success(req.id, json!(true)),
                Err(e) => IpcResponse::error(req.id, e.to_string()),
            },
            "queue.is_running" => {
                let running = self.queue_mgr.is_worker_running();
                IpcResponse::success(req.id, json!({ "isRunning": running }))
            }
            "presets.list" => {
                let reg = self.preset_registry.lock().unwrap_or_else(PoisonError::into_inner);
                let presets = reg.list();
                IpcResponse::success(req.id, json!(presets))
            }
            _ => IpcResponse::error(req.id, format!("unknown method '{}'", req.method)),
        }
    }
}
