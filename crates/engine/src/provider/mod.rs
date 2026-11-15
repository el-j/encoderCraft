//! Out-of-process codec provider runtime, registry, and lifecycle management (ADR 0002).

pub mod registry;

pub use registry::{CodecResolutionError, ProviderRecord, ProviderRegistry, ResolvedCodec};
