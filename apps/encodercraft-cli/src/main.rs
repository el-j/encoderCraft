//! Headless command-line interface and daemon runner for encoderCraft.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, SourceDescriptor, StartPolicy};
use encodercraft_demux::probe_file;
use encodercraft_dispatch::{DispatchClient, IpcRequest, IpcResponse};
use encodercraft_engine::QueueManager;
use encodercraft_ipc::{IpcHandler, IpcServer};
use encodercraft_mcp::McpServer;
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_presets::{PresetRegistry, evaluate_preset};
use encodercraft_watch::{WatchFolderConfig, WatchFolderService};
use serde_json::json;

struct EngineIpcHandler {
    queue_mgr: Arc<QueueManager>,
    preset_registry: Arc<Mutex<PresetRegistry>>,
}

impl IpcHandler for EngineIpcHandler {
    fn handle(&self, req: &IpcRequest) -> IpcResponse {
        match req.method.as_str() {
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
            "presets.list" => {
                let reg = self.preset_registry.lock().unwrap_or_else(PoisonError::into_inner);
                let presets = reg.list();
                IpcResponse::success(req.id, json!(presets))
            }
            _ => IpcResponse::error(req.id, format!("unknown method '{}'", req.method)),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args.get(1).map(String::as_str) {
        Some("encode") => cmd_encode(&args[2..])?,
        Some("probe") => cmd_probe(&args[2..])?,
        Some("presets") => cmd_presets()?,
        Some("queue") => cmd_queue(&args[2..])?,
        Some("watch") => cmd_watch(&args[2..])?,
        Some("daemon") => cmd_daemon(&args[2..])?,
        Some("mcp") => cmd_mcp()?,
        Some("--help") | Some("-h") | Some("help") => print_usage(),
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}\n");
            print_usage();
        }
        None => print_usage(),
    }

    Ok(())
}

fn print_usage() {
    println!("encoderCraft CLI — Pure-Rust Universal Media Transcoding & Batch Queue Engine");
    println!("\nUsage: encodercraft <command> [options]");
    println!("\nCommands:");
    println!("  encode <input> [options]                Transcode an input file directly");
    println!("      -p, --preset <name>                 Encoding preset (default: Match Source - High Bitrate)");
    println!("      -o, --output <file>                 Output file path");
    println!("      --stems                             Export discrete multichannel audio stems (.wav)");
    println!("      --hardware                          Enable hardware acceleration (VideoToolbox / NVENC)");
    println!("      --xmp-title <title>                 Embed XMP Dublin Core title");
    println!("      --xmp-creator <creator>             Embed XMP Dublin Core author/creator");
    println!("      --sidecar-xmp                       Export external sidecar .xmp file");
    println!("      --caption <file>                    Subtitle/caption file to embed or burn");
    println!("      --burn-captions                     Burn captions directly into video frames");
    println!("  probe <input>                           Inspect containers, codecs, and stream metadata");
    println!("  presets                                 List all factory presets");
    println!("  queue [list|cancel <id>]                Interact with running daemon queue");
    println!("  watch <dir> -p <preset> -o <out_dir>    Start automated directory monitor");
    println!("  daemon [--port <port>]                  Run background IPC daemon");
    println!("  mcp                                     Run Model Context Protocol server over stdio");
}

fn cmd_encode(args: &[String]) -> Result<(), EncoderError> {
    let mut input = None;
    let mut preset_name = "Match Source - High Bitrate".to_string();
    let mut output = None;
    let mut export_stems = false;
    let mut hardware_accel = false;
    let mut xmp_title = None;
    let mut xmp_creator = None;
    let mut sidecar_xmp = false;
    let mut caption_file = None;
    let mut burn_captions = false;

    let mut i = 0;
    while i < args.len() {
        if let Some(arg) = args.get(i) {
            match arg.as_str() {
                "-p" | "--preset" => {
                    if let Some(val) = args.get(i + 1) {
                        preset_name = val.clone();
                        i += 1;
                    }
                }
                "-o" | "--output" => {
                    if let Some(val) = args.get(i + 1) {
                        output = Some(PathBuf::from(val));
                        i += 1;
                    }
                }
                "--stems" => {
                    export_stems = true;
                }
                "--hardware" => {
                    hardware_accel = true;
                }
                "--xmp-title" => {
                    if let Some(val) = args.get(i + 1) {
                        xmp_title = Some(val.clone());
                        i += 1;
                    }
                }
                "--xmp-creator" => {
                    if let Some(val) = args.get(i + 1) {
                        xmp_creator = Some(val.clone());
                        i += 1;
                    }
                }
                "--sidecar-xmp" => {
                    sidecar_xmp = true;
                }
                "--caption" => {
                    if let Some(val) = args.get(i + 1) {
                        caption_file = Some(PathBuf::from(val));
                        i += 1;
                    }
                }
                "--burn-captions" => {
                    burn_captions = true;
                }
                other if !other.starts_with('-') && input.is_none() => {
                    input = Some(PathBuf::from(other));
                }
                _ => {}
            }
        }
        i += 1;
    }

    let input_path = input.ok_or_else(|| EncoderError::InvalidJobSpec("missing input file".into()))?;
    let output_path = output.unwrap_or_else(|| {
        let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        PathBuf::from(format!("{stem}_encoded.mp4"))
    });

    println!("Probing source: {}", input_path.display());
    let media_info = probe_file(&input_path)?;

    let registry = PresetRegistry::default();
    let preset = registry.find_by_name(&preset_name).ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{preset_name}' not found")))?;

    let mut target = evaluate_preset(preset, &media_info, output_path.clone())?;

    if export_stems {
        target.audio_stems = Some(encodercraft_core::AudioStemMode::DiscreteMono);
    }
    if hardware_accel {
        target.hardware_acceleration = true;
    }
    if xmp_title.is_some() || xmp_creator.is_some() || sidecar_xmp {
        let meta = encodercraft_core::XmpMetadata { title: xmp_title, creator: xmp_creator, export_sidecar_xmp: sidecar_xmp, ..Default::default() };
        target.metadata = Some(meta);
    }
    if let Some(cap) = caption_file {
        target.caption_path = Some(cap);
        target.burn_captions = burn_captions;
    }

    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: format!("cli-encode-{}", std::process::id()),
        client_info: ClientInfo { app_name: "encoderCraft CLI".into(), app_version: "0.1.0".into(), pid: std::process::id(), user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: input_path, start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![target],
    };

    println!("Starting transcode: preset='{}' ➔ {}", preset_name, output_path.display());
    let pipeline = TranscodePipeline::new(spec);
    pipeline.execute()?;
    println!("Encoding complete: {}", output_path.display());
    Ok(())
}

