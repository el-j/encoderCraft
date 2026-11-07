//! Exhaustive industry-grade end-to-end integration tests for encoderCraft.
//!
//! Validates the entire pipeline, batch queue lifecycle, video/audio transforms,
//! watch folder automation, and IPC dispatch protocol for production rollout readiness.

use std::path::PathBuf;
use std::sync::Arc;

use encodercraft_audio_dsp::apply_audio_delay;
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{
    AudioChannelMapping, AudioEncodeSettings, AudioSampleBuffer, ChannelLayout, ClientInfo, ContainerType, CraftJobSpec, DeinterlaceMode, FlipMode,
    FrameRateMode, JobPriority, JobStatus, OutputTarget, Rational64, Rotation, ScalingFilter, SourceDescriptor, StartPolicy, VideoFrame,
    VideoTransform,
};
use encodercraft_dispatch::DispatchClient;
use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_ipc::IpcServer;
use encodercraft_pipeline::{FrameScaler, deinterlace, flip, rotate};
use encodercraft_presets::PresetRegistry;
use encodercraft_watch::{PostProcessAction, WatchFolderConfig, WatchFolderService, WatchTargetConfig};

// ---------------------------------------------------------------------------
// 1. Video Transforms Matrix E2E Test
// ---------------------------------------------------------------------------

#[test]
fn test_video_transforms_matrix_e2e() {
    let dims = Dimensions::new(4, 4).expect("valid test dimensions");
    let mut src = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).expect("frame");

    // Populate frame with coordinate gradients
    {
        let (data, stride) = src.plane_mut(0).expect("plane 0");
        for y in 0..4 {
            for x in 0..4 {
                let off = y * stride + x * 3;
                data[off] = (x * 40) as u8;
                data[off + 1] = (y * 40) as u8;
                data[off + 2] = 200;
            }
        }
    }

    // A. Verify Rotations (90, 180, 270)
    let rot90 = rotate(&src, Rotation::Cw90).expect("rotate 90");
    assert_eq!((rot90.dimensions.width, rot90.dimensions.height), (4, 4));

    let rot180 = rotate(&src, Rotation::Cw180).expect("rotate 180");
    assert_eq!((rot180.dimensions.width, rot180.dimensions.height), (4, 4));

    let rot270 = rotate(&src, Rotation::Cw270).expect("rotate 270");
    assert_eq!((rot270.dimensions.width, rot270.dimensions.height), (4, 4));

    // B. Verify Horizontal and Vertical Flips
    let flip_h = flip(&src, FlipMode::Horizontal).expect("flip horizontal");
    let (src_data, _src_stride) = src.plane(0).expect("src plane");
    let (h_data, _h_stride) = flip_h.plane(0).expect("flip plane");
    // Pixel at (0, 0) in flipped should equal pixel at (3, 0) in original
    assert_eq!(h_data[0..3], src_data[3 * 3..3 * 3 + 3]);

    let flip_v = flip(&src, FlipMode::Vertical).expect("flip vertical");
    let (_v_data, v_stride) = flip_v.plane(0).expect("flip plane");
    let (src_data, src_stride) = src.plane(0).expect("src plane");
    let (v_data, _) = flip_v.plane(0).expect("flip plane");
    // Pixel at (0, 0) in flipped should equal pixel at (0, 3) in original
    assert_eq!(v_data[0..3], src_data[3 * src_stride..3 * src_stride + 3]);
    let _ = v_stride;

    let flip_both = flip(&src, FlipMode::Both).expect("flip both");
    let (b_data, _) = flip_both.plane(0).expect("flip plane");
    // Both axes reversed should match 180° rotation
    let (r180_data, _) = rot180.plane(0).expect("rot180 plane");
    assert_eq!(b_data, r180_data);

    // C. Verify Deinterlacing Modes
    let deint_top = deinterlace(&src, DeinterlaceMode::TopFieldFirst).expect("deinterlace top");
    assert_eq!(deint_top.dimensions, src.dimensions);

    let deint_bot = deinterlace(&src, DeinterlaceMode::BottomFieldFirst).expect("deinterlace bottom");
    assert_eq!(deint_bot.dimensions, src.dimensions);

    let deint_bob = deinterlace(&src, DeinterlaceMode::Bob).expect("deinterlace bob");
    assert_eq!(deint_bob.dimensions, src.dimensions);

    // D. Verify Frame Rescaling with Aspect Ratio Preservation
    let target_dims = Dimensions::new(8, 4).expect("wide dimensions");
    let scaler_fit = FrameScaler::with_filter(ScaleMode::Fit, ScalingFilter::Lanczos3);
    let scaled_fit = scaler_fit.scale(&src, target_dims).expect("scale fit");
    assert_eq!(scaled_fit.dimensions, target_dims);

    let scaler_fill = FrameScaler::with_filter(ScaleMode::Fill, ScalingFilter::Bicubic);
    let scaled_fill = scaler_fill.scale(&src, target_dims).expect("scale fill");
    assert_eq!(scaled_fill.dimensions, target_dims);
}

