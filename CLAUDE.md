# encoderCraft — Agent & Assistant Quick Reference

Read `AGENTS.md` before doing any work in this repository.

## Commands
- **Run GUI:** `cargo run --release -p encodercraft`
- **Run CLI:** `cargo run --release -p encodercraft-cli -- <command>`
- **Run MCP Server:** `cargo run --release -p encodercraft-cli -- mcp`
- **Quality Gates:** `cargo xtask ci` (runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask layers`, `cargo xtask wasm`)
- **Layer Validation:** `cargo xtask layers`
- **Run Unit Tests:** `cargo test --workspace`

## Key Architecture Rules
- **Never crash:** `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]` in every crate root.
- **Inherit lints:** Every crate's `Cargo.toml` must include `[lints] workspace = true`.
- **Layering:** Nothing below Layer 6 may depend on `egui`, `eframe`, `winit`, or `rfd`.
- **Clean-room:** Black-box observation of Adobe Media Encoder only. No GPL code. No proprietary Adobe assets.
- **Reuse first:** Consult `docs/artcraft-exporters-survey.md` to reuse existing codecs and muxers from `craft-libs`, `filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, `texturecraft`.
