//! MCP tool definitions, JSON schemas, and execution handlers.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_demux::probe_file;
use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use serde_json::{Value, json};

/// Returns the schema definition of all tools supported by the encoderCraft MCP server.
pub fn list_tools() -> Value {
    json!([
        {
            "name": "encodercraft_submit_transcode",
            "description": "Submits a media transcoding job to encoderCraft using a named preset.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_path": { "type": "string", "description": "Absolute filesystem path to the source media file." },
                    "preset_name": { "type": "string", "description": "Name of the encoding preset (e.g. 'YouTube 4K UHD', 'Match Source - High Bitrate')." },
                    "output_path": { "type": "string", "description": "Destination file path for the encoded output." }
                },
                "required": ["source_path", "preset_name", "output_path"]
            }
        },
        {
            "name": "encodercraft_get_queue_status",
            "description": "Returns the status of all queued, processing, and completed transcode jobs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "Optional specific job ID to inspect." }
                }
            }
        },
        {
            "name": "encodercraft_list_presets",
            "description": "Lists all available factory and user presets categorized by workflow.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Optional filter (web, broadcast, social, audio, spatial3d)." }
                }
            }
        },
        {
            "name": "encodercraft_probe_media",
            "description": "Probes a media file and returns its container, stream codecs, dimensions, framerate, and duration.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Filesystem path to the media file to inspect." }
                },
                "required": ["path"]
            }
        }
    ])
}

/// Executes an MCP tool call by name with given JSON arguments.
pub fn call_tool(
    name: &str,
    args: &Value,
    queue_mgr: &Arc<QueueManager>,
    preset_registry: &Arc<Mutex<PresetRegistry>>,
) -> Result<Value, EncoderError> {
    match name {
        "encodercraft_submit_transcode" => {
            let source_path = args
                .get("source_path")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'source_path'".into()))?;
            let preset_name = args
                .get("preset_name")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'preset_name'".into()))?;
            let output_path = args
                .get("output_path")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'output_path'".into()))?;

            let reg = preset_registry.lock().unwrap_or_else(PoisonError::into_inner);
            let preset = reg.find_by_name(preset_name).ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown preset '{preset_name}'")))?;

            let job_id = format!("mcp-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));

            let target = OutputTarget {
                target_id: format!("{job_id}-target-1"),
                output_path: PathBuf::from(output_path),
                preset_name: preset.name.clone(),
                container: preset.container,
                custom_width: None,
                custom_height: None,
                custom_frame_rate: None,
                target_bitrate_kbps: None,
                target_lufs: None,
                ..Default::default()
            };

            let spec = CraftJobSpec {
                version: "1.0".into(),
                job_id: job_id.clone(),
                client_info: ClientInfo {
                    app_name: "encoderCraft MCP Server".into(),
                    app_version: "0.1.0".into(),
                    pid: std::process::id(),
                    user_notes: Some("Submitted via Model Context Protocol".into()),
                },
                priority: JobPriority::Normal,
                start_policy: StartPolicy::AutoStart,
                source: SourceDescriptor::File { path: PathBuf::from(source_path), start_tick: None, duration_ticks: None, timebase: None },
                targets: vec![target],
            };

            let assigned = queue_mgr.submit_job(spec)?;
            Ok(json!({
                "jobId": assigned,
                "status": "Queued",
                "message": format!("Job successfully queued for transcode with preset '{preset_name}'")
            }))
        }
        "encodercraft_get_queue_status" => {
            let jobs = queue_mgr.list_jobs();
            if let Some(target_id) = args.get("job_id").and_then(Value::as_str) {
                if let Some(job) = jobs.iter().find(|j| j.spec.job_id == target_id) {
                    Ok(json!(job))
                } else {
                    Err(EncoderError::InvalidJobSpec(format!("job '{target_id}' not found")))
                }
            } else {
                Ok(json!({ "totalJobs": jobs.len(), "jobs": jobs }))
            }
        }
        "encodercraft_list_presets" => {
            let reg = preset_registry.lock().unwrap_or_else(PoisonError::into_inner);
            let presets = reg.list();
            Ok(json!({ "count": presets.len(), "presets": presets }))
        }
        "encodercraft_probe_media" => {
            let path_str =
                args.get("path").and_then(Value::as_str).ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'path'".into()))?;
            let info = probe_file(std::path::Path::new(path_str))?;
            Ok(json!(info))
        }
        _ => Err(EncoderError::InvalidJobSpec(format!("unknown MCP tool '{name}'"))),
    }
}
