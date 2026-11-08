//! Headless command-line interface and daemon runner for encoderCraft.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod stream;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, JobStatus, SourceDescriptor, StartPolicy};
use encodercraft_demux::probe_file;
use encodercraft_dispatch::DispatchClient;
use encodercraft_engine::{EngineIpcHandler, LossyConversionKey, QueueManager};
use encodercraft_ipc::IpcServer;
use encodercraft_mcp::McpServer;
use encodercraft_presets::{PresetRegistry, evaluate_preset};
use encodercraft_watch::{WatchFolderConfig, WatchFolderService};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), EncoderError> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args.get(1).map(String::as_str) {
        Some("encode") => cmd_encode(&args[2..])?,
        Some("convert") => cmd_convert(&args[2..])?,
        Some("probe") => cmd_probe(&args[2..])?,
        Some("presets") => cmd_presets()?,
        Some("--containers") | Some("containers") => print!("{}", encodercraft_video_codecs::capabilities::render_container_matrix_markdown()),
        Some("--formats") | Some("formats") => print!("{}", encodercraft_video_codecs::capabilities::render_text()),
        Some("queue") => cmd_queue(&args[2..])?,
        Some("command") => cmd_command(&args[2..])?,
        Some("watch") => cmd_watch(&args[2..])?,
        Some("daemon") => cmd_daemon(&args[2..])?,
        Some("stream-encode") => stream::cmd_stream_encode(&args[2..], &default_preset_registry())?,
        Some("stream-test") => stream::cmd_stream_test(&args[2..])?,
        Some("lut-convert") => cmd_lut_convert(&args[2..])?,
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
    println!("  stream-encode --endpoint <name> --size WxH [--fps N|N/D] [--audio RATE:CH] -o <file> [-p <preset>]");
    println!("                                          Encode frames and audio another app streams over shared memory");
    println!("  stream-test --endpoint <name> --size WxH [--fps N|N/D] [--audio RATE:CH] [--seconds N]");
    println!("                                          Stream a synthetic test pattern to a waiting stream-encode");
    println!("  encode <input> [options]                Transcode an input file directly");
    println!("      -p, --preset <name>                 Encoding preset (default: Match Source - High Bitrate)");
    println!("      -o, --output <file>                 Output file path");
    println!("      --lut <file>                        Apply 3D Look-Up Table (.cube, .3dl, .look)");
    println!("      --stems                             Export discrete multichannel audio stems (.wav)");
    println!("      --hardware                          Enable hardware acceleration (VideoToolbox / NVENC)");
    println!("      --xmp-title <title>                 Embed XMP Dublin Core title");
    println!("      --xmp-creator <creator>             Embed XMP Dublin Core author/creator");
    println!("      --sidecar-xmp                       Export external sidecar .xmp file");
    println!("      --caption <file>                    Subtitle/caption file to embed or burn");
    println!("      --fps <rate>                        Frame rate for image sequences (e.g. 25, 29.97, 30000/1001); input like shot_%04d.png");
    println!("      --rotate <0|90|180|270>              Clockwise video rotation");
    println!("      --scale-mode <fit|fill|stretch|center> Video scaling and aspect handling");
    println!("      --scale-filter <bilinear|bicubic|lanczos3> Spatial resampling quality");
    println!("      --frame-rate-mode <nearest|blend>     Frame selection when changing video frame rate");
    println!("      --frame-memory-mib <MiB>               Bounded video-frame queue budget (default 256 MiB)");
    println!("      --normalization-memory-mib <MiB>       Maximum whole-track loudness buffer (default 1024 MiB)");
    println!("      --burn-captions                     Burn captions directly into video frames");
    println!("      --sequence <name|index>             Export only this sequence/composition of a project source (see `probe`)");
    println!("      --pages <N|A-B>                     Export only these pages of a document source");
    println!("      --layers <a,b,...>                  Export only these layers of a CAD drawing or layered image");
    println!("      Lossy video conversions prompt for approval; unattended jobs require a saved allow decision");
    println!(
        "  convert <input> --to <format> [-o <file>] Convert to a target format (extension or name, e.g. pdf, flac, soundcraft); invalid pairs are rejected"
    );
    println!("  probe <input>                           Inspect containers, codecs, and stream metadata");
    println!("  lut-convert <input> <output> [--size N] [--trilinear|--tetrahedral]");
    println!("                                          Convert, resample, and transcode 1D/3D LUT (.cube, .3dl, .look)");
    println!("  presets                                 List all factory presets");
    println!("  --formats                               Show the codec decode/encode capability table");
    println!("  queue [list|start|pause|clear|cancel <id>|log] Interact with running daemon queue");
    println!("  command list                            List shared commands from a running instance");
    println!("  command execute <id> [json-params]     Execute a shared command on the running instance");
    println!("  watch <dir> -p <preset> -o <out_dir>    Start automated directory monitor");
    println!("  daemon [--port <port>]                  Run background IPC daemon");
    println!("  mcp                                     Run Model Context Protocol server over stdio");
}

