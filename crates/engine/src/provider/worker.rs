//! Codec worker process supervisor, lifecycle management, and fault isolation (ADR 0002).

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use encodercraft_color::PixelFormat;
use encodercraft_core::{CURRENT_PROTOCOL_VERSION, EncoderError, ProviderOperationKind, Rational64};
use encodercraft_video_codecs::EncodedPacket;

use super::protocol::{HostMessage, WorkerMessage, read_message, write_message};

const MAX_STDERR_CAPTURE_BYTES: usize = 64 * 1024;

/// Supervised child process executing an out-of-process codec provider.
pub struct CodecWorkerProcess {
    provider_id: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_log: Arc<Mutex<Vec<u8>>>,
    config_record: Vec<u8>,
    fourcc: Option<[u8; 4]>,
}

impl CodecWorkerProcess {
    /// Launches the provider child process and conducts the protocol handshake.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_and_handshake(
        provider_id: &str,
        executable_path: &Path,
        operation_kind: ProviderOperationKind,
        codec_id: &str,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<Rational64>,
        pixel_format: Option<PixelFormat>,
        bitrate_kbps: Option<u32>,
        sample_rate: Option<u32>,
        channels: Option<u16>,
        parameters: HashMap<String, serde_json::Value>,
    ) -> Result<Self, EncoderError> {
        if !executable_path.is_file() {
            return Err(EncoderError::Io(format!("provider executable '{}' does not exist or is not a regular file", executable_path.display())));
        }

        let mut cmd = Command::new(executable_path);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child =
            cmd.spawn().map_err(|e| EncoderError::Io(format!("failed to spawn provider executable '{}': {e}", executable_path.display())))?;

        let stdin = child.stdin.take().ok_or_else(|| EncoderError::Io("failed to capture child stdin pipe".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| EncoderError::Io("failed to capture child stdout pipe".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| EncoderError::Io("failed to capture child stderr pipe".into()))?;

        // Background thread to continuously capture stderr into bounded buffer
        let stderr_log = Arc::new(Mutex::new(Vec::new()));
        let stderr_capture = Arc::clone(&stderr_log);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut chunk = [0u8; 1024];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut log = stderr_capture.lock().unwrap_or_else(PoisonError::into_inner);
                if log.len() + n <= MAX_STDERR_CAPTURE_BYTES {
                    log.extend_from_slice(&chunk[..n]);
                }
            }
        });

        let mut worker = Self {
            provider_id: provider_id.to_string(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_log,
            config_record: Vec::new(),
            fourcc: None,
        };

        // Handshake: send Hello, await HelloAck
        let hello = HostMessage::Hello {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            codec_id: codec_id.to_string(),
            operation_kind,
            width,
            height,
            fps,
            pixel_format,
            bitrate_kbps,
            sample_rate,
            channels,
            parameters,
        };

        write_message(&mut worker.stdin, &hello, None)?;

        let (ack, _): (WorkerMessage, _) = worker.read_worker_message(None)?;
        match ack {
            WorkerMessage::HelloAck { protocol_version, codec_id: ack_codec, config_record_base64, fourcc } => {
                if protocol_version != CURRENT_PROTOCOL_VERSION {
                    let _ = worker.terminate();
                    return Err(EncoderError::InvalidJobSpec(format!(
                        "provider worker protocol mismatch: host={CURRENT_PROTOCOL_VERSION}, worker={protocol_version}"
                    )));
                }
                if ack_codec != codec_id {
                    let _ = worker.terminate();
                    return Err(EncoderError::InvalidJobSpec(format!(
                        "provider worker acknowledged incorrect codec '{ack_codec}' (expected '{codec_id}')"
                    )));
                }
                if let Some(b64) = config_record_base64 {
                    // Simple hex/base64 decode or fallback to raw bytes
                    if let Ok(bytes) = hex_or_bytes(&b64) {
                        worker.config_record = bytes;
                    }
                }
                if let Some(fcc_str) = fourcc {
                    let bytes = fcc_str.as_bytes();
                    if bytes.len() == 4 {
                        let mut arr = [0u8; 4];
                        arr.copy_from_slice(bytes);
                        worker.fourcc = Some(arr);
                    }
                }
            }
            WorkerMessage::Error { code, message } => {
                let _ = worker.terminate();
                return Err(EncoderError::InvalidJobSpec(format!("provider handshake rejected [{code}]: {message}")));
            }
            other => {
                let _ = worker.terminate();
                return Err(EncoderError::InvalidJobSpec(format!("unexpected handshake response: {other:?}")));
            }
        }

        Ok(worker)
    }

    /// Submits a raw uncompressed frame and reads any immediately produced packets.
    pub fn encode_video_frame(&mut self, pts: i64, dts: i64, duration_ticks: u32, frame_bytes: &[u8]) -> Result<Vec<EncodedPacket>, EncoderError> {
        let msg = HostMessage::EncodeFrame { pts, dts, duration_ticks, payload_bytes: frame_bytes.len() as u32 };

        if let Err(e) = write_message(&mut self.stdin, &msg, Some(frame_bytes)) {
            return Err(self.handle_io_error(e));
        }

        // External workers may buffer frames (GOP / B-frames), so we read what's available
        // If worker returns zero packets immediately or one packet, we process it
        let mut packets = Vec::new();
        // Read next message from worker
        let (worker_msg, payload): (WorkerMessage, _) = self.read_worker_message(None)?;
        match worker_msg {
            WorkerMessage::Packet { pts, dts, is_keyframe, duration_ticks, payload_bytes } => {
                let data = self.read_exact_payload(payload_bytes, payload)?;
                packets.push(EncodedPacket { data, pts, dts, is_keyframe, duration_ticks });
            }
            WorkerMessage::Error { code, message } => {
                return Err(EncoderError::Internal(format!("provider error [{code}]: {message}")));
            }
            _ => {}
        }

        Ok(packets)
    }

    /// Flushes all pending buffered frames from the worker until `FlushAck` is received.
    pub fn flush_video(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        if let Err(e) = write_message(&mut self.stdin, &HostMessage::Flush, None) {
            return Err(self.handle_io_error(e));
        }

        let mut packets = Vec::new();
        loop {
            let (msg, payload): (WorkerMessage, _) = self.read_worker_message(None)?;
            match msg {
                WorkerMessage::Packet { pts, dts, is_keyframe, duration_ticks, payload_bytes } => {
                    let data = self.read_exact_payload(payload_bytes, payload)?;
                    packets.push(EncodedPacket { data, pts, dts, is_keyframe, duration_ticks });
                }
                WorkerMessage::FlushAck => {
                    break;
                }
                WorkerMessage::Error { code, message } => {
                    return Err(EncoderError::Internal(format!("provider flush error [{code}]: {message}")));
                }
                other => {
                    return Err(EncoderError::Internal(format!("unexpected message during flush: {other:?}")));
                }
            }
        }

        Ok(packets)
    }

    /// Read worker message with diagnostic error enrichment on crash or broken pipe.
    fn read_worker_message(&mut self, expected_payload_bytes: Option<u32>) -> Result<(WorkerMessage, Option<Vec<u8>>), EncoderError> {
        read_message(&mut self.stdout, expected_payload_bytes).map_err(|e| self.handle_io_error(e))
    }

    fn read_exact_payload(&mut self, bytes: u32, preloaded: Option<Vec<u8>>) -> Result<Vec<u8>, EncoderError> {
        if let Some(buf) = preloaded
            && buf.len() == bytes as usize
        {
            return Ok(buf);
        }
        let mut buf = vec![0u8; bytes as usize];
        self.stdout.read_exact(&mut buf).map_err(|e| self.handle_io_error(EncoderError::Io(e.to_string())))?;
        Ok(buf)
    }

    fn handle_io_error(&mut self, err: EncoderError) -> EncoderError {
        let stderr_bytes = self.stderr_log.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let stderr_str = String::from_utf8_lossy(&stderr_bytes).trim().to_string();

        let status = self.child.try_wait().ok().flatten();
        let status_desc = if let Some(s) = status { format!("exited with {s}") } else { "process unresponsive or broken pipe".to_string() };

        EncoderError::Io(format!(
            "provider worker '{}' communication error: {err} ({status_desc})\nStderr diagnostics: {}",
            self.provider_id,
            if stderr_str.is_empty() { "none" } else { &stderr_str }
        ))
    }

    /// Concludes child execution and reaps process.
    pub fn terminate(&mut self) -> Result<(), EncoderError> {
        let _ = write_message(&mut self.stdin, &HostMessage::Cancel, None);
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }

    /// Decoder configuration record (`avcC` / `hvcC` / `av1C`).
    pub fn config_record(&self) -> &[u8] {
        &self.config_record
    }

    /// FourCC sample entry.
    pub fn fourcc(&self) -> Option<[u8; 4]> {
        self.fourcc
    }
}

impl Drop for CodecWorkerProcess {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn hex_or_bytes(s: &str) -> Result<Vec<u8>, ()> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    // Simple hex decoder
    if s.len().is_multiple_of(2) && s.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut v = Vec::with_capacity(s.len() / 2);
        for i in (0..s.len()).step_by(2) {
            let byte = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ())?;
            v.push(byte);
        }
        return Ok(v);
    }
    Ok(s.as_bytes().to_vec())
}
