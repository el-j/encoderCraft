//! Client implementation for connecting to a local encoderCraft instance.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use encodercraft_core::{CraftJobSpec, EncoderError, JobStatus};
use serde_json::json;

use crate::protocol::{IpcRequest, IpcResponse};

/// Connection endpoint for the dispatch client.
#[derive(Debug, Clone)]
pub enum Endpoint {
    #[cfg(unix)]
    UnixSocket(PathBuf),
    Tcp(String),
}

/// Lightweight client for dispatching render jobs to encoderCraft from sibling applications.
pub struct DispatchClient {
    endpoint: Endpoint,
    req_counter: AtomicU64,
}

impl DispatchClient {
    /// Connects using the default local Unix domain socket or TCP fallback.
    pub fn connect_default() -> Self {
        #[cfg(unix)]
        {
            let sock_path = default_socket_path();
            Self { endpoint: Endpoint::UnixSocket(sock_path), req_counter: AtomicU64::new(1) }
        }
        #[cfg(not(unix))]
        {
            Self::connect_tcp("127.0.0.1:8998")
        }
    }

    /// Connects via a specific TCP host:port address.
    pub fn connect_tcp(addr: impl Into<String>) -> Self {
        Self { endpoint: Endpoint::Tcp(addr.into()), req_counter: AtomicU64::new(1) }
    }

    /// Submits a `CraftJobSpec` to encoderCraft and returns the assigned job ID.
    pub fn submit_job(&self, spec: &CraftJobSpec) -> Result<String, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let spec_value = serde_json::to_value(spec).map_err(|e| EncoderError::InvalidJobSpec(format!("failed to serialize job spec: {e}")))?;