/// Parses `25`, `29.97` or `30000/1001` into an exact frame rate.
fn parse_fps(text: &str) -> Result<encodercraft_core::Rational64, EncoderError> {
    let bad = || EncoderError::InvalidJobSpec(format!("invalid frame rate '{text}'"));
    let (num, den) = if let Some((n, d)) = text.split_once('/') {
        (n.trim().parse::<i64>().map_err(|_| bad())?, d.trim().parse::<i64>().map_err(|_| bad())?)
    } else {
        let value: f64 = text.trim().parse().map_err(|_| bad())?;
        if !value.is_finite() || value <= 0.0 || value > 1000.0 {
            return Err(bad());
        }
        let ntsc = [(23.976, (24_000, 1_001)), (29.97, (30_000, 1_001)), (59.94, (60_000, 1_001))];
        match ntsc.iter().find(|(v, _)| (value - v).abs() < 0.005) {
            Some((_, pair)) => *pair,
            None => ((value * 1000.0).round() as i64, 1000),
        }
    };
    if num <= 0 || den <= 0 || num > 1_000_000 || den > 1_000_000 {
        return Err(bad());
    }
    let g = gcd(num, den);
    Ok(encodercraft_core::Rational64 { num: num / g, den: den / g })
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

fn request_conversion_approvals(queue: &QueueManager, spec: &CraftJobSpec) -> Result<Vec<LossyConversionKey>, EncoderError> {
    let conversions = queue.conversion_review(spec)?;
    if conversions.is_empty() {
        return Ok(Vec::new());
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let first = conversions.first().map(|conversion| conversion.description()).unwrap_or_default();
        return Err(EncoderError::ConversionApprovalRequired(first));
    }

    let mut approved_once = Vec::new();
    for conversion in conversions {
        println!("Lossy conversion requires approval: {}", conversion.description());
        loop {
            print!("Allow once [y], always allow this format pair [a], refuse [n], always refuse [d]? ");
            std::io::stdout().flush().map_err(|error| EncoderError::Io(format!("could not display conversion prompt: {error}")))?;
            let mut answer = String::new();
            let bytes =
                std::io::stdin().read_line(&mut answer).map_err(|error| EncoderError::Io(format!("could not read conversion approval: {error}")))?;
            if bytes == 0 {
                return Err(EncoderError::ConversionApprovalRequired(conversion.description()));
            }
            match answer.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => {
                    approved_once.push(conversion.key);
                    break;
                }
                "a" | "always" => {
                    queue.save_conversion_decision(conversion.key, true)?;
                    break;
                }
                "n" | "no" => {
                    return Err(EncoderError::InvalidJobSpec(format!("conversion refused: {}", conversion.description())));
                }
                "d" | "deny" => {
                    queue.save_conversion_decision(conversion.key.clone(), false)?;
                    return Err(EncoderError::InvalidJobSpec(format!(
                        "conversion refused and saved for this format pair: {}",
                        conversion.description()
                    )));
                }
                _ => println!("Choose y, a, n, or d."),
            }
        }
    }
    Ok(approved_once)
}

fn submit_cli_job(queue: &QueueManager, spec: CraftJobSpec) -> Result<String, EncoderError> {
    let approved_once = request_conversion_approvals(queue, &spec)?;
    queue.submit_job_with_approvals(spec, &approved_once)
}

fn new_cli_queue(limits: encodercraft_pipeline::PipelineLimits) -> QueueManager {
    let queue = QueueManager::new_in_memory_with_limits(limits);
    if let Some(path) = encodercraft_engine::default_preferences_path() {
        queue.set_conversion_preferences_path(path);
    }
    queue
}

