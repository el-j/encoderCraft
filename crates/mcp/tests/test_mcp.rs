//! Integration tests verifying MCP server initialization, tool discovery, and tool execution.

use std::sync::{Arc, Mutex};

use encodercraft_engine::QueueManager;
use encodercraft_mcp::McpServer;
use encodercraft_presets::PresetRegistry;
use serde_json::{Value, json};

#[test]
fn test_mcp_handshake_and_tools() {
    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let preset_reg = Arc::new(Mutex::new(PresetRegistry::default()));
    let server = McpServer::new(queue_mgr, preset_reg);

    // 1. Initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "TestClient", "version": "1.0" }
        }
    });
    let init_resp: Value = serde_json::from_str(&server.process_message(&init_req.to_string()).unwrap()).unwrap();
    assert_eq!(init_resp.get("id").and_then(Value::as_u64), Some(1));
    assert!(init_resp.get("result").unwrap().get("serverInfo").is_some());

    // 2. Tools List
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let list_resp: Value = serde_json::from_str(&server.process_message(&list_req.to_string()).unwrap()).unwrap();
    let tools = list_resp.get("result").unwrap().get("tools").and_then(Value::as_array).unwrap();
    assert!(tools.len() >= 4);
    assert!(tools.iter().any(|tool| tool.get("name").and_then(Value::as_str) == Some("encodercraft_execute_command")));

    // 3. Call encodercraft_list_presets
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_list_presets",
            "arguments": {}
        }
    });
    let call_resp: Value = serde_json::from_str(&server.process_message(&call_req.to_string()).unwrap()).unwrap();
    assert!(call_resp.get("result").unwrap().get("content").is_some());

    let command_list = json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_list_commands",
            "arguments": {}
        }
    });
    let command_list_resp: Value = serde_json::from_str(&server.process_message(&command_list.to_string()).unwrap()).unwrap();
    let command_list_text = command_list_resp["result"]["content"][0]["text"].as_str().unwrap();
    let command_list_value: Value = serde_json::from_str(command_list_text).unwrap();
    assert!(command_list_value["commands"].as_array().unwrap().iter().any(|item| item["id"] == "queue.select_all"));

    let command_execute = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_execute_command",
            "arguments": {
                "command_id": "queue.select_all",
                "params": {}
            }
        }
    });
    let command_execute_resp: Value = serde_json::from_str(&server.process_message(&command_execute.to_string()).unwrap()).unwrap();
    let command_execute_text = command_execute_resp["result"]["content"][0]["text"].as_str().unwrap();
    let command_execute_value: Value = serde_json::from_str(command_execute_text).unwrap();
    assert_eq!(command_execute_value["jobIds"].as_array().map(Vec::len), Some(0));

    // 4. Call encodercraft_submit_transcode
    let transcode_req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_submit_transcode",
            "arguments": {
                "source_path": "test_input.mov",
                "preset_name": "YouTube 1080p Full HD",
                "output_path": "test_out.mp4"
            }
        }
    });
    let transcode_resp: Value = serde_json::from_str(&server.process_message(&transcode_req.to_string()).unwrap()).unwrap();
    assert!(transcode_resp.get("result").unwrap().get("content").is_some());

    // 5. Call encodercraft_get_queue_status
    let status_req = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_get_queue_status",
            "arguments": {}
        }
    });
    let status_resp: Value = serde_json::from_str(&server.process_message(&status_req.to_string()).unwrap()).unwrap();
    assert!(status_resp.get("result").unwrap().get("content").is_some());

    // 6. Call encodercraft_batch_transcode
    let batch_req = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_batch_transcode",
            "arguments": {
                "source_paths": ["clip_a.mov", "clip_b.mov"],
                "preset_name": "YouTube 1080p Full HD"
            }
        }
    });
    let batch_resp: Value = serde_json::from_str(&server.process_message(&batch_req.to_string()).unwrap()).unwrap();
    assert!(batch_resp.get("result").unwrap().get("content").is_some());

    // 7. Call encodercraft_stitch_clips
    let stitch_req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_stitch_clips",
            "arguments": {
                "clip_paths": ["clip_1.mov", "clip_2.mov"],
                "output_path": "stitched_mcp.mp4"
            }
        }
    });
    let stitch_resp: Value = serde_json::from_str(&server.process_message(&stitch_req.to_string()).unwrap()).unwrap();
    assert!(stitch_resp.get("result").unwrap().get("content").is_some());

    // 8. Call encodercraft_manage_queue
    let manage_req = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {
            "name": "encodercraft_manage_queue",
            "arguments": {
                "action": "clear_finished"
            }
        }
    });
    let manage_resp: Value = serde_json::from_str(&server.process_message(&manage_req.to_string()).unwrap()).unwrap();
    assert!(manage_resp.get("result").unwrap().get("content").is_some());
}

/// One second of 48 kHz mono 16-bit silence-free tone as a canonical WAV file.
fn write_test_wav(path: &std::path::Path) {
    let samples: Vec<i16> = (0..48_000_i32).map(|n| ((n % 100) as i16 - 50) * 200).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&96_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn call_tool(server: &McpServer, id: u64, name: &str, arguments: Value) -> Value {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
    let resp: Value = serde_json::from_str(&server.process_message(&req.to_string()).unwrap()).unwrap();
    serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// A headless MCP client must be able to submit a job and have it run without any GUI starting the queue.
#[test]
fn submitted_job_runs_to_completion_without_a_gui() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("tone.wav");
    let output = dir.path().join("tone.flac");
    write_test_wav(&source);

    let queue_mgr = Arc::new(QueueManager::new_in_memory());
    let server = McpServer::new(queue_mgr, Arc::new(Mutex::new(PresetRegistry::default())));

    let submitted = call_tool(
        &server,
        1,
        "encodercraft_submit_transcode",
        json!({
            "source_path": source.to_string_lossy(),
            "preset_name": "Lossless FLAC Master (24-bit 48kHz)",
            "output_path": output.to_string_lossy(),
        }),
    );
    let job_id = submitted["jobId"].as_str().unwrap().to_string();

    let mut status = String::new();
    for _ in 0..240 {
        let job = call_tool(&server, 2, "encodercraft_get_queue_status", json!({ "job_id": job_id }));
        status = job["status"].as_str().unwrap_or_default().to_string();
        if matches!(status.as_str(), "Completed" | "Failed" | "Cancelled") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert_eq!(status, "Completed");
    assert!(std::fs::metadata(&output).map(|m| m.len() > 0).unwrap_or(false));
}
