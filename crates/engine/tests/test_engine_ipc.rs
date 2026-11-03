//! Integration tests for EngineIpcHandler.

use std::sync::{Arc, Mutex};

use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_dispatch::IpcRequest;
use encodercraft_engine::{EngineIpcHandler, QueueManager};
use encodercraft_ipc::IpcHandler;
use encodercraft_presets::PresetRegistry;
use serde_json::json;

#[test]
fn test_ipc_handler_ping_and_version() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));
    let handler = EngineIpcHandler::new(queue_mgr, preset_registry);

    let ping_req = IpcRequest::new(1, "app.ping", json!({}));
    let ping_res = handler.handle(&ping_req);
    assert_eq!(ping_res.id, 1);
    assert!(ping_res.error.is_none());
    assert_eq!(ping_res.result.unwrap().get("pong").unwrap().as_bool(), Some(true));

    let ver_req = IpcRequest::new(2, "app.version", json!({}));
    let ver_res = handler.handle(&ver_req);
    assert_eq!(ver_res.id, 2);
    assert!(ver_res.error.is_none());
    assert_eq!(ver_res.result.unwrap().get("app").unwrap().as_str(), Some("encoderCraft"));
}

#[test]
fn test_ipc_handler_queue_workflow() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_registry = Arc::new(Mutex::new(PresetRegistry::default()));
    let handler = EngineIpcHandler::new(queue_mgr, preset_registry);

    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "ipc-job-42".into(),
        client_info: ClientInfo { app_name: "FilmCraft".into(), app_version: "0.9.5".into(), pid: 1234, user_notes: Some("Direct Send".into()) },
        priority: JobPriority::High,
        start_policy: StartPolicy::Paused,
        source: SourceDescriptor::File { path: "test_source.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t1".into(),
            output_path: "test_output.mp4".into(),
            preset_name: "YouTube 4K".into(),
            container: ContainerType::IsoBmffMp4,
            ..Default::default()
        }],
    };

    // 1. Submit job
    let sub_req = IpcRequest::new(10, "queue.submit", serde_json::to_value(&spec).unwrap());
    let sub_res = handler.handle(&sub_req);
    assert_eq!(sub_res.id, 10);
    assert!(sub_res.error.is_none());
    assert_eq!(sub_res.result.unwrap().as_str(), Some("ipc-job-42"));

    // 2. Query status
    let stat_req = IpcRequest::new(11, "queue.status", json!({ "jobId": "ipc-job-42" }));
    let stat_res = handler.handle(&stat_req);
    assert_eq!(stat_res.id, 11);
    assert!(stat_res.error.is_none());

    // 3. List jobs
    let list_req = IpcRequest::new(12, "queue.list", json!({}));
    let list_res = handler.handle(&list_req);
    assert_eq!(list_res.id, 12);
    let jobs_val = list_res.result.unwrap();
    assert!(!jobs_val.as_array().unwrap().is_empty());

    // 4. Presets list
    let pre_req = IpcRequest::new(13, "presets.list", json!({}));
    let pre_res = handler.handle(&pre_req);
    assert_eq!(pre_res.id, 13);
    assert!(pre_res.result.unwrap().as_array().unwrap().len() >= 5);

    // 5. Cancel job
    let cancel_req = IpcRequest::new(14, "queue.cancel", json!({ "jobId": "ipc-job-42" }));
    let cancel_res = handler.handle(&cancel_req);
    assert_eq!(cancel_res.id, 14);
    assert!(cancel_res.error.is_none());
}

