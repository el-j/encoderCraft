//! Model Context Protocol (MCP) server for encoderCraft.
//!
//! Enables AI coding agents and autonomous pair programmers to submit transcode jobs,
//! inspect render queue status, list factory presets, and probe media metadata.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod server;
pub mod tools;

pub use server::McpServer;
pub use tools::{call_tool, list_tools};
