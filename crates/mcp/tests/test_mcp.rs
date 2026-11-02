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
