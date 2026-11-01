//! Unit and integration tests for encoderCraft dispatch client.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use encodercraft_core::{ClientInfo, ContainerType, CraftJobSpec, JobPriority, OutputTarget, SourceDescriptor, StartPolicy};
use encodercraft_dispatch::{DispatchClient, IpcRequest, IpcResponse};
use serde_json::json;

#[test]
fn test_ipc_protocol_serde() {
    let req = IpcRequest::new(42, "queue.submit", json!({ "jobId": "test-1" }));
    let json_str = serde_json::to_string(&req).unwrap();
    let decoded: IpcRequest = serde_json::from_str(&json_str).unwrap();
    assert_eq!(req, decoded);

    let resp = IpcResponse::success(42, json!("job-accepted"));
    let json_str = serde_json::to_string(&resp).unwrap();
    let decoded: IpcResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(resp, decoded);
}

#[test]
fn test_dispatch_client_mock_server_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();

        let req: IpcRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(req.method, "queue.submit");

        let resp = IpcResponse::success(req.id, json!("job-mock-123"));
        let mut resp_bytes = serde_json::to_string(&resp).unwrap();
        resp_bytes.push('\n');

        let mut writer = stream;
        writer.write_all(resp_bytes.as_bytes()).unwrap();
        writer.flush().unwrap();
    });

    let client = DispatchClient::connect_tcp(addr.to_string());
    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: "test-roundtrip".into(),
        client_info: ClientInfo { app_name: "MockApp".into(), app_version: "1.0".into(), pid: 1234, user_notes: None },
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

    let assigned_id = client.submit_job(&spec).unwrap();
    assert_eq!(assigned_id, "job-mock-123");

    server_handle.join().unwrap();
}
