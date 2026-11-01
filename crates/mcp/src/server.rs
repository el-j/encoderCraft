//! Model Context Protocol (MCP) server stdio runner.

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use serde_json::{Value, json};

use crate::tools::{call_tool, list_tools};

/// MCP server controller.
pub struct McpServer {
    queue_mgr: Arc<QueueManager>,
    preset_registry: Arc<Mutex<PresetRegistry>>,
}

impl McpServer {
    /// Creates a new MCP server wrapping the given queue manager and preset registry.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        Self { queue_mgr, preset_registry }
    }

    /// Processes a single incoming JSON-RPC line from the client.
    /// Returns `Some(response_json)` or `None` if the message was a notification requiring no response.
    pub fn process_message(&self, raw: &str) -> Option<String> {
        let req: Value = serde_json::from_str(raw).ok()?;
        let id = req.get("id");
        let method = req.get("method").and_then(Value::as_str)?;

        let response = match method {
            "initialize" => {
                let id = id?;
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "encodercraft-mcp",
                            "version": "0.1.0"
                        }
                    }
                })
            }
            "notifications/initialized" => {
                // Notifications do not receive a response
                return None;
            }
            "ping" => {
                let id = id?;
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                })
            }
            "tools/list" => {
                let id = id?;
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": list_tools()
                    }
                })
            }
            "tools/call" => {
                let id = id?;
                let params = req.get("params")?;
                let name = params.get("name").and_then(Value::as_str)?;
                let empty_args = json!({});
                let args = params.get("arguments").unwrap_or(&empty_args);

                match call_tool(name, args, &self.queue_mgr, &self.preset_registry) {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": text
                                    }
                                ]
                            }
                        })
                    }
                    Err(err) => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32000,
                                "message": err.to_string()
                            }
                        })
                    }
                }
            }
            _ => {
                let id = id?;
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method '{method}' not found")
                    }
                })
            }
        };

        serde_json::to_string(&response).ok()
    }

    /// Runs the stdio loop, reading requests from `reader` and writing responses to `writer`.
    pub fn run_stdio<R: BufRead, W: Write>(&self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            if let Some(resp) = self.process_message(&line) {
                writer.write_all(resp.as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
            line.clear();
        }
        Ok(())
    }
}
