//! Integration tests verifying IPC server and dispatch client communication.

use std::sync::Arc;

use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, JobStatus, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_dispatch::{DispatchClient, IpcRequest, IpcResponse};
use encodercraft_ipc::{IpcHandler, IpcServer};
use serde_json::json;

struct EchoHandler;

impl IpcHandler for EchoHandler {
    fn handle(&self, req: &IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "queue.submit" => IpcResponse::success(req.id, json!("ipc-job-1")),
            "queue.status" => IpcResponse::success(req.id, json!(JobStatus::Queued)),
            _ => IpcResponse::error(req.id, "unsupported method"),
        }
    }
}

#[test]
fn test_ipc_server_tcp_roundtrip() {
    let mut server = IpcServer::bind_tcp("127.0.0.1:8999", Arc::new(EchoHandler)).unwrap();

    let client = DispatchClient::connect_tcp("127.0.0.1:8999");
    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "test-ipc".into(),
        client_info: ClientInfo { app_name: "TestClient".into(), app_version: "1.0".into(), pid: 100, user_notes: None },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::File { path: "in.mov".into(), start_tick: None, duration_ticks: None, timebase: None },
        targets: vec![OutputTarget {
            target_id: "t1".into(),
            output_path: "out.mp4".into(),
            preset_name: "Web".into(),
            container: ContainerType::IsoBmffMp4,
            custom_width: None,
            custom_height: None,
            custom_frame_rate: None,
            target_bitrate_kbps: None,
            target_lufs: None,
        }],
    };

    let id = client.submit_job(&spec).unwrap();
    assert_eq!(id, "ipc-job-1");

    let status = client.get_status(&id).unwrap();
    assert_eq!(status, JobStatus::Queued);

    server.stop();
}
