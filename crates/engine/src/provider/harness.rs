//! Conformance test harness and mock worker for provider validation (M46.4).

use std::io::{Read, Write};

use encodercraft_core::{CURRENT_PROTOCOL_VERSION, EncoderError};

use super::protocol::{HostMessage, WorkerMessage, read_message, write_message};

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Standalone mock codec provider simulating an external video encoder executable.
pub struct MockCodecWorker {
    protocol_version: u32,
    codec_id: String,
    config_record: Vec<u8>,
    fourcc: Option<String>,
    simulate_error_on_frame: Option<usize>,
    frame_count: usize,
}

impl MockCodecWorker {
    pub fn new(codec_id: &str) -> Self {
        Self {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            codec_id: codec_id.to_string(),
            config_record: vec![0x01, 0x64, 0x00, 0x1F], // mock avcC
            fourcc: Some("avc1".to_string()),
            simulate_error_on_frame: None,
            frame_count: 0,
        }
    }

    pub fn with_unsupported_protocol(mut self, version: u32) -> Self {
        self.protocol_version = version;
        self
    }

    pub fn with_error_on_frame(mut self, frame_index: usize) -> Self {
        self.simulate_error_on_frame = Some(frame_index);
        self
    }

    /// Runs the worker loop reading from `reader` and writing to `writer`.
    pub fn run<R: Read, W: Write>(&mut self, reader: &mut R, writer: &mut W) -> Result<(), EncoderError> {
        // Read Hello
        let (msg, _): (HostMessage, _) = read_message(reader, None)?;
        match msg {
            HostMessage::Hello { protocol_version, codec_id, .. } => {
                if protocol_version != self.protocol_version {
                    let err = WorkerMessage::Error {
                        code: "PROTOCOL_MISMATCH".into(),
                        message: format!("expected protocol version {}, got {}", self.protocol_version, protocol_version),
                    };
                    write_message(writer, &err, None)?;
                    return Ok(());
                }

                if codec_id != self.codec_id {
                    let err = WorkerMessage::Error {
                        code: "CODEC_MISMATCH".into(),
                        message: format!("expected codec {}, got {}", self.codec_id, codec_id),
                    };
                    write_message(writer, &err, None)?;
                    return Ok(());
                }

                let ack = WorkerMessage::HelloAck {
                    protocol_version: self.protocol_version,
                    codec_id: self.codec_id.clone(),
                    config_record_base64: Some(hex_encode(&self.config_record)),
                    fourcc: self.fourcc.clone(),
                };
                write_message(writer, &ack, None)?;
            }
            _ => {
                let err = WorkerMessage::Error { code: "EXPECTED_HELLO".into(), message: "initial message must be Hello".into() };
                write_message(writer, &err, None)?;
                return Ok(());
            }
        }

        // Loop for frames / flush / cancel
        while let Ok((msg, payload)) = read_message(reader, None) {
            match msg {
                HostMessage::EncodeFrame { pts, dts, duration_ticks, payload_bytes } => {
                    // Read payload
                    let _ = payload;
                    let mut frame_data = vec![0u8; payload_bytes as usize];
                    reader.read_exact(&mut frame_data).map_err(|e| EncoderError::Io(format!("failed reading frame bytes: {e}")))?;

                    if let Some(target) = self.simulate_error_on_frame
                        && self.frame_count == target
                    {
                        let err = WorkerMessage::Error { code: "INTERNAL_ENCODER_FAULT".into(), message: "simulated encoder failure".into() };
                        write_message(writer, &err, None)?;
                        return Ok(());
                    }

                    self.frame_count += 1;

                    // Produce simulated encoded bitstream packet
                    let packet_data = vec![0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84];
                    let packet = WorkerMessage::Packet { pts, dts, is_keyframe: pts == 0, duration_ticks, payload_bytes: packet_data.len() as u32 };
                    write_message(writer, &packet, Some(&packet_data))?;
                }
                HostMessage::Flush => {
                    write_message(writer, &WorkerMessage::FlushAck, None)?;
                    break;
                }
                HostMessage::Cancel => {
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use encodercraft_color::PixelFormat;
    use encodercraft_core::{ProviderOperationKind, Rational64};

    use super::*;

    #[test]
    fn mock_worker_handshake_and_frame_encode_roundtrip() {
        let (mut host_out, mut worker_in) = pipe();
        let (mut worker_out, mut host_in) = pipe();

        let handle = std::thread::spawn(move || {
            let mut worker = MockCodecWorker::new("mock-h264");
            worker.run(&mut worker_in, &mut worker_out).unwrap();
        });

        // Host side: send Hello
        let hello = HostMessage::Hello {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            codec_id: "mock-h264".into(),
            operation_kind: ProviderOperationKind::VideoEncode,
            width: Some(1920),
            height: Some(1080),
            fps: Some(Rational64::new(30, 1).unwrap()),
            pixel_format: Some(PixelFormat::Yuv420p),
            bitrate_kbps: Some(5000),
            sample_rate: None,
            channels: None,
            parameters: HashMap::new(),
        };
        write_message(&mut host_out, &hello, None).unwrap();

        let (ack, _): (WorkerMessage, _) = read_message(&mut host_in, None).unwrap();
        match ack {
            WorkerMessage::HelloAck { codec_id, .. } => assert_eq!(codec_id, "mock-h264"),
            other => panic!("expected HelloAck, got {other:?}"),
        }

        // Host side: send EncodeFrame
        let frame_payload = vec![128u8; 1920 * 1080 * 3 / 2];
        let frame_msg = HostMessage::EncodeFrame { pts: 0, dts: 0, duration_ticks: 3000, payload_bytes: frame_payload.len() as u32 };
        write_message(&mut host_out, &frame_msg, Some(&frame_payload)).unwrap();

        let (packet_msg, packet_payload): (WorkerMessage, _) = read_message(&mut host_in, Some(7)).unwrap();
        match packet_msg {
            WorkerMessage::Packet { is_keyframe, pts, .. } => {
                assert!(is_keyframe);
                assert_eq!(pts, 0);
            }
            other => panic!("expected Packet, got {other:?}"),
        }
        assert_eq!(packet_payload.unwrap().len(), 7);

        // Host side: Flush
        write_message(&mut host_out, &HostMessage::Flush, None).unwrap();
        let (flush_ack, _): (WorkerMessage, _) = read_message(&mut host_in, None).unwrap();
        assert_eq!(flush_ack, WorkerMessage::FlushAck);

        handle.join().unwrap();
    }

    #[test]
    fn mock_worker_reports_unsupported_protocol_version() {
        let (mut host_out, mut worker_in) = pipe();
        let (mut worker_out, mut host_in) = pipe();

        let handle = std::thread::spawn(move || {
            let mut worker = MockCodecWorker::new("mock-h264").with_unsupported_protocol(999);
            worker.run(&mut worker_in, &mut worker_out).unwrap();
        });

        let hello = HostMessage::Hello {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            codec_id: "mock-h264".into(),
            operation_kind: ProviderOperationKind::VideoEncode,
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            bitrate_kbps: None,
            sample_rate: None,
            channels: None,
            parameters: HashMap::new(),
        };
        write_message(&mut host_out, &hello, None).unwrap();

        let (resp, _): (WorkerMessage, _) = read_message(&mut host_in, None).unwrap();
        match resp {
            WorkerMessage::Error { code, .. } => assert_eq!(code, "PROTOCOL_MISMATCH"),
            other => panic!("expected Error, got {other:?}"),
        }

        handle.join().unwrap();
    }

    #[test]
    fn mock_worker_reports_encoder_error() {
        let (mut host_out, mut worker_in) = pipe();
        let (mut worker_out, mut host_in) = pipe();

        let handle = std::thread::spawn(move || {
            let mut worker = MockCodecWorker::new("mock-h264").with_error_on_frame(0);
            worker.run(&mut worker_in, &mut worker_out).unwrap();
        });

        let hello = HostMessage::Hello {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            codec_id: "mock-h264".into(),
            operation_kind: ProviderOperationKind::VideoEncode,
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            bitrate_kbps: None,
            sample_rate: None,
            channels: None,
            parameters: HashMap::new(),
        };
        write_message(&mut host_out, &hello, None).unwrap();

        let (ack, _): (WorkerMessage, _) = read_message(&mut host_in, None).unwrap();
        assert!(matches!(ack, WorkerMessage::HelloAck { .. }));

        let frame_msg = HostMessage::EncodeFrame { pts: 0, dts: 0, duration_ticks: 3000, payload_bytes: 10 };
        write_message(&mut host_out, &frame_msg, Some(&[0u8; 10])).unwrap();

        let (resp, _): (WorkerMessage, _) = read_message(&mut host_in, None).unwrap();
        match resp {
            WorkerMessage::Error { code, .. } => assert_eq!(code, "INTERNAL_ENCODER_FAULT"),
            other => panic!("expected Error, got {other:?}"),
        }

        handle.join().unwrap();
    }

    fn pipe() -> (ChannelWriter, ChannelReader) {
        let (tx, rx) = std::sync::mpsc::channel();
        (ChannelWriter { sender: tx }, ChannelReader { receiver: rx })
    }

    struct ChannelWriter {
        sender: std::sync::mpsc::Sender<u8>,
    }

    impl Write for ChannelWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            for b in buf {
                self.sender.send(*b).map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct ChannelReader {
        receiver: std::sync::mpsc::Receiver<u8>,
    }

    impl Read for ChannelReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            let first = match self.receiver.recv() {
                Ok(b) => b,
                Err(_) => return Ok(0), // EOF
            };
            buf[0] = first;
            let mut count = 1;
            while count < buf.len() {
                match self.receiver.try_recv() {
                    Ok(b) => {
                        buf[count] = b;
                        count += 1;
                    }
                    Err(_) => break,
                }
            }
            Ok(count)
        }
    }
}
