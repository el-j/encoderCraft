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
            "queue.start" => {
                let _ = self.queue_mgr.process_next_job();
                IpcResponse::success(req.id, json!(true))
            }
            "queue.pause" => {
                let job_id = req.params.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
                if !job_id.is_empty() {
                    let _ = self.queue_mgr.pause_job(job_id);
                }
                IpcResponse::success(req.id, json!(true))
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