fn parse_memory_mib(option: &str, value: &str) -> Result<usize, EncoderError> {
    let mib = value.parse::<usize>().map_err(|_| EncoderError::InvalidJobSpec(format!("{option} needs a positive integer MiB value")))?;
    mib.checked_mul(1 << 20)
        .filter(|bytes| mib > 0 && *bytes <= 64 << 30)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("{option} must be between 1 and 65536 MiB")))
}

#[derive(Default)]
struct CliEncodeOptions {
    inputs: Vec<PathBuf>,
    preset_name: String,
    preset_explicit: bool,
    to: Option<encodercraft_core::ContainerType>,
    output: Option<PathBuf>,
    export_stems: bool,
    hardware_accel: bool,
    xmp_title: Option<String>,
    xmp_creator: Option<String>,
    sidecar_xmp: bool,
    caption_file: Option<PathBuf>,
    lut_file: Option<PathBuf>,
    burn_captions: bool,
    fps: Option<encodercraft_core::Rational64>,
    rotation: encodercraft_core::Rotation,
    scale_mode: encodercraft_geom::ScaleMode,
    scaling_filter: encodercraft_core::ScalingFilter,
    frame_rate_mode: encodercraft_core::FrameRateMode,
    frame_memory_bytes: Option<usize>,
    normalization_memory_bytes: Option<usize>,
    selection: encodercraft_core::SourceSelection,
}

