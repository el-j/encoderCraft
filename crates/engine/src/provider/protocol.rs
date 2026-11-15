//! Bounded wire protocol definitions and binary framing for out-of-process codec providers (ADR 0002).

use std::collections::HashMap;
use std::io::{Read, Write};

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, ProviderOperationKind, Rational64};
use serde::{Deserialize, Serialize};

/// Maximum allowable bytes for a JSON control message envelope (64 KB).
pub const MAX_CONTROL_BYTES: u32 = 64 * 1024;

/// Maximum allowable bytes for a binary frame or packet payload (64 MB).
pub const MAX_PAYLOAD_BYTES: u32 = 64 * 1024 * 1024;

/// Message sent from the encoderCraft host to the provider worker process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostMessage {
    /// Initial negotiation handshake.
    Hello {
        protocol_version: u32,
        codec_id: String,
        operation_kind: ProviderOperationKind,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        fps: Option<Rational64>,
        #[serde(default)]
        pixel_format: Option<PixelFormat>,
        #[serde(default)]
        bitrate_kbps: Option<u32>,
        #[serde(default)]
        sample_rate: Option<u32>,
        #[serde(default)]
        channels: Option<u16>,
        #[serde(default)]
        parameters: HashMap<String, serde_json::Value>,
    },
    /// Video frame metadata header; raw frame bytes follow immediately.
    EncodeFrame { pts: i64, dts: i64, duration_ticks: u32, payload_bytes: u32 },
    /// Audio sample buffer header; raw interleaved audio samples follow immediately.
    EncodeAudio { pts: i64, frames: u32, payload_bytes: u32 },
    /// Video or audio packet header to decode; raw bitstream packet follows immediately.
    DecodePacket { pts: i64, dts: i64, payload_bytes: u32 },
    /// Flushes any pending buffered frames and concludes output.
    Flush,
    /// Aborts current processing and cleans up resources immediately.
    Cancel,
}

/// Message sent from the provider worker process back to the encoderCraft host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerMessage {
    /// Acknowledgement of the initial negotiation handshake.
    HelloAck {
        protocol_version: u32,
        codec_id: String,
        #[serde(default)]
        config_record_base64: Option<String>,
        #[serde(default)]
        fourcc: Option<String>,
    },
    /// Encoded compressed video packet; raw packet bitstream follows immediately.
    Packet { pts: i64, dts: i64, is_keyframe: bool, duration_ticks: u32, payload_bytes: u32 },
    /// Encoded compressed audio packet; raw audio bitstream follows immediately.
    AudioChunk { pts: i64, payload_bytes: u32 },
    /// Decoded video frame metadata; uncompressed plane bytes follow immediately.
    DecodedFrame { pts: i64, width: u32, height: u32, pixel_format: PixelFormat, payload_bytes: u32 },
    /// Worker has flushed all buffered data.
    FlushAck,
    /// Worker encountered an unrecoverable error.
    Error { code: String, message: String },
}

/// Writes a framed message and optional raw payload to a writer.
/// Wire format:
/// `[4 bytes control JSON len (BE)][control JSON bytes][payload bytes]`
pub fn write_message<M: Serialize, W: Write>(writer: &mut W, message: &M, payload: Option<&[u8]>) -> Result<(), EncoderError> {
    let json = serde_json::to_vec(message).map_err(|e| EncoderError::Io(format!("failed to serialize control message: {e}")))?;
    let control_len = json.len() as u32;
    if control_len > MAX_CONTROL_BYTES {
        return Err(EncoderError::InvalidJobSpec(format!(
            "control message exceeds maximum allowed envelope size ({control_len} > {MAX_CONTROL_BYTES})"
        )));
    }

    let payload_bytes = payload.map(|p| p.len() as u32).unwrap_or(0);
    if payload_bytes > MAX_PAYLOAD_BYTES {
        return Err(EncoderError::InvalidJobSpec(format!(
            "payload exceeds maximum allowed binary frame size ({payload_bytes} > {MAX_PAYLOAD_BYTES})"
        )));
    }

    writer.write_all(&control_len.to_be_bytes()).map_err(|e| EncoderError::Io(e.to_string()))?;
    writer.write_all(&json).map_err(|e| EncoderError::Io(e.to_string()))?;
    if let Some(data) = payload {
        writer.write_all(data).map_err(|e| EncoderError::Io(e.to_string()))?;
    }
    writer.flush().map_err(|e| EncoderError::Io(e.to_string()))?;

    Ok(())
}

/// Reads a framed message and optional raw payload from a reader.
pub fn read_message<M: for<'de> Deserialize<'de>, R: Read>(
    reader: &mut R,
    expected_payload_bytes: Option<u32>,
) -> Result<(M, Option<Vec<u8>>), EncoderError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).map_err(|e| EncoderError::Io(format!("failed reading control message length: {e}")))?;
    let control_len = u32::from_be_bytes(len_buf);
    if control_len > MAX_CONTROL_BYTES {
        return Err(EncoderError::InvalidJobSpec(format!(
            "incoming control message exceeds maximum allowed size ({control_len} > {MAX_CONTROL_BYTES})"
        )));
    }

    let mut json_buf = vec![0u8; control_len as usize];
    reader.read_exact(&mut json_buf).map_err(|e| EncoderError::Io(format!("failed reading control message body: {e}")))?;

    let message: M = serde_json::from_slice(&json_buf).map_err(|e| EncoderError::Io(format!("failed to parse control JSON: {e}")))?;

    let payload = if let Some(bytes) = expected_payload_bytes {
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(EncoderError::InvalidJobSpec(format!("requested payload exceeds maximum allowed size ({bytes} > {MAX_PAYLOAD_BYTES})")));
        }
        let mut data = vec![0u8; bytes as usize];
        reader.read_exact(&mut data).map_err(|e| EncoderError::Io(format!("failed reading binary payload: {e}")))?;
        Some(data)
    } else {
        None
    };

    Ok((message, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_host_message_with_payload() {
        let msg = HostMessage::EncodeFrame { pts: 100, dts: 100, duration_ticks: 3000, payload_bytes: 4 };
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];

        let mut buf = Vec::new();
        write_message(&mut buf, &msg, Some(&payload)).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let (decoded_msg, decoded_payload) = read_message::<HostMessage, _>(&mut cursor, Some(4)).unwrap();

        assert_eq!(decoded_msg, msg);
        assert_eq!(decoded_payload, Some(payload));
    }

    #[test]
    fn rejects_oversized_control_envelope() {
        let mut buf = Vec::new();
        let oversized_len = MAX_CONTROL_BYTES + 1;
        buf.extend_from_slice(&oversized_len.to_be_bytes());
        buf.resize(oversized_len as usize + 4, 0);

        let mut cursor = std::io::Cursor::new(buf);
        let err = read_message::<HostMessage, _>(&mut cursor, None).unwrap_err();
        assert!(matches!(err, EncoderError::InvalidJobSpec(_)));
    }

    #[test]
    fn rejects_oversized_payload() {
        let msg = HostMessage::Flush;
        let mut buf = Vec::new();
        let oversized_payload = MAX_PAYLOAD_BYTES + 1;
        let json = serde_json::to_vec(&msg).unwrap();
        buf.extend_from_slice(&(json.len() as u32).to_be_bytes());
        buf.extend_from_slice(&json);

        let mut cursor = std::io::Cursor::new(buf);
        let err = read_message::<HostMessage, _>(&mut cursor, Some(oversized_payload)).unwrap_err();
        assert!(matches!(err, EncoderError::InvalidJobSpec(_)));
    }
}
