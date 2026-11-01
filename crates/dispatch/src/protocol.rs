//! Lightweight IPC wire protocol data structures for inter-app job dispatch.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Standard JSON-RPC request format over IPC transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl IpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self { id, method: method.into(), params }
    }
}

/// Standard JSON-RPC response format over IPC transport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success(id: u64, result: Value) -> Self {
        Self { id, result: Some(result), error: None }
    }

    pub fn error(id: u64, err: impl Into<String>) -> Self {
        Self { id, result: None, error: Some(err.into()) }
    }
}