fn parse_encode_args(args: &[String]) -> Result<CliEncodeOptions, EncoderError> {
    let mut opts = CliEncodeOptions {
        preset_name: "Match Source - High Bitrate".to_string(),
        rotation: encodercraft_core::Rotation::None,
        scale_mode: encodercraft_geom::ScaleMode::Fit,
        scaling_filter: encodercraft_core::ScalingFilter::Bilinear,
        frame_rate_mode: encodercraft_core::FrameRateMode::Nearest,
        ..Default::default()
    };

    let mut i = 0;
    while i < args.len() {
        if let Some(arg) = args.get(i) {
            match arg.as_str() {
                "-p" | "--preset" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.preset_name = val.clone();
                        opts.preset_explicit = true;
                        i += 1;
                    }
                }
                "-o" | "--output" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.output = Some(PathBuf::from(val));
                        i += 1;
                    }
                }
                "--to" => {
                    let val = args
                        .get(i + 1)
                        .ok_or_else(|| EncoderError::InvalidJobSpec("--to needs a target format such as pdf, flac or soundcraft".into()))?;
                    opts.to = Some(
                        encodercraft_core::ContainerType::from_extension(val)
                            .ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown target format '{val}'")))?,
                    );
                    i += 1;
                }
                "--fps" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--fps needs a value such as 25 or 30000/1001".into()))?;
                    opts.fps = Some(parse_fps(val)?);
                    i += 1;
                }
                "--rotate" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--rotate needs 0, 90, 180 or 270".into()))?;
                    opts.rotation = match val.as_str() {
                        "0" => encodercraft_core::Rotation::None,
                        "90" => encodercraft_core::Rotation::Cw90,
                        "180" => encodercraft_core::Rotation::Cw180,
                        "270" => encodercraft_core::Rotation::Cw270,
                        _ => return Err(EncoderError::InvalidJobSpec(format!("invalid rotation '{val}'; use 0, 90, 180 or 270"))),
                    };
                    i += 1;
                }
                "--scale-mode" => {
                    let val =
                        args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--scale-mode needs fit, fill, stretch or center".into()))?;
                    opts.scale_mode = match val.to_ascii_lowercase().as_str() {
                        "fit" => encodercraft_geom::ScaleMode::Fit,
                        "fill" => encodercraft_geom::ScaleMode::Fill,
                        "stretch" => encodercraft_geom::ScaleMode::Stretch,
                        "center" => encodercraft_geom::ScaleMode::Center,
                        _ => return Err(EncoderError::InvalidJobSpec(format!("invalid scale mode '{val}'; use fit, fill, stretch or center"))),
                    };
                    i += 1;
                }
                "--frame-rate-mode" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--frame-rate-mode needs nearest or blend".into()))?;
                    opts.frame_rate_mode = match val.to_ascii_lowercase().as_str() {
                        "nearest" => encodercraft_core::FrameRateMode::Nearest,
                        "blend" => encodercraft_core::FrameRateMode::Blend,
                        _ => return Err(EncoderError::InvalidJobSpec(format!("invalid frame-rate mode '{val}'; use nearest or blend"))),
                    };
                    i += 1;
                }
                "--frame-memory-mib" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--frame-memory-mib needs an integer value".into()))?;
                    opts.frame_memory_bytes = Some(parse_memory_mib("--frame-memory-mib", val)?);
                    i += 1;
                }
                "--normalization-memory-mib" => {
                    let val =
                        args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--normalization-memory-mib needs an integer value".into()))?;
                    opts.normalization_memory_bytes = Some(parse_memory_mib("--normalization-memory-mib", val)?);
                    i += 1;
                }
                "--sequence" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--sequence needs a name or index".into()))?;
                    opts.selection.sequence = Some(val.clone());
                    i += 1;
                }
                "--pages" => {
                    let val = args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--pages needs a page or range such as 3 or 2-5".into()))?;
                    opts.selection.pages = Some(encodercraft_core::SourceSelection::parse_pages(val)?);
                    i += 1;
                }
                "--layers" => {
                    let val =
                        args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--layers needs a comma-separated list of layer names".into()))?;
                    opts.selection.layers = Some(val.split(',').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect());
                    i += 1;
                }
                "--stems" => {
                    opts.export_stems = true;
                }
                "--hardware" => {
                    opts.hardware_accel = true;
                }
                "--xmp-title" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.xmp_title = Some(val.clone());
                        i += 1;
                    }
                }
                "--scale-filter" => {
                    let val =
                        args.get(i + 1).ok_or_else(|| EncoderError::InvalidJobSpec("--scale-filter needs bilinear, bicubic or lanczos3".into()))?;
                    opts.scaling_filter = match val.to_ascii_lowercase().as_str() {
                        "bilinear" => encodercraft_core::ScalingFilter::Bilinear,
                        "bicubic" => encodercraft_core::ScalingFilter::Bicubic,
                        "lanczos3" | "lanczos" => encodercraft_core::ScalingFilter::Lanczos3,
                        _ => {
                            return Err(EncoderError::InvalidJobSpec(format!("invalid scaling filter '{val}'; use bilinear, bicubic or lanczos3")));
                        }
                    };
                }
                "--xmp-creator" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.xmp_creator = Some(val.clone());
                        i += 1;
                    }
                }
                "--sidecar-xmp" => {
                    opts.sidecar_xmp = true;
                }
                "--caption" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.caption_file = Some(PathBuf::from(val));
                        i += 1;
                    }
                }
                "--burn-captions" => {
                    opts.burn_captions = true;
                }
                "--lut" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.lut_file = Some(PathBuf::from(val));
                        i += 1;
                    }
                }
                other if !other.starts_with('-') => {
                    opts.inputs.push(PathBuf::from(other));
                }
                _ => {}
            }
        }
        i += 1;
    }

    Ok(opts)
}