fn write_wav(path: &std::path::Path) {
    let samples: Vec<u8> = (0..1600i32).flat_map(|i| ((((i as f32) * 0.2).sin() * 6000.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36u32 + samples.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    b.extend_from_slice(&samples);
    std::fs::write(path, b).unwrap();
}

fn wav_spec(dir: &std::path::Path, id: &str) -> CraftJobSpec {
    let src = dir.join(format!("{id}.wav"));
    write_wav(&src);
    CraftJobSpec {
        version: "1.0".into(),
        job_id: id.into(),
        client_info: ClientInfo { app_name: "test".into(), app_version: "0".into(), pid: 1, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::Paused,
        source: SourceDescriptor::File { path: src, start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t".into(),
            output_path: dir.join(format!("{id}_out.wav")),
            preset_name: "WAV".into(),
            container: ContainerType::Wave,
            ..Default::default()
        }],
    }
}

fn wait_idle(handler: &EngineIpcHandler) -> Vec<serde_json::Value> {
    for _ in 0..600 {
        let res = handler.handle(&IpcRequest::new(90, "queue.list", json!({}))).result.unwrap();
        let jobs = res.as_array().unwrap().clone();
        let busy = jobs.iter().any(|j| matches!(j["status"].as_str(), Some("Queued" | "Processing")));
        let running = handler.handle(&IpcRequest::new(91, "queue.is_running", json!({}))).result.unwrap();
        if !busy && running.as_bool() != Some(true) {
            return jobs;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("queue did not drain");
}

#[test]
fn test_ipc_start_once_drains_all_jobs_and_cancel_is_honoured() {
    let dir = tempfile::tempdir().unwrap();
    let handler = EngineIpcHandler::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));

    for id in ["a", "b", "c", "d"] {
        let res = handler.handle(&IpcRequest::new(1, "queue.submit", serde_json::to_value(wav_spec(dir.path(), id)).unwrap()));
        assert!(res.error.is_none(), "{:?}", res.error);
    }
    assert!(handler.handle(&IpcRequest::new(2, "queue.cancel", json!({ "jobId": "d" }))).error.is_none());
    for id in ["a", "b", "c"] {
        handler.handle(&IpcRequest::new(3, "queue.reset", json!({ "jobId": id })));
    }

    assert!(handler.handle(&IpcRequest::new(4, "queue.start", json!({}))).error.is_none());
    let jobs = wait_idle(&handler);
    let status = |id: &str| jobs.iter().find(|j| j["spec"]["jobId"] == id).and_then(|j| j["status"].as_str().map(str::to_owned));
    for id in ["a", "b", "c"] {
        assert_eq!(status(id).as_deref(), Some("Completed"), "job {id}");
        assert!(dir.path().join(format!("{id}_out.wav")).exists());
    }
    assert_eq!(status("d").as_deref(), Some("Cancelled"));
    assert!(!dir.path().join("d_out.wav").exists());
}

#[test]
fn test_ipc_auto_start_policy_runs_without_queue_start() {
    let dir = tempfile::tempdir().unwrap();
    let handler = EngineIpcHandler::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));

    let mut spec = wav_spec(dir.path(), "auto");
    spec.start_policy = StartPolicy::AutoStart;
    let res = handler.handle(&IpcRequest::new(1, "queue.submit", serde_json::to_value(&spec).unwrap()));
    assert!(res.error.is_none(), "{:?}", res.error);

    let jobs = wait_idle(&handler);
    assert_eq!(jobs[0]["status"].as_str(), Some("Completed"));
    assert!(dir.path().join("auto_out.wav").exists());
}

#[test]
fn test_ipc_paused_policy_waits_for_an_explicit_start() {
    let dir = tempfile::tempdir().unwrap();
    let handler = EngineIpcHandler::new(Arc::new(QueueManager::new_in_memory()), Arc::new(Mutex::new(PresetRegistry::default())));

    let res = handler.handle(&IpcRequest::new(1, "queue.submit", serde_json::to_value(wav_spec(dir.path(), "held")).unwrap()));
    assert!(res.error.is_none(), "{:?}", res.error);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let jobs = handler.handle(&IpcRequest::new(2, "queue.list", json!({}))).result.unwrap();
    assert_eq!(jobs[0]["status"].as_str(), Some("Paused"));
    assert!(!dir.path().join("held_out.wav").exists());
}

#[test]
fn test_ipc_subscribe_streams_job_events_until_completion() {
    use std::io::{BufRead, BufReader, Write};

    let dir = tempfile::tempdir().unwrap();
    let queue = Arc::new(QueueManager::new_in_memory());
    let handler = Arc::new(EngineIpcHandler::new(queue, Arc::new(Mutex::new(PresetRegistry::default()))));
    let mut server = encodercraft_ipc::IpcServer::bind_tcp("127.0.0.1:18998", handler).unwrap();

    let stream = std::net::TcpStream::connect("127.0.0.1:18998").unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    writer.write_all(b"{\"id\":7,\"method\":\"queue.subscribe\",\"params\":{}}\n").unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("subscribed"), "{line}");

    let mut spec = wav_spec(dir.path(), "evt");
    spec.start_policy = StartPolicy::AutoStart;
    let submit = format!("{{\"id\":8,\"method\":\"queue.submit\",\"params\":{}}}\n", serde_json::to_string(&spec).unwrap());
    let mut other = std::net::TcpStream::connect("127.0.0.1:18998").unwrap();
    other.write_all(submit.as_bytes()).unwrap();

    let mut saw_completed = false;
    for _ in 0..50 {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            break;
        }
        if line.contains("\"jobId\":\"evt\"") && line.contains("Completed") {
            saw_completed = true;
            break;
        }
    }
    assert!(saw_completed, "no completion event received");
    server.stop();
}
