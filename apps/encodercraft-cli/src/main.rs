//! Headless command-line interface and daemon runner for encoderCraft.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, SourceDescriptor, StartPolicy};
use encodercraft_demux::probe_file;
use encodercraft_dispatch::DispatchClient;
use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_ipc::IpcServer;
use encodercraft_mcp::McpServer;
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_presets::{PresetRegistry, evaluate_preset};
use encodercraft_watch::{WatchFolderConfig, WatchFolderService};

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
        Some("--version") | Some("-V") => println!("encodercraft-cli {}", env!("CARGO_PKG_VERSION")),
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
    println!("  queue [list|start|pause|clear|cancel <id>|log] Interact with running daemon queue");
    println!("  watch <dir> -p <preset> -o <out_dir>    Start automated directory monitor");
    println!("  daemon [--port <port>]                  Run background IPC daemon");
    println!("  mcp                                     Run Model Context Protocol server over stdio");
}

fn cmd_encode(args: &[String]) -> Result<(), EncoderError> {
    let mut inputs: Vec<PathBuf> = Vec::new();
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
                other if !other.starts_with('-') => {
                    inputs.push(PathBuf::from(other));
                }
                _ => {}
            }
        }
        i += 1;
    }

    if inputs.is_empty() {
        return Err(EncoderError::InvalidJobSpec("missing input file(s)".into()));
    }

    let registry = PresetRegistry::default();
    let preset = registry.find_by_name(&preset_name).ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{preset_name}' not found")))?;

    let total = inputs.len();
    if total > 1 {
        println!("Starting batch transcode: {total} files using preset '{preset_name}'");
    }

    for (idx, input_path) in inputs.iter().enumerate() {
        let output_path = if total == 1
            && let Some(ref out) = output
        {
            out.clone()
        } else if let Some(ref out_dir) = output
            && out_dir.is_dir()
        {
            let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            out_dir.join(format!("{stem}_encoded.mp4"))
        } else {
            let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            PathBuf::from(format!("{stem}_encoded.mp4"))
        };

        if total > 1 {
            println!("\n[{}/{}] Probing: {}", idx + 1, total, input_path.display());
        } else {
            println!("Probing source: {}", input_path.display());
        }

        let media_info = probe_file(input_path)?;
        let mut target = evaluate_preset(preset, &media_info, output_path.clone())?;

        if export_stems {
            target.audio_stems = Some(encodercraft_core::AudioStemMode::DiscreteMono);
        }
        if hardware_accel {
            target.hardware_acceleration = true;
        }
        if xmp_title.is_some() || xmp_creator.is_some() || sidecar_xmp {
            let meta = encodercraft_core::XmpMetadata {
                title: xmp_title.clone(),
                creator: xmp_creator.clone(),
                export_sidecar_xmp: sidecar_xmp,
                ..Default::default()
            };
            target.metadata = Some(meta);
        }
        if let Some(ref cap) = caption_file {
            target.caption_path = Some(cap.clone());
            target.burn_captions = burn_captions;
        }

        let spec = CraftJobSpec {
            version: "1.0".into(),
            job_id: format!("cli-encode-{}-{}", std::process::id(), idx + 1),
            client_info: ClientInfo { app_name: "encoderCraft CLI".into(), app_version: "0.1.0".into(), pid: std::process::id(), user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: input_path.clone(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![target],
        };

        println!("Transcoding: preset='{}' ➔ {}", preset_name, output_path.display());
        let pipeline = TranscodePipeline::new(spec);
        pipeline.execute()?;
        println!("Complete: {}", output_path.display());
    }

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
        Some("start") => {
            client.start_queue()?;
            println!("Queue processing triggered.");
        }
        Some("pause") => {
            let job_id = args.get(1).map(String::as_str);
            client.pause_queue(job_id)?;
            println!("Queue/job paused.");
        }
        Some("clear") => {
            let removed = client.clear_finished()?;
            println!("Cleared {removed} completed jobs from queue.");
        }
        Some("cancel") => {
            let job_id = args.get(1).ok_or_else(|| EncoderError::InvalidJobSpec("missing job ID to cancel".into()))?;
            client.cancel_job(job_id)?;
            println!("Job '{job_id}' cancelled.");
        }
        Some("log") => {
            let res = client.get_log()?;
            if let Some(entries) = res.as_array() {
                println!("encoderCraft Transcode Log ({} events):\n", entries.len());
                for (idx, e) in entries.iter().enumerate() {
                    let status = e.get("status").and_then(|v| v.as_str()).unwrap_or("Unknown");
                    let src = e.get("source_name").and_then(|v| v.as_str()).unwrap_or("media");
                    let preset = e.get("preset_name").and_then(|v| v.as_str()).unwrap_or("preset");
                    let ms = e.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    let sec = ms as f64 / 1000.0;
                    println!(" [{:>2}] [{:<9}] {:<24} ➔ {:<18} ({:.2}s)", idx + 1, status, src, preset, sec);
                }
            } else {
                println!("No transcode log records available.");
            }
        }
        _ => {
            let res = client.list_jobs()?;
            if let Some(jobs) = res.as_array() {
                println!("encoderCraft Queue: {} jobs total\n", jobs.len());
                for (idx, j) in jobs.iter().enumerate() {
                    let job_id = j.get("spec").and_then(|s| s.get("jobId")).and_then(|v| v.as_str()).unwrap_or("unknown");
                    let app =
                        j.get("spec").and_then(|s| s.get("clientInfo")).and_then(|c| c.get("appName")).and_then(|v| v.as_str()).unwrap_or("client");
                    let status = j.get("status").and_then(|v| v.as_str()).unwrap_or("Queued");
                    let progress = j.get("progressPercent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    println!(" [{:>2}] ID: {:<24} App: {:<14} Status: {:<10} Progress: {:>3.0}%", idx + 1, job_id, app, status, progress);
                }
            } else {
                println!("Queue is empty or daemon unreachable.");
            }
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

    let handler = Arc::new(EngineIpcHandler::new(queue_mgr.clone(), preset_registry));

    let addr = format!("127.0.0.1:{port}");
    println!("Starting encoderCraft IPC daemon on {addr}...");
    let _server_tcp = IpcServer::bind_tcp(&addr, handler.clone())?;

    #[cfg(unix)]
    let _server_unix = {
        let sock_path = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let sock_file = PathBuf::from(sock_path).join("encodercraft.sock");
        let _ = std::fs::remove_file(&sock_file);
        IpcServer::bind_unix(&sock_file, handler.clone()).ok()
    };

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
