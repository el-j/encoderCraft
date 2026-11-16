//! Client dispatch library for submitting jobs to encoderCraft from sibling ArtCraft applications.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod client;
pub mod protocol;
pub mod shared_stream;

#[cfg(unix)]
pub use client::socket_path_for_uid;
pub use client::{DispatchClient, Endpoint, default_socket_path};
pub use protocol::{IpcRequest, IpcResponse};
pub use shared_stream::{
    MAX_SHARED_STREAM_PAYLOAD_BYTES, SharedStreamHeader, SharedStreamKind, SharedStreamPacket, SharedStreamPublisher, SharedStreamReceiver,
};
