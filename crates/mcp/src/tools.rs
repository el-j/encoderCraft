//! MCP tool definitions, JSON schemas, and execution handlers.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_demux::probe_file;
use encodercraft_engine::{QueueManager, cmd::CommandRegistry};
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
        },
        {
            "name": "encodercraft_batch_transcode",
            "description": "Batch submits multiple media files to encoderCraft with a single preset.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_paths": { "type": "array", "items": { "type": "string" }, "description": "List of source file paths." },
                    "preset_name": { "type": "string", "description": "Encoding preset name." },
                    "output_dir": { "type": "string", "description": "Optional destination directory for encoded files." }
                },
                "required": ["source_paths", "preset_name"]
            }
        },
        {
            "name": "encodercraft_stitch_clips",
            "description": "Stitches multiple sequential media clips into a single continuous video job.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_paths": { "type": "array", "items": { "type": "string" }, "description": "Ordered list of clips to concatenate." },
                    "output_path": { "type": "string", "description": "Destination file path for the stitched output." },
                    "preset_name": { "type": "string", "description": "Preset name (default: 'Match Source - High Bitrate')." }
                },
                "required": ["clip_paths", "output_path"]
            }
        },
        {
            "name": "encodercraft_manage_queue",
            "description": "Executes batch queue control commands: start, pause, cancel, duplicate, reset, or clear_finished.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["start", "pause", "cancel", "duplicate", "reset", "clear_finished"], "description": "Queue management action." },
                    "job_id": { "type": "string", "description": "Target job ID (required for pause, cancel, duplicate, reset)." }
                },
                "required": ["action"]
            }
        },
        {
            "name": "encodercraft_get_log",
            "description": "Retrieves the transcode history log with timestamps, durations, preset names, and statuses.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "encodercraft_set_output_dir",
            "description": "Batch re-routes destination output directory across specified jobs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "job_ids": { "type": "array", "items": { "type": "string" }, "description": "List of job IDs to update." },
                    "output_dir": { "type": "string", "description": "New target directory path." }
                },
                "required": ["job_ids", "output_dir"]
            }
        },
        {
            "name": "encodercraft_list_commands",
            "description": "Lists the shared engine commands available through the GUI, CLI, IPC, and MCP.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "encodercraft_execute_command",
            "description": "Executes a shared engine command by ID with its JSON parameters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command_id": { "type": "string", "description": "A command ID returned by encodercraft_list_commands." },
                    "params": { "type": "object", "description": "The command's JSON parameters.", "additionalProperties": true }
                },
                "required": ["command_id"]
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
        "encodercraft_list_commands" => {
            let registry = CommandRegistry::with_preset_registry(Arc::clone(preset_registry));
            let commands: Vec<_> =
                registry.list().into_iter().map(|(id, label, description)| json!({ "id": id, "label": label, "description": description })).collect();
            Ok(json!({ "commands": commands }))
        }
        "encodercraft_execute_command" => {
            let command_id = args
                .get("command_id")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'command_id'".into()))?;
            let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
            CommandRegistry::with_preset_registry(Arc::clone(preset_registry)).dispatch(queue_mgr, command_id, params)
        }
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
            start_worker_unless_stopped(queue_mgr);
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
        "encodercraft_batch_transcode" => {
            let paths: Vec<String> = serde_json::from_value(args.get("source_paths").cloned().unwrap_or(Value::Array(Vec::new())))
                .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid source_paths: {e}")))?;
            let preset_name = args
                .get("preset_name")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'preset_name'".into()))?;
            let out_dir = args.get("output_dir").and_then(Value::as_str).map(PathBuf::from);

            let reg = preset_registry.lock().unwrap_or_else(PoisonError::into_inner);
            let preset = reg.find_by_name(preset_name).ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown preset '{preset_name}'")))?;

            let mut queued_ids = Vec::new();
            for (idx, p_str) in paths.iter().enumerate() {
                let p = PathBuf::from(p_str);
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
                let out_file =
                    if let Some(ref d) = out_dir { d.join(format!("{stem}_encoded.mp4")) } else { p.with_file_name(format!("{stem}_encoded.mp4")) };

                let job_id = format!(
                    "mcp-batch-{}-{}",
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
                    idx + 1
                );
                let target = OutputTarget {
                    target_id: format!("{job_id}-target-1"),
                    output_path: out_file,
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
                        app_name: "MCP Batch".into(),
                        app_version: "0.1.0".into(),
                        pid: std::process::id(),
                        user_notes: Some(format!("Batch item {} of {}", idx + 1, paths.len())),
                    },
                    priority: JobPriority::Normal,
                    start_policy: StartPolicy::AutoStart,
                    source: SourceDescriptor::File { path: p, start_tick: None, duration_ticks: None, timebase: None },
                    targets: vec![target],
                };

                let assigned = queue_mgr.submit_job(spec)?;
                start_worker_unless_stopped(queue_mgr);
                queued_ids.push(assigned);
            }

            Ok(json!({ "submittedCount": queued_ids.len(), "jobIds": queued_ids }))
        }
        "encodercraft_stitch_clips" => {
            let clip_strs: Vec<String> = serde_json::from_value(args.get("clip_paths").cloned().unwrap_or(Value::Array(Vec::new())))
                .map_err(|e| EncoderError::InvalidJobSpec(format!("invalid clip_paths: {e}")))?;
            let out_str = args
                .get("output_path")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'output_path'".into()))?;
            let preset_name = args.get("preset_name").and_then(Value::as_str).unwrap_or("Match Source - High Bitrate").to_string();

            let paths = clip_strs.into_iter().map(PathBuf::from).collect();
            let job_id = queue_mgr.stitch_sources_into_job(paths, PathBuf::from(out_str), preset_name)?;
            Ok(json!({ "jobId": job_id, "status": "Queued" }))
        }
        "encodercraft_manage_queue" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing required argument 'action'".into()))?;
            let job_id = args.get("job_id").and_then(Value::as_str);

            match action {
                "start" => {
                    let running = queue_mgr.start_queue();
                    Ok(json!({ "action": "start", "running": running }))
                }
                "pause" => {
                    let id = job_id.ok_or_else(|| EncoderError::InvalidJobSpec("job_id required for pause".into()))?;
                    queue_mgr.pause_job(id)?;
                    Ok(json!({ "action": "pause", "jobId": id, "status": "paused" }))
                }
                "cancel" => {
                    let id = job_id.ok_or_else(|| EncoderError::InvalidJobSpec("job_id required for cancel".into()))?;
                    queue_mgr.cancel_job(id)?;
                    Ok(json!({ "action": "cancel", "jobId": id, "status": "cancelled" }))
                }
                "duplicate" => {
                    let id = job_id.ok_or_else(|| EncoderError::InvalidJobSpec("job_id required for duplicate".into()))?;
                    let new_id = queue_mgr.duplicate_job(id)?;
                    Ok(json!({ "action": "duplicate", "originalJobId": id, "newJobId": new_id }))
                }
                "reset" => {
                    let id = job_id.ok_or_else(|| EncoderError::InvalidJobSpec("job_id required for reset".into()))?;
                    queue_mgr.reset_job_status(id)?;
                    Ok(json!({ "action": "reset", "jobId": id, "status": "queued" }))
                }
                "clear_finished" => {
                    let count = queue_mgr.clear_completed_jobs()?;
                    Ok(json!({ "action": "clear_finished", "removedCount": count }))
                }
                other => Err(EncoderError::InvalidJobSpec(format!("unknown queue action '{other}'"))),
            }
        }
        "encodercraft_get_log" => {
            let entries = queue_mgr.list_log_entries();
            Ok(json!({ "logEntries": entries }))
        }
        "encodercraft_set_output_dir" => {
            let job_ids: Vec<String> = args
                .get("job_ids")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default();
            let output_dir = args
                .get("output_dir")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| EncoderError::InvalidJobSpec("missing 'output_dir'".into()))?;

            let count = queue_mgr.set_jobs_output_dir(&job_ids, output_dir)?;
            Ok(json!({ "modifiedCount": count }))
        }
        _ => Err(EncoderError::InvalidJobSpec(format!("unknown MCP tool '{name}'"))),
    }
}

/// Headless servers have no GUI to start the queue, so submitting a job drains it unless the user stopped it.
fn start_worker_unless_stopped(queue_mgr: &Arc<QueueManager>) {
    if !queue_mgr.is_stop_requested() {
        queue_mgr.start_background_worker();
    }
}
