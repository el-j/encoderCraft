//! Bounded zero-copy media packets over a local shared-memory service.

use std::fmt::Display;

use encodercraft_core::EncoderError;
use iceoryx2::node::{Node, NodeBuilder};
use iceoryx2::port::{backpressure_strategy::BackpressureStrategy, publisher::Publisher, subscriber::Subscriber};
use iceoryx2::service::{ipc, port_factory::publish_subscribe::PortFactory, service_name::ServiceName};

const HEADER_SIZE: usize = 32;
const HEADER_MAGIC: &[u8; 4] = b"ECST";
const HEADER_VERSION: u16 = 1;
const STREAM_BUFFER_DEPTH: usize = 2;

/// Maximum media payload accepted in a single shared-memory packet.
pub const MAX_SHARED_STREAM_PAYLOAD_BYTES: usize = 512 * 1024 * 1024;

type IpcService = ipc::Service;
type SharedService = PortFactory<IpcService, [u8], ()>;
type SharedPublisher = Publisher<IpcService, [u8], ()>;
type SharedSubscriber = Subscriber<IpcService, [u8], ()>;
type SharedSample = iceoryx2::sample::Sample<IpcService, [u8], ()>;

/// Media packet category carried by the shared stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SharedStreamKind {
    VideoFrame = 1,
    AudioSamples = 2,
    EndOfStream = 3,
}

impl TryFrom<u16> for SharedStreamKind {
    type Error = EncoderError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::VideoFrame),
            2 => Ok(Self::AudioSamples),
            3 => Ok(Self::EndOfStream),
            _ => Err(EncoderError::InvalidJobSpec(format!("unknown shared stream packet kind {value}"))),
        }
    }
}

/// Fixed metadata placed before each packet's shared payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedStreamHeader {
    pub kind: SharedStreamKind,
    pub pts: i64,
    pub duration: i64,
    pub payload_bytes: u64,
}

/// Producer for bounded shared-memory media packets.
pub struct SharedStreamPublisher {
    _node: Node<IpcService>,
    _service: SharedService,
    publisher: SharedPublisher,
}

impl SharedStreamPublisher {
    /// Creates or opens a local stream service. `service_name` must be a valid iceoryx2 name.
    pub fn create(service_name: &str) -> Result<Self, EncoderError> {
        let node = NodeBuilder::new().create::<IpcService>().map_err(|error| io_error("create shared stream node", error))?;
        let name = ServiceName::new(service_name).map_err(|error| EncoderError::InvalidJobSpec(format!("invalid shared stream name: {error}")))?;
        let service = stream_service(&node, &name)?;
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(MAX_SHARED_STREAM_PAYLOAD_BYTES + HEADER_SIZE)
            .max_loaned_samples(STREAM_BUFFER_DEPTH)
            .backpressure_strategy(BackpressureStrategy::RetryUntilDelivered)
            .create()
            .map_err(|error| io_error("create shared stream publisher", error))?;

        Ok(Self { _node: node, _service: service, publisher })
    }

    /// Loans shared memory to the caller, which fills it in place before publication.
    ///
    /// The callback writes directly into the shared sample; media bytes are not copied into a
    /// socket buffer or serialized `Vec`.
    pub fn publish_with(&self, header: SharedStreamHeader, fill: impl FnOnce(&mut [u8]) -> Result<(), EncoderError>) -> Result<(), EncoderError> {
        if header.payload_bytes > MAX_SHARED_STREAM_PAYLOAD_BYTES as u64 {
            return Err(EncoderError::InvalidJobSpec(format!(
                "shared stream payload ({} bytes) exceeds the {} byte limit",
                header.payload_bytes, MAX_SHARED_STREAM_PAYLOAD_BYTES
            )));
        }
        if header.kind == SharedStreamKind::EndOfStream && header.payload_bytes != 0 {
            return Err(EncoderError::InvalidJobSpec("end-of-stream packet cannot contain media bytes".into()));
        }

        let payload_len = usize::try_from(header.payload_bytes)
            .map_err(|_| EncoderError::InvalidJobSpec("shared stream payload length exceeds this platform's address space".into()))?;
        let sample_len =
            HEADER_SIZE.checked_add(payload_len).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream packet size overflow".into()))?;
        let mut sample = self.publisher.loan_slice(sample_len).map_err(|error| io_error("loan shared stream sample", error))?;
        let bytes = sample.payload_mut();
        let (header_bytes, media_bytes) = bytes.split_at_mut(HEADER_SIZE);
        encode_header(header, header_bytes)?;
        fill(media_bytes)?;
        sample.send().map_err(|error| io_error("publish shared stream sample", error))?;
        Ok(())
    }
}

