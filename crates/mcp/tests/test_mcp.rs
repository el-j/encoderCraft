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
}
