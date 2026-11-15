//! Out-of-process codec provider runtime, registry, and lifecycle management (ADR 0002).

pub mod adapter;
pub mod harness;
pub mod protocol;
pub mod registry;
pub mod worker;

pub use adapter::ProviderVideoEncoder;
pub use harness::MockCodecWorker;
pub use protocol::{HostMessage, WorkerMessage, read_message, write_message};
pub use registry::{CodecResolutionError, ProviderRecord, ProviderRegistry, ResolvedCodec};
pub use worker::CodecWorkerProcess;
