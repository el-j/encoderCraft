//! Inter-process communication server for encoderCraft.
//!
//! Provides loopback Unix domain socket and TCP JSON-RPC service.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod server;

pub use server::{IpcHandler, IpcServer};