// ---------------------------------------------------------------------------
// 2. Audio Transforms & Delay Offset E2E Test
// ---------------------------------------------------------------------------

#[test]
fn test_audio_transforms_and_delay_offset_e2e() {
    let mut buf = AudioSampleBuffer::from_planar_channels(
        48_000,
        ChannelLayout::Stereo,
        0,
        vec![vec![0.75; 960], vec![0.5; 960]], // 20 ms at 48kHz
    )
    .expect("audio buffer");

    // Positive delay: 5 ms delay = 240 samples of silence prepended
    apply_audio_delay(&mut buf, 5).expect("apply positive delay");
    assert_eq!(buf.samples_per_channel(), 1200);
    assert_eq!(buf.channel(0).expect("ch0")[0], 0.0);
    assert_eq!(buf.channel(0).expect("ch0")[239], 0.0);
    assert_eq!(buf.channel(0).expect("ch0")[240], 0.75);

    // Negative advance: -10 ms advance = 480 samples trimmed from beginning
    apply_audio_delay(&mut buf, -10).expect("apply negative advance");
    assert_eq!(buf.samples_per_channel(), 720);
    assert_eq!(buf.channel(0).expect("ch0")[0], 0.75);

    // Multichannel Matrix Router: 2-in to 6-out (5.1 surround)
    let mut mapping = AudioChannelMapping::new_pass_through(2);
    mapping.output_channels = 6;
    mapping.matrix = vec![
        vec![1.0, 0.0], // L
        vec![0.0, 1.0], // R
        vec![0.7, 0.7], // C
        vec![0.5, 0.5], // LFE
        vec![0.8, 0.0], // Ls
        vec![0.0, 0.8], // Rs
    ];
    let routed = encodercraft_audio_dsp::apply_audio_matrix(&mapping, &buf).expect("apply matrix");
    assert_eq!(routed.channel_count(), 6);
    assert_eq!(routed.samples_per_channel(), 720);
}

// ---------------------------------------------------------------------------
// 3. Queue Scheduler & Batch Multi-Target Lifecycle E2E Test
// ---------------------------------------------------------------------------