fn cmd_probe(args: &[String]) -> Result<(), EncoderError> {
    let input = args.first().ok_or_else(|| EncoderError::InvalidJobSpec("probe command requires an input file path".into()))?;

    let info = probe_file(Path::new(input))?;
    let json = serde_json::to_string_pretty(&info).map_err(|e| EncoderError::Io(format!("json format error: {e}")))?;
    println!("{json}");
    Ok(())
}

fn cmd_presets() -> Result<(), EncoderError> {
    let registry = PresetRegistry::default();
    println!("Available Presets ({}):", registry.list().len());
    for p in registry.list() {
        println!("  - [{:?}] {} (Container: {:?})", p.category, p.name, p.container);
        println!("    {}", p.description);
    }
    Ok(())
}

fn cmd_queue(args: &[String]) -> Result<(), EncoderError> {
    let client = DispatchClient::connect_default();
    match args.first().map(String::as_str) {
        Some("cancel") => {
            let job_id = args.get(1).ok_or_else(|| EncoderError::InvalidJobSpec("missing job ID to cancel".into()))?;
            client.cancel_job(job_id)?;
            println!("Job '{job_id}' cancelled.");
        }
        _ => {
            println!("Querying active queue daemon...");
            // Default queue status check
        }
    }
    Ok(())
}

fn cmd_watch(args: &[String]) -> Result<(), EncoderError> {
    let watch_dir = args.first().map(PathBuf::from).ok_or_else(|| EncoderError::InvalidJobSpec("missing watch directory".into()))?;

    let mut out_dir = PathBuf::from("output");
    let mut preset = "Match Source - High Bitrate".to_string();

    let mut i = 1;
    while i < args.len() {
        if let Some(arg) = args.get(i) {
            match arg.as_str() {
                "-p" | "--preset" => {
                    if let Some(val) = args.get(i + 1) {
                        preset = val.clone();
                        i += 1;
                    }
                }
                "-o" | "--output" => {
                    if let Some(val) = args.get(i + 1) {
                        out_dir = PathBuf::from(val);
                        i += 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    let mut service = WatchFolderService::new(PresetRegistry::default());
    service.add_config(WatchFolderConfig::new("cli-watch", watch_dir.clone(), out_dir, preset));

    println!("Watching directory: {} (Press Ctrl+C to stop)", watch_dir.display());
    loop {
        if let Ok(jobs) = service.scan_once() {
            for (wf_id, spec) in jobs {
                println!("Ingested job from {wf_id}: {}", spec.job_id);
                let pipeline = TranscodePipeline::new(spec);
                let _ = pipeline.execute();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn cmd_daemon(args: &[String]) -> Result<(), EncoderError> {
    let mut port = 8998;
    if let Some(p_idx) = args.iter().position(|a| a == "--port" || a == "-p")
        && let Some(val) = args.get(p_idx + 1)
        && let Ok(parsed) = val.parse::<u16>()
    {
        port = parsed;
    }

    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));

    let handler = Arc::new(EngineIpcHandler { queue_mgr: queue_mgr.clone(), preset_registry });

    let addr = format!("127.0.0.1:{port}");
    println!("Starting encoderCraft IPC daemon on {addr}...");
    let _server = IpcServer::bind_tcp(&addr, handler)?;

    println!("Daemon running. Press Ctrl+C to exit.");
    loop {
        // Run queue processor in background
        let _ = queue_mgr.process_next_job();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cmd_mcp() -> Result<(), EncoderError> {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));
    let server = McpServer::new(queue_mgr, preset_registry);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    server.run_stdio(stdin.lock(), stdout.lock()).map_err(|e| EncoderError::Io(format!("MCP stdio error: {e}")))
}
