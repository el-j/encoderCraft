//! Central transcoding queue engine, concurrency scheduler, and command registry.
//!
//! Enforces poison-resilient locking, atomic filesystem journaling, priority scheduling,
//! and unified command dispatch across GUI, CLI, loopback IPC, and MCP.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod automation;
pub mod cmd;
pub mod conversion;
pub mod ipc;
pub mod provider;
pub mod queue;
pub mod state;
pub mod system;

pub use automation::{play_completion_chime, resolve_unique_output_path, should_idle_auto_start};
pub use cmd::{CommandRegistry, CommandSpec};
pub use conversion::{ConversionDecision, LossyConversion, LossyConversionChange, LossyConversionKey, assess_lossy_conversions};
pub use ipc::EngineIpcHandler;
pub use provider::{CodecResolutionError, ProviderRecord, ProviderRegistry, ResolvedCodec};
pub use queue::{QueueManager, increment_id, increment_path_stem};
pub use state::{
    AppPreferences, LogEntry, QueueJournal, QueuedJob, default_config_dir, default_preferences_path, load_journal, load_preferences,
    save_journal_atomic, save_preferences_atomic,
};
