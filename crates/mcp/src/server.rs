//! Model Context Protocol (MCP) server stdio runner powered by `craft-mcp-server`.

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_engine::QueueManager;
use encodercraft_presets::PresetRegistry;
use serde_json::Value;

use crate::roots::AutomationRoots;
use crate::tools::{call_tool_with_roots, list_tools};

/// Maximum accepted length of a single MCP JSON-RPC request line (4 MiB).
const MAX_MCP_LINE_BYTES: usize = 4 * 1024 * 1024;

struct EncoderCraftTools {
    queue_mgr: Arc<QueueManager>,
    preset_registry: Arc<Mutex<PresetRegistry>>,
    roots: Option<AutomationRoots>,
}

impl craft_mcp_server::Tools for EncoderCraftTools {
    fn instructions(&self) -> String {
        "Inspect media files, query presets, submit and manage batch transcoding jobs, and execute shared encoderCraft commands.".into()
    }

    fn list(&mut self) -> Vec<Value> {
        list_tools().as_array().cloned().unwrap_or_default()
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        match call_tool_with_roots(name, args, &self.queue_mgr, &self.preset_registry, self.roots.as_ref()) {
            Ok(result) => {
                let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                craft_mcp_server::text_result(text, false)
            }
            Err(err) => craft_mcp_server::text_result(err.to_string(), true),
        }
    }
}

/// MCP server controller backed by the shared `craft-mcp-server` protocol engine.
pub struct McpServer {
    inner: Mutex<craft_mcp_server::Server<EncoderCraftTools>>,
}

impl McpServer {
    /// Creates a new MCP server wrapping the given queue manager and preset registry.
    pub fn new(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>) -> Self {
        Self::build(queue_mgr, preset_registry, None)
    }

    /// Creates a new MCP server that enforces canonical read and write root boundaries.
    pub fn with_roots(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>, roots: AutomationRoots) -> Self {
        Self::build(queue_mgr, preset_registry, Some(roots))
    }

    fn build(queue_mgr: Arc<QueueManager>, preset_registry: Arc<Mutex<PresetRegistry>>, roots: Option<AutomationRoots>) -> Self {
        let info = craft_mcp_server::Info {
            name: "encodercraft-mcp",
            title: "encoderCraft",
            version: env!("CARGO_PKG_VERSION"),
            max_line: MAX_MCP_LINE_BYTES,
        };
        let tools = EncoderCraftTools { queue_mgr, preset_registry, roots };
        Self { inner: Mutex::new(craft_mcp_server::Server::new(info, tools)) }
    }

    /// Processes a single incoming JSON-RPC line from the client.
    /// Returns `Some(response_json)` or `None` if the message was a notification requiring no response.
    pub fn process_message(&self, raw: &str) -> Option<String> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).handle_line(raw)
    }

    /// Runs the bounded stdio loop, reading requests from `reader` and writing responses to `writer`.
    pub fn run_stdio<R: BufRead, W: Write>(&self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).serve(&mut reader, &mut writer)
    }
}
