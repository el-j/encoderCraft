//! Central transcoding queue engine, concurrency scheduler, and command registry.
//!
//! Enforces poison-resilient locking, atomic filesystem journaling, priority scheduling,
//! and unified command dispatch across GUI, CLI, loopback IPC, and MCP.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cmd;
pub mod conversion;
pub mod ipc;
pub mod queue;
pub mod state;

pub use cmd::{CommandRegistry, CommandSpec};
pub use conversion::{ConversionDecision, LossyConversion, LossyConversionChange, LossyConversionKey, assess_lossy_conversions};
pub use ipc::EngineIpcHandler;
pub use queue::QueueManager;
pub use state::{
    AppPreferences, LogEntry, QueueJournal, QueuedJob, default_config_dir, default_preferences_path, load_journal, load_preferences,
    save_journal_atomic, save_preferences_atomic,
};