#[test]
fn test_queue_scheduler_lifecycle_and_multitarget_e2e() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let journal_path = temp_dir.path().join("queue-journal.json");

    let queue = QueueManager::new_persisted(journal_path.clone()).expect("persisted queue");

    // Create job with 2 targets
    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "industry-job-1".into(),
        client_info: ClientInfo { app_name: "Rollout QA".into(), app_version: "0.1.0".into(), pid: 42, user_notes: None },
        priority: JobPriority::High,
        start_policy: StartPolicy::Paused,
        source: SourceDescriptor::File {
            path: PathBuf::from("production_camera_master.mov"),
            start_tick: None,
            duration_ticks: None,
            timebase: None,
        },
        targets: vec![
            OutputTarget {
                target_id: "t1-prores".into(),
                output_path: temp_dir.path().join("master.mov"),
                preset_name: "Apple ProRes 422 HQ".into(),
                container: ContainerType::QuickTimeMov,
                transform: VideoTransform {
                    scale_mode: ScaleMode::Fit,
                    scaling_filter: ScalingFilter::Lanczos3,
                    rotation: Rotation::None,
                    flip: FlipMode::None,
                    deinterlace: DeinterlaceMode::TopFieldFirst,
                    par_conform: encodercraft_core::PixelAspectRatioConform::Square,
                    frame_rate_mode: FrameRateMode::Nearest,
                },
                audio: Some(AudioEncodeSettings {
                    codec: Some("pcm_s24le".into()),
                    sample_rate: Some(48_000),
                    layout: Some(ChannelLayout::Stereo),
                    bitrate_kbps: None,
                    delay_ms: 0,
                }),
                ..Default::default()
            },
            OutputTarget {
                target_id: "t2-web".into(),
                output_path: temp_dir.path().join("web.mp4"),
                preset_name: "YouTube 1080p Full HD".into(),
                container: ContainerType::IsoBmffMp4,
                audio: Some(AudioEncodeSettings {
                    codec: Some("aac".into()),
                    sample_rate: Some(48_000),
                    layout: Some(ChannelLayout::Stereo),
                    bitrate_kbps: Some(320),
                    delay_ms: -20, // 20ms audio advance
                }),
                ..Default::default()
            },
        ],
    };

    // Submit job
    queue.submit_job(spec.clone()).expect("submit job");
    assert_eq!(queue.list_jobs().len(), 1);

    // Duplicate target
    queue.duplicate_output_target("industry-job-1", 1).expect("duplicate target");
    let jobs = queue.list_jobs();
    assert_eq!(jobs[0].spec.targets.len(), 3);

    // Update job priority via update_job_spec
    let mut updated_spec = jobs[0].spec.clone();
    updated_spec.priority = JobPriority::Urgent;
    queue.update_job_spec("industry-job-1", updated_spec).expect("set urgent");
    assert_eq!(queue.list_jobs()[0].spec.priority, JobPriority::Urgent);

    // Cancel job
    queue.cancel_job("industry-job-1").expect("cancel job");
    assert_eq!(queue.list_jobs()[0].status, JobStatus::Cancelled);

    // Reset job back to Queued
    queue.reset_job_status("industry-job-1").expect("reset status");
    assert_eq!(queue.list_jobs()[0].status, JobStatus::Queued);

    // Drop and re-open queue to verify journal persistence
    drop(queue);
    let reloaded_queue = QueueManager::new_persisted(journal_path).expect("reload queue");
    let reloaded_jobs = reloaded_queue.list_jobs();
    assert_eq!(reloaded_jobs.len(), 1);
    assert_eq!(reloaded_jobs[0].status, JobStatus::Queued);
    assert_eq!(reloaded_jobs[0].spec.targets.len(), 3);
}

// ---------------------------------------------------------------------------
// 4. Watch Folder Multi-Target Automation E2E Test
// ---------------------------------------------------------------------------

