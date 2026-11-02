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