fn cmd_encode(args: &[String]) -> Result<(), EncoderError> {
    let opts = parse_encode_args(args)?;
    let inputs = opts.inputs;
    let mut preset_name = opts.preset_name;
    let preset_explicit = opts.preset_explicit;
    let to = opts.to;
    let output = opts.output;
    let export_stems = opts.export_stems;
    let hardware_accel = opts.hardware_accel;
    let xmp_title = opts.xmp_title;
    let xmp_creator = opts.xmp_creator;
    let sidecar_xmp = opts.sidecar_xmp;
    let caption_file = opts.caption_file;
    let lut_file = opts.lut_file;
    let burn_captions = opts.burn_captions;
    let fps = opts.fps;
    let rotation = opts.rotation;
    let scale_mode = opts.scale_mode;
    let scaling_filter = opts.scaling_filter;
    let frame_rate_mode = opts.frame_rate_mode;
    let frame_memory_bytes = opts.frame_memory_bytes;
    let normalization_memory_bytes = opts.normalization_memory_bytes;
    let selection = opts.selection;

    if inputs.is_empty() {
        return Err(EncoderError::InvalidJobSpec("missing input file(s)".into()));
    }

    let registry = default_preset_registry();
    if let Some(container) = to
        && !preset_explicit
        && let Some(p) = registry.list().iter().find(|p| p.container == container)
    {
        preset_name = p.name.clone();
    }
    let preset = registry.find_by_name(&preset_name).ok_or_else(|| EncoderError::InvalidJobSpec(format!("preset '{preset_name}' not found")))?;

    if let Some(target) = to {
        if let Some(out) = output.as_ref().filter(|o| total_is_single(&inputs) && !o.is_dir())
            && let Some(ext) = out.extension().and_then(|e| e.to_str())
            && encodercraft_core::ContainerType::from_extension(ext).is_some_and(|c| c != target)
        {
            return Err(EncoderError::InvalidJobSpec(format!("output extension '.{ext}' does not match --to {}", target.file_extension())));
        }
        for input in &inputs {
            let source = input.extension().and_then(|e| e.to_str()).and_then(encodercraft_core::ContainerType::from_extension);
            if let Some(source) = source
                && source != target
                && !source.can_convert_to(target)
            {
                let valid: Vec<&str> = source.valid_conversion_targets().iter().map(|c| c.file_extension()).collect();
                return Err(EncoderError::InvalidJobSpec(format!(
                    "cannot convert {} to {}; valid targets: {}",
                    input.display(),
                    target.file_extension(),
                    valid.join(", ")
                )));
            }
        }
    }

    let total = inputs.len();
    if total > 1 {
        println!("Starting batch transcode: {total} files using preset '{preset_name}'");
    }

    let mut pipeline_limits = encodercraft_pipeline::PipelineLimits::default();
    if let Some(bytes) = frame_memory_bytes {
        pipeline_limits.queue_bytes = bytes;
    }
    if let Some(bytes) = normalization_memory_bytes {
        pipeline_limits.audio_bytes = bytes;
    }
    let queue = Arc::new(new_cli_queue(pipeline_limits));
    let mut submitted: Vec<(String, PathBuf)> = Vec::new();

    for (idx, input_path) in inputs.iter().enumerate() {
        let default_ext = match to {
            Some(c) => c.file_extension(),
            None if is_session_path(input_path) => "wav",
            None => "mp4",
        };
        let output_path = if total == 1
            && let Some(ref out) = output
        {
            out.clone()
        } else if let Some(ref out_dir) = output
            && out_dir.is_dir()
        {
            let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            out_dir.join(format!("{stem}_encoded.{default_ext}"))
        } else {
            let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            PathBuf::from(format!("{stem}_encoded.{default_ext}"))
        };

        if total > 1 {
            println!("\n[{}/{}] Probing: {}", idx + 1, total, input_path.display());
        } else {
            println!("Probing source: {}", input_path.display());
        }

        let media_info = encodercraft_source::probe_media_info(input_path)?;
        let mut target = evaluate_preset(preset, &media_info, output_path.clone())?;
        if let Some(container) = to {
            target.container = container;
        } else if let Some(container) = session_output_container(input_path, &output_path) {
            target.container = container;
        } else if let Some(container) = project_output_container(input_path, &output_path)? {
            target.container = container;
        }

        target.selection = selection.clone();
        target.transform = encodercraft_core::VideoTransform {
            scale_mode,
            scaling_filter,
            rotation,
            flip: encodercraft_core::FlipMode::None,
            deinterlace: encodercraft_core::DeinterlaceMode::None,
            par_conform: encodercraft_core::PixelAspectRatioConform::Source,
            frame_rate_mode,
        };
        if let Some(rate) = fps {
            target.custom_frame_rate = Some(rate);
        }
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
        if let Some(ref lut) = lut_file {
            target.lut_path = Some(lut.clone());
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

        println!("Queued: preset='{}' ➔ {}", preset_name, output_path.display());
        submitted.push((spec.job_id.clone(), output_path));
        submit_cli_job(&queue, spec)?;
    }

    queue.start_queue();
    wait_for_jobs(&queue, &submitted)
}

fn total_is_single(inputs: &[PathBuf]) -> bool {
    inputs.len() == 1
}

/// `convert <input> --to <format>`: validates the source/target pair, then runs the regular encode path.
fn cmd_convert(args: &[String]) -> Result<(), EncoderError> {
    if !args.iter().any(|a| a == "--to") {
        return Err(EncoderError::InvalidJobSpec("convert requires --to <format> (e.g. --to pdf)".into()));
    }
    cmd_encode(args)
}

/// `lut-convert <input> <output> [--size N] [--trilinear|--tetrahedral]`: converts and resamples 1D/3D Look-Up Tables.
fn cmd_lut_convert(args: &[String]) -> Result<(), EncoderError> {
    if args.is_empty() {
        eprintln!("Usage: encodercraft lut-convert <input.cube|.3dl|.look> <output.cube|.3dl|.look> [--size N] [--trilinear|--tetrahedral]");
        return Ok(());
    }
    let input_path = Path::new(&args[0]);
    let output_path = if args.len() > 1 && !args[1].starts_with('-') {
        Path::new(&args[1])
    } else {
        return Err(EncoderError::InvalidJobSpec("output path required for lut-convert".into()));
    };

    let mut target_size = None;
    let mut mode = encodercraft_color::InterpolationMode::Tetrahedral;

    let mut i = 2;
    while i < args.len() {
        if let Some(arg) = args.get(i) {
            match arg.as_str() {
                "--size" => {
                    if let Some(s) = args.get(i + 1) {
                        target_size = Some(s.parse::<usize>().map_err(|e| EncoderError::InvalidJobSpec(format!("invalid --size: {e}")))?);
                        i += 1;
                    }
                }
                "--trilinear" => mode = encodercraft_color::InterpolationMode::Trilinear,
                "--tetrahedral" => mode = encodercraft_color::InterpolationMode::Tetrahedral,
                _ => {}
            }
        }
        i += 1;
    }

    let text =
        std::fs::read_to_string(input_path).map_err(|e| EncoderError::Io(format!("failed to read input LUT `{}`: {e}", input_path.display())))?;
    let mut lut = encodercraft_color::Lut3D::parse_auto(&text)?;

    if let Some(new_size) = target_size {
        lut = lut.resample(new_size, mode)?;
    }

    let ext = output_path.extension().and_then(|e| e.to_str()).unwrap_or("cube").to_lowercase();
    let out_str = match ext.as_str() {
        "3dl" => lut.to_3dl(),
        "look" => lut.to_look(),
        _ => lut.to_cube(),
    };

    std::fs::write(output_path, out_str).map_err(|e| EncoderError::Io(format!("failed to write output LUT `{}`: {e}", output_path.display())))?;
    println!("Successfully converted LUT to {} (size {}x{}x{})", output_path.display(), lut.size, lut.size, lut.size);
    Ok(())
}

fn is_session_path(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sesx") || e.eq_ignore_ascii_case("soundcraft"))
}

/// For DAW session inputs the output container follows the `-o` extension (audio masters or another session format).
fn session_output_container(input: &std::path::Path, output: &std::path::Path) -> Option<encodercraft_core::ContainerType> {
    use encodercraft_core::ContainerType;
    if !is_session_path(input) {
        return None;
    }
    let ext = output.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)?;
    if ext == "aiff" {
        return Some(ContainerType::Aiff);
    }
    [ContainerType::Wave, ContainerType::Flac, ContainerType::Aiff, ContainerType::AdobeAuditionSesx, ContainerType::ArtcraftSoundcraft]
        .into_iter()
        .find(|c| c.file_extension() == ext)
        .or(Some(ContainerType::Wave))
}