        let request = IpcRequest::new(req_id, "queue.submit", spec_value);
        let response = self.send_request(&request)?;

        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }

        let result = response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty response from encoderCraft".into()))?;

        result
            .as_str()
            .map(ToString::to_string)
            .or_else(|| result.get("jobId").and_then(|v| v.as_str()).map(ToString::to_string))
            .ok_or_else(|| EncoderError::InvalidJobSpec("malformed job ID in response".into()))
    }

    /// Queries the status of an existing job.
    pub fn get_status(&self, job_id: &str) -> Result<JobStatus, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.status", json!({ "jobId": job_id }));
        let response = self.send_request(&request)?;

        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }

        let result = response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty status response".into()))?;

        serde_json::from_value(result).map_err(|e| EncoderError::InvalidJobSpec(format!("invalid status response: {e}")))
    }

    /// Cancels an active or queued job.
    pub fn cancel_job(&self, job_id: &str) -> Result<(), EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.cancel", json!({ "jobId": job_id }));
        let response = self.send_request(&request)?;

        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }
        Ok(())
    }

    /// Queries the full list of queued and active jobs from encoderCraft.
    pub fn list_jobs(&self) -> Result<serde_json::Value, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.list", json!({}));
        let response = self.send_request(&request)?;

        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }

        response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty list response".into()))
    }

    /// Lists the shared engine commands exposed by the running encoderCraft instance.
    pub fn list_commands(&self) -> Result<serde_json::Value, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "command.list", json!({}));
        let response = self.send_request(&request)?;
        if let Some(error) = response.error {
            return Err(EncoderError::InvalidJobSpec(error));
        }
        response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty command list response".into()))
    }

    /// Executes a shared engine command on the running encoderCraft instance.
    pub fn execute_command(&self, command_id: &str, params: serde_json::Value) -> Result<serde_json::Value, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "command.execute", json!({ "commandId": command_id, "params": params }));
        let response = self.send_request(&request)?;
        if let Some(error) = response.error {
            return Err(EncoderError::InvalidJobSpec(error));
        }
        response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty command response".into()))
    }

    /// Pings the running encoderCraft daemon.
    pub fn ping(&self) -> Result<bool, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "app.ping", json!({}));
        let response = self.send_request(&request)?;
        Ok(response.error.is_none())
    }

    /// Triggers start of queue processing on the running encoderCraft daemon.
    pub fn start_queue(&self) -> Result<(), EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.start", json!({}));
        let response = self.send_request(&request)?;
        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }
        Ok(())
    }

    /// Pauses an active job or the entire queue.
    pub fn pause_queue(&self, job_id: Option<&str>) -> Result<(), EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let params = if let Some(id) = job_id { json!({ "jobId": id }) } else { json!({}) };
        let request = IpcRequest::new(req_id, "queue.pause", params);
        let response = self.send_request(&request)?;
        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }
        Ok(())
    }

    /// Clears completed jobs from the queue on the running daemon.
    pub fn clear_finished(&self) -> Result<usize, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.clear_finished", json!({}));
        let response = self.send_request(&request)?;
        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }
        let result = response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty clear response".into()))?;
        Ok(result.get("removedCount").and_then(|v| v.as_u64()).unwrap_or(0) as usize)
    }

    /// Retrieves transcode history log from the running daemon.
    pub fn get_log(&self) -> Result<serde_json::Value, EncoderError> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let request = IpcRequest::new(req_id, "queue.log", json!({}));
        let response = self.send_request(&request)?;
        if let Some(err) = response.error {
            return Err(EncoderError::InvalidJobSpec(err));
        }
        response.result.ok_or_else(|| EncoderError::InvalidJobSpec("empty log response".into()))
    }

    fn send_request(&self, req: &IpcRequest) -> Result<IpcResponse, EncoderError> {
        let mut line = serde_json::to_string(req).map_err(|e| EncoderError::Io(format!("failed to serialize request: {e}")))?;
        line.push('\n');

        match &self.endpoint {
            #[cfg(unix)]
            Endpoint::UnixSocket(path) => {
                use std::os::unix::net::UnixStream;
                if let Ok(mut stream) = UnixStream::connect(path) {
                    stream.write_all(line.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write to socket: {e}")))?;
                    stream.flush().map_err(|e| EncoderError::Io(format!("flush failed: {e}")))?;

                    let mut reader = BufReader::new(stream);
                    let mut resp_line = String::new();
                    reader.read_line(&mut resp_line).map_err(|e| EncoderError::Io(format!("failed to read response: {e}")))?;

                    serde_json::from_str(&resp_line).map_err(|e| EncoderError::Io(format!("failed to parse response JSON: {e}")))
                } else {
                    // Fall back to loopback TCP (AME/ArtCraft cross-connection resilience)
                    let mut tcp = TcpStream::connect("127.0.0.1:8998")
                        .or_else(|_| TcpStream::connect("127.0.0.1:8999"))
                        .map_err(|e| EncoderError::Io(format!("failed to connect to encoderCraft via Unix socket {} or TCP: {e}", path.display())))?;
                    tcp.write_all(line.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write to tcp: {e}")))?;
                    tcp.flush().map_err(|e| EncoderError::Io(format!("flush failed: {e}")))?;

                    let mut reader = BufReader::new(tcp);
                    let mut resp_line = String::new();
                    reader.read_line(&mut resp_line).map_err(|e| EncoderError::Io(format!("failed to read response: {e}")))?;

                    serde_json::from_str(&resp_line).map_err(|e| EncoderError::Io(format!("failed to parse response JSON: {e}")))
                }
            }
            Endpoint::Tcp(addr) => {
                let mut stream = TcpStream::connect(addr).map_err(|e| EncoderError::Io(format!("failed to connect to {addr}: {e}")))?;
                stream.write_all(line.as_bytes()).map_err(|e| EncoderError::Io(format!("failed to write to tcp: {e}")))?;
                stream.flush().map_err(|e| EncoderError::Io(format!("flush failed: {e}")))?;

                let mut reader = BufReader::new(stream);
                let mut resp_line = String::new();
                reader.read_line(&mut resp_line).map_err(|e| EncoderError::Io(format!("failed to read response: {e}")))?;

                serde_json::from_str(&resp_line).map_err(|e| EncoderError::Io(format!("failed to parse response JSON: {e}")))
            }
        }
    }
}

/// Returns the default platform Unix socket path: `/tmp/encodercraft-<uid>.sock`.
pub fn default_socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let sock = PathBuf::from(tmp).join("encodercraft.sock");
        if sock.exists() {
            return sock;
        }
        let uid = get_uid();
        let fallback = PathBuf::from(format!("/tmp/encodercraft-{uid}.sock"));
        if fallback.exists() {
            return fallback;
        }
        PathBuf::from("/tmp/encodercraft.sock")
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(r"\\.\pipe\encodercraft")
    }
}

#[cfg(unix)]
fn get_uid() -> u32 {
    std::env::var("UID").ok().and_then(|s| s.parse().ok()).unwrap_or(1000)
}