/// Consumer for bounded shared-memory media packets.
pub struct SharedStreamReceiver {
    _node: Node<IpcService>,
    _service: SharedService,
    subscriber: SharedSubscriber,
}

impl SharedStreamReceiver {
    /// Opens or creates a local stream service and subscribes to its shared samples.
    pub fn open(service_name: &str) -> Result<Self, EncoderError> {
        let node = NodeBuilder::new().create::<IpcService>().map_err(|error| io_error("create shared stream node", error))?;
        let name = ServiceName::new(service_name).map_err(|error| EncoderError::InvalidJobSpec(format!("invalid shared stream name: {error}")))?;
        let service = stream_service(&node, &name)?;
        let subscriber = service.subscriber_builder().create().map_err(|error| io_error("create shared stream subscriber", error))?;

        Ok(Self { _node: node, _service: service, subscriber })
    }

    /// Receives the next packet without copying its media payload.
    pub fn receive(&self) -> Result<Option<SharedStreamPacket>, EncoderError> {
        let Some(sample) = self.subscriber.receive().map_err(|error| io_error("receive shared stream sample", error))? else {
            return Ok(None);
        };
        let bytes = sample.payload();
        let header_bytes =
            bytes.get(..HEADER_SIZE).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream packet is shorter than its header".into()))?;
        let header = decode_header(header_bytes)?;
        let payload =
            bytes.get(HEADER_SIZE..).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream packet has an invalid payload range".into()))?;
        if payload.len() as u64 != header.payload_bytes {
            return Err(EncoderError::InvalidJobSpec(format!(
                "shared stream packet declares {} bytes but contains {}",
                header.payload_bytes,
                payload.len()
            )));
        }
        if header.payload_bytes > MAX_SHARED_STREAM_PAYLOAD_BYTES as u64 {
            return Err(EncoderError::InvalidJobSpec(format!("shared stream payload exceeds the {MAX_SHARED_STREAM_PAYLOAD_BYTES} byte limit")));
        }

        Ok(Some(SharedStreamPacket { header, sample }))
    }
}

/// Received packet. Its payload remains in shared memory for this value's lifetime.
pub struct SharedStreamPacket {
    pub header: SharedStreamHeader,
    sample: SharedSample,
}

impl SharedStreamPacket {
    /// Borrows the media bytes directly from the shared-memory sample.
    pub fn payload(&self) -> Result<&[u8], EncoderError> {
        self.sample.payload().get(HEADER_SIZE..).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream packet is shorter than its header".into()))
    }
}

fn stream_service(node: &Node<IpcService>, name: &ServiceName) -> Result<SharedService, EncoderError> {
    node.service_builder(name)
        .publish_subscribe::<[u8]>()
        .enable_safe_overflow(false)
        .subscriber_max_borrowed_samples(1)
        .history_size(STREAM_BUFFER_DEPTH)
        .subscriber_max_buffer_size(STREAM_BUFFER_DEPTH)
        .open_or_create()
        .map_err(|error| io_error("open shared stream service", error))
}

fn encode_header(header: SharedStreamHeader, bytes: &mut [u8]) -> Result<(), EncoderError> {
    if bytes.len() != HEADER_SIZE {
        return Err(EncoderError::Internal("shared stream header buffer has an invalid size".into()));
    }
    bytes.get_mut(0..4).ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?.copy_from_slice(HEADER_MAGIC);
    bytes
        .get_mut(4..6)
        .ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?
        .copy_from_slice(&HEADER_VERSION.to_le_bytes());
    bytes
        .get_mut(6..8)
        .ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?
        .copy_from_slice(&(header.kind as u16).to_le_bytes());
    bytes
        .get_mut(8..16)
        .ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?
        .copy_from_slice(&header.pts.to_le_bytes());
    bytes
        .get_mut(16..24)
        .ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?
        .copy_from_slice(&header.duration.to_le_bytes());
    bytes
        .get_mut(24..32)
        .ok_or_else(|| EncoderError::Internal("shared stream header buffer is truncated".into()))?
        .copy_from_slice(&header.payload_bytes.to_le_bytes());
    Ok(())
}

