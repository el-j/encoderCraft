//! Standard IPC dispatch handler connecting the queue engine to inter-app protocols.

use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::CraftJobSpec;
use encodercraft_dispatch::{IpcRequest, IpcResponse};
use encodercraft_ipc::IpcHandler;
use encodercraft_presets::PresetRegistry;
use serde_json::json;

use crate::queue::QueueManager;

/// Reusable IPC handler for encoderCraft engine.
pub struct EngineIpcHandler {
    pub queue_mgr: Arc<QueueManager>,
    pub preset_registry: Arc<Mutex<PresetRegistry>>,
}

impl EngineIpcHandler {
    /// Creates a new `EngineIpcHandler` instance.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        Self { queue_mgr, preset_registry }
    }
}

impl IpcHandler for EngineIpcHandler {
    fn handle(&self, req: &IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "app.ping" => IpcResponse::success(req.id, json!({ "pong": true, "app": "encoderCraft", "version": "0.1.0" })),
            "app.version" => IpcResponse::success(req.id, json!({ "app": "encoderCraft", "version": "0.1.0", "engine": "ArtCraft-Rust" })),
            "queue.submit" => {
                let spec: CraftJobSpec = match serde_json::from_value(req.params.clone()) {
                    Ok(s) => s,
                    Err(e) => return IpcResponse::error(req.id, format!("invalid job spec: {e}")),
                };
                match self.queue_mgr.submit_job(spec) {
                    Ok(id) => IpcResponse::success(req.id, json!(id)),
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
