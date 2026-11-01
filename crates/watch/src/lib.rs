//! Watch folder engine for automated directory monitoring and ingest.
//!
//! Features three-stage settle-delay checks, media format sniffing, preset evaluation,
//! and automated post-processing actions (archive, move to processed, delete).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod config;
pub mod service;
pub mod settle;

pub use config::{PostProcessAction, WatchFolderConfig};
pub use service::WatchFolderService;
pub use settle::probe_file_settled;