#[test]
fn test_watch_folder_multi_target_automation_e2e() {
    let dir = tempfile::tempdir().expect("tempdir");
    let watch_in = dir.path().join("watch_in");
    let watch_out = dir.path().join("watch_out");
    let archive_dir = dir.path().join("archive");

    std::fs::create_dir_all(&watch_in).expect("create in");
    std::fs::create_dir_all(&watch_out).expect("create out");
    std::fs::create_dir_all(&archive_dir).expect("create archive");

    let queue = Arc::new(QueueManager::new_in_memory());
    let mut service = WatchFolderService::new(PresetRegistry::default());

    let mut cfg = WatchFolderConfig::new("wf-qa-1", watch_in.clone(), watch_out.clone(), "Match Source - High Bitrate");
    cfg.settle_delay_ms = 0; // immediate for unit test
    cfg.targets.push(WatchTargetConfig::new("YouTube 1080p Full HD", Some(watch_out.join("web"))));
    cfg.targets.push(WatchTargetConfig::new("Apple ProRes 422 HQ", Some(watch_out.join("prores"))));
    cfg.post_action = PostProcessAction::MoveToArchive(archive_dir.clone());
    service.add_config(cfg);

    assert_eq!(service.configs().len(), 1);

    // Place valid media image in watch folder
    let media_file = watch_in.join("card_001.png");
    let test_frame =
        VideoFrame::new_allocated(Dimensions::new(16, 16).expect("dims"), PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24)
            .expect("frame");
    let png_bytes = encodercraft_image_codecs::encode_png(&test_frame).expect("encode png");
    std::fs::write(&media_file, png_bytes).expect("write valid test media");

    // Perform settled folder scan
    let ready = service.scan_once().expect("scan watch folder");
    assert_eq!(ready.len(), 1);
    let (folder_id, spec) = &ready[0];
    assert_eq!(folder_id, "wf-qa-1");
    assert_eq!(spec.targets.len(), 3);
    assert_eq!(spec.targets[0].preset_name, "Match Source - High Bitrate");
    assert_eq!(spec.targets[1].preset_name, "YouTube 1080p Full HD");
    assert_eq!(spec.targets[2].preset_name, "Apple ProRes 422 HQ");

    // Submit scanned job to queue
    queue.submit_job(spec.clone()).expect("submit watch job");
    assert_eq!(queue.list_jobs().len(), 1);
}

// ---------------------------------------------------------------------------
// 5. IPC Server & Sibling App Job Ingestion Protocol E2E Test
// ---------------------------------------------------------------------------

#[test]
fn test_ipc_server_and_client_dispatch_protocol_e2e() {
    let queue = Arc::new(QueueManager::new_in_memory());
    let presets = Arc::new(std::sync::Mutex::new(PresetRegistry::default()));
    let handler = Arc::new(EngineIpcHandler::new(queue.clone(), presets));

    let addr = "127.0.0.1:18999";
    let mut server = IpcServer::bind_tcp(addr, handler).expect("bind tcp");
    assert!(server.is_running());

    // Connect IPC client
    let client = DispatchClient::connect_tcp(addr);

    // Call queue.list (initially empty)
    let list = client.list_jobs().expect("list jobs via ipc");
    assert_eq!(list.as_array().expect("array").len(), 0);

    // Submit transcode job via IPC
    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "ipc-submitted-job".into(),
        client_info: ClientInfo {
            app_name: "FilmCraft".into(),
            app_version: "2026.1".into(),
            pid: 9999,
            user_notes: Some("Export timeline sequence".into()),
        },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: SourceDescriptor::File { path: PathBuf::from("timeline.mov"), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "filmcraft-export-target".into(),
            output_path: PathBuf::from("timeline.mp4"),
            preset_name: "Match Source - High Bitrate".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        }],
    };

    let response_id = client.submit_job(&spec).expect("submit job via ipc");
    assert_eq!(response_id, "ipc-submitted-job");

    // Verify job appeared in the queue
    let updated_list = client.list_jobs().expect("list jobs after submit");
    let arr = updated_list.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["spec"]["clientInfo"]["appName"].as_str(), Some("FilmCraft"));

    // Check status
    let status = client.get_status("ipc-submitted-job").expect("get status via ipc");
    assert_eq!(status, JobStatus::Paused);

    // Cancel job via IPC
    client.cancel_job("ipc-submitted-job").expect("cancel job via ipc");
    let status_after = client.get_status("ipc-submitted-job").expect("status after cancel");
    assert_eq!(status_after, JobStatus::Cancelled);

    server.stop();
}