/// Project, document, CAD and layered sources are not media: without `--to`, the `-o` extension picks the output
/// container, and it must be a valid conversion target for the source.
fn project_output_container(input: &std::path::Path, output: &std::path::Path) -> Result<Option<encodercraft_core::ContainerType>, EncoderError> {
    use encodercraft_core::{ContainerType, MediaStreamKind};
    let Some(source) = input.extension().and_then(|e| e.to_str()).and_then(ContainerType::from_extension) else {
        return Ok(None);
    };
    let kind = source.media_kind();
    let non_media = matches!(
        kind,
        MediaStreamKind::ProjectTimeline
            | MediaStreamKind::MotionGraph
            | MediaStreamKind::Cad2D
            | MediaStreamKind::LayeredComposite
            | MediaStreamKind::Document
    );
    if !non_media {
        return Ok(None);
    }
    let Some(ext) = output.extension().and_then(|e| e.to_str()) else {
        return Ok(Some(source));
    };
    // Interchange formats are written by the timeline exporter, which keys off the output extension.
    if kind == MediaStreamKind::ProjectTimeline && (ext.eq_ignore_ascii_case("fcpxml") || ext.eq_ignore_ascii_case("otio")) {
        return Ok(Some(ContainerType::AdobePremierePrproj));
    }
    let target = ContainerType::from_extension(ext)
        .ok_or_else(|| EncoderError::InvalidJobSpec(format!("unknown output format '.{ext}' for {}", input.display())))?;
    if target != source && !source.can_convert_to(target) {
        let valid: Vec<&str> = source.valid_conversion_targets().iter().map(|c| c.file_extension()).collect();
        return Err(EncoderError::InvalidJobSpec(format!("cannot convert {} to .{ext}; valid targets: {}", input.display(), valid.join(", "))));
    }
    Ok(Some(target))
}