fn decode_header(bytes: &[u8]) -> Result<SharedStreamHeader, EncoderError> {
    if bytes.len() != HEADER_SIZE {
        return Err(EncoderError::InvalidJobSpec("shared stream header has an invalid size".into()));
    }
    let magic = bytes.get(0..4).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header is truncated".into()))?;
    if magic != HEADER_MAGIC {
        return Err(EncoderError::InvalidJobSpec("shared stream header has an invalid signature".into()));
    }
    let version = read_u16(bytes, 4)?;
    if version != HEADER_VERSION {
        return Err(EncoderError::InvalidJobSpec(format!("unsupported shared stream protocol version {version}")));
    }

    Ok(SharedStreamHeader {
        kind: SharedStreamKind::try_from(read_u16(bytes, 6)?)?,
        pts: read_i64(bytes, 8)?,
        duration: read_i64(bytes, 16)?,
        payload_bytes: read_u64(bytes, 24)?,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, EncoderError> {
    let end = offset.checked_add(2).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header offset overflow".into()))?;
    let raw = bytes.get(offset..end).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header is truncated".into()))?;
    let value: [u8; 2] = raw.try_into().map_err(|_| EncoderError::InvalidJobSpec("shared stream header field has an invalid size".into()))?;
    Ok(u16::from_le_bytes(value))
}

fn read_i64(bytes: &[u8], offset: usize) -> Result<i64, EncoderError> {
    let end = offset.checked_add(8).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header offset overflow".into()))?;
    let raw = bytes.get(offset..end).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header is truncated".into()))?;
    let value: [u8; 8] = raw.try_into().map_err(|_| EncoderError::InvalidJobSpec("shared stream header field has an invalid size".into()))?;
    Ok(i64::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, EncoderError> {
    let end = offset.checked_add(8).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header offset overflow".into()))?;
    let raw = bytes.get(offset..end).ok_or_else(|| EncoderError::InvalidJobSpec("shared stream header is truncated".into()))?;
    let value: [u8; 8] = raw.try_into().map_err(|_| EncoderError::InvalidJobSpec("shared stream header field has an invalid size".into()))?;
    Ok(u64::from_le_bytes(value))
}

fn io_error(action: &str, error: impl Display) -> EncoderError {
    EncoderError::Io(format!("{action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn service_name() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!("encodercraft/stream-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
    }

    #[test]
    fn packet_payload_borrows_received_sample_without_copying() {
        let name = service_name();
        let publisher = SharedStreamPublisher::create(&name).unwrap();
        let receiver = SharedStreamReceiver::open(&name).unwrap();

        publisher
            .publish_with(SharedStreamHeader { kind: SharedStreamKind::VideoFrame, pts: 24, duration: 1, payload_bytes: 4 }, |payload| {
                payload.copy_from_slice(&[1, 2, 3, 4]);
                Ok(())
            })
            .unwrap();

        let packet = receiver.receive().unwrap().expect("published packet");
        assert_eq!(packet.header.kind, SharedStreamKind::VideoFrame);
        assert_eq!(packet.header.pts, 24);
        assert_eq!(packet.payload().unwrap(), &[1, 2, 3, 4]);
        assert_eq!(packet.payload().unwrap().as_ptr(), packet.sample.payload().get(HEADER_SIZE..).unwrap().as_ptr());
    }

    #[test]
    fn oversized_payload_is_rejected_before_a_shared_sample_is_loaned() {
        let publisher = SharedStreamPublisher::create(&service_name()).unwrap();
        let fill_called = Cell::new(false);
        let result = publisher.publish_with(
            SharedStreamHeader { kind: SharedStreamKind::VideoFrame, pts: 0, duration: 1, payload_bytes: MAX_SHARED_STREAM_PAYLOAD_BYTES as u64 + 1 },
            |_| {
                fill_called.set(true);
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!fill_called.get());
    }
}