/// Blocks until every submitted job is finished, printing progress; fails if any job failed.
fn wait_for_jobs(queue: &QueueManager, submitted: &[(String, PathBuf)]) -> Result<(), EncoderError> {
    let mut reported = vec![false; submitted.len()];
    let mut failures = Vec::new();
    loop {
        let jobs = queue.list_jobs();
        let mut pending = false;
        for (idx, (id, out)) in submitted.iter().enumerate() {
            let Some(job) = jobs.iter().find(|j| &j.spec.job_id == id) else { continue };
            match &job.status {
                JobStatus::Completed | JobStatus::Failed(_) | JobStatus::Cancelled => {
                    if let Some(flag) = reported.get_mut(idx)
                        && !*flag
                    {
                        *flag = true;
                        match &job.status {
                            JobStatus::Completed => println!("Complete: {}", out.display()),
                            JobStatus::Failed(e) => {
                                eprintln!("Failed: {}: {e}", out.display());
                                failures.push(e.clone());
                            }
                            _ => {
                                eprintln!("Cancelled: {}", out.display());
                                failures.push("cancelled".to_string());
                            }
                        }
                    }
                }
                JobStatus::Processing => {
                    pending = true;
                    let eta = job.target_eta_seconds.map(|seconds| format!(" | Target ETA {seconds}s")).unwrap_or_default();
                    eprint!("\r[{}/{}] {:>3.0}%{eta}", idx + 1, submitted.len(), job.progress_percent);
                }
                _ => pending = true,
            }
        }
        if !pending {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    match failures.first() {
        Some(first) => Err(EncoderError::Io(format!("{} of {} job(s) failed: {first}", failures.len(), submitted.len()))),
        None => Ok(()),
    }
}

fn cmd_probe(args: &[String]) -> Result<(), EncoderError> {
    let input = args.first().ok_or_else(|| EncoderError::InvalidJobSpec("probe command requires an input file path".into()))?;

    let inventory = encodercraft_pipeline::inspect_source(Path::new(input));
    // Project, document, CAD and layered sources have no media streams; their inventory is the probe result.
    match probe_file(Path::new(input)) {
        Ok(info) => {
            let json = serde_json::to_string_pretty(&info).map_err(|e| EncoderError::Io(format!("json format error: {e}")))?;
            println!("{json}");
        }
        Err(e) if inventory.is_empty() => return Err(e),
        Err(_) => {}
    }
    for seq in &inventory.sequences {
        let duration = seq.duration_secs.map(|d| format!(", {d:.2}s")).unwrap_or_default();
        println!("sequence[{}]: {} ({}x{} @ {} fps{duration})", seq.index, seq.name, seq.width, seq.height, seq.fps);
    }
    if let Some(pages) = inventory.pages {
        println!("pages: {pages}");
    }
    if !inventory.layers.is_empty() {
        println!("layers: {}", inventory.layers.join(", "));
    }
    Ok(())
}

fn cmd_presets() -> Result<(), EncoderError> {
    let registry = default_preset_registry();
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

fn cmd_command(args: &[String]) -> Result<(), EncoderError> {
    let client = DispatchClient::connect_default();
    match args.first().map(String::as_str) {
        Some("list") => {
            let commands = client.list_commands()?;
            println!("{}", serde_json::to_string_pretty(&commands).map_err(|error| EncoderError::Internal(error.to_string()))?);
        }
        Some("execute") => {
            let command_id = args.get(1).ok_or_else(|| EncoderError::InvalidJobSpec("missing command ID".into()))?;
            let params = match args.get(2) {
                Some(text) => {
                    serde_json::from_str(text).map_err(|error| EncoderError::InvalidJobSpec(format!("invalid JSON parameters: {error}")))?
                }
                None => serde_json::json!({}),
            };
            let result = client.execute_command(command_id, params)?;
            println!("{}", serde_json::to_string_pretty(&result).map_err(|error| EncoderError::Internal(error.to_string()))?);
        }
        _ => return Err(EncoderError::InvalidJobSpec("usage: encodercraft command list | command execute <id> [json-params]".into())),
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

    let queue = Arc::new(QueueManager::new_in_memory());
    if let Some(path) = encodercraft_engine::default_preferences_path() {
        queue.set_conversion_preferences_path(path);
    }
    let mut service = WatchFolderService::new(default_preset_registry());
    service.add_config(WatchFolderConfig::new("cli-watch", watch_dir.clone(), out_dir, preset));

    println!("Watching directory: {} (Press Ctrl+C to stop)", watch_dir.display());
    loop {
        if let Ok(jobs) = service.scan_once() {
            for (wf_id, spec) in jobs {
                println!("Ingested job from {wf_id}: {}", spec.job_id);
                match queue.submit_job(spec) {
                    Ok(_) => {
                        queue.start_queue();
                    }
                    Err(e) => eprintln!("Could not queue job from {wf_id}: {e}"),
                }
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
    if let Some(path) = encodercraft_engine::default_preferences_path() {
        queue_mgr.set_conversion_preferences_path(path);
    }
    let preset_registry = Arc::new(Mutex::new(default_preset_registry()));

    let handler = Arc::new(EngineIpcHandler::new(queue_mgr.clone(), preset_registry));

    let addr = format!("127.0.0.1:{port}");
    println!("Starting encoderCraft IPC daemon on {addr}...");
    let _server_tcp = IpcServer::bind_tcp(&addr, handler.clone())?;

    #[cfg(unix)]
    let _server_unix = {
        let sock_file = encodercraft_dispatch::default_socket_path();
        let _ = std::fs::remove_file(&sock_file);
        IpcServer::bind_unix(&sock_file, handler.clone()).ok()
    };

    println!("Daemon running. Press Ctrl+C to exit.");
    loop {
        // Drain newly submitted jobs; a running worker is left untouched.
        if !queue_mgr.is_stop_requested() {
            queue_mgr.start_background_worker();
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn cmd_mcp() -> Result<(), EncoderError> {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    if let Some(path) = encodercraft_engine::default_preferences_path() {
        queue_mgr.set_conversion_preferences_path(path);
    }
    let preset_registry = Arc::new(Mutex::new(default_preset_registry()));
    let server = McpServer::new(queue_mgr, preset_registry);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    server.run_stdio(stdin.lock(), stdout.lock()).map_err(|e| EncoderError::Io(format!("MCP stdio error: {e}")))
}

fn default_preset_registry() -> PresetRegistry {
    PresetRegistry::new(encodercraft_engine::default_config_dir().map(|dir| dir.join("presets")))
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use encodercraft_core::ContainerType;
    use std::path::Path;

    #[test]
    fn session_output_follows_extension() {
        let s = Path::new("mix.sesx");
        assert_eq!(session_output_container(s, Path::new("o.flac")), Some(ContainerType::Flac));
        assert_eq!(session_output_container(s, Path::new("o.aiff")), Some(ContainerType::Aiff));
        assert_eq!(session_output_container(s, Path::new("o.soundcraft")), Some(ContainerType::ArtcraftSoundcraft));
        assert_eq!(session_output_container(s, Path::new("o.xyz")), Some(ContainerType::Wave));
        assert_eq!(session_output_container(Path::new("a.mov"), Path::new("o.flac")), None);
    }

    #[test]
    fn memory_limit_parser_accepts_only_bounded_positive_mib_values() {
        assert_eq!(parse_memory_mib("--frame-memory-mib", "1").unwrap(), 1 << 20);
        assert!(parse_memory_mib("--frame-memory-mib", "0").is_err());
        assert!(parse_memory_mib("--frame-memory-mib", "65537").is_err());
        assert!(parse_memory_mib("--frame-memory-mib", "large").is_err());
    }
}
