# encoderCraft — instructions for agents

`encoderCraft` is a clean-room, open-source, Rust-native media transcoding and batch queue application targeting **Adobe Media Encoder** parity—and architectural superiority (universal media domains, memory safety, speed, headless daemon, agent control).

Initially developed under username **`el-j`** (`github.com/el-j/encodercraft`), it is designed for seamless upstream ingestion into the **ArtCraft** organization (`github.com/storytold/encodercraft`). Siblings with the same conventions: `filmcraft` (Premiere-class), `effectcraft` (After Effects), `soundcraft` (Pro Tools), `photocraft` (Photoshop), `texturecraft` (Substance Painter), `designcraft` (InDesign), `vectorcraft` (Illustrator), `pdfcraft` (Acrobat).

---

## Start every session here
1. Read `ROADMAP.md` (current stage, headline numbers) and `docs/implementation-milestones.md` (active milestone, next task).
2. Review `docs/architecture.md` for layer boundaries and `docs/target-app-parity.md` for Adobe Media Encoder behavioral references.
3. Check `docs/artcraft-exporters-survey.md` before writing any new codec or format logic: **always reuse existing ArtCraft exporters** from `craft-libs`, `filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, or `texturecraft`!

---

## Never crash
People and automated render farms trust `encoderCraft` with multi-hour batch queues and multi-gigabyte exports; a crash or panic destroys work. **This outranks feature work**: never ship a feature through a panic path, and fix any crash before building on top of it.
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`). The one exception is an infallible literal: `#[allow(clippy::expect_used)]` + `.expect("why it cannot fail")`.
- **Errors are `Result<T, E>`** through typed errors (using `thiserror`) and `?` operator. Return clean errors; never use `unwrap_or_default()` if a default could produce a corrupted file or silent data loss.
- **Input-derived numbers are hostile:** media files from camera cards or network watch folders may be malformed or truncated. Use `.get()` instead of direct indexing, check char boundaries, use checked/saturating arithmetic, never divide by zero or cast NaN to integer, and strictly cap allocation sizes.
- **Bounded ring buffers & recursion:** media queues and frame streaming must use bounded buffers with backpressure to prevent out-of-memory panics.
- **Poison-resilient locks:** `lock().unwrap_or_else(PoisonError::into_inner)` or map to a typed error; thread joins must be cleanly handled.
- **Last-resort guard:** Escaped panics in third-party or platform code become an error (`crates/engine/src/guard.rs`), never a terminated application.
- **Prove it:** Every bug or crash fix must land with a synthetic regression test proving the fix.
- Every crate root must carry:
  ```rust
  #![deny(
      clippy::unwrap_used,
      clippy::expect_used,
      clippy::panic,
      clippy::unimplemented,
      clippy::todo,
      clippy::unreachable
  )]
  ```
  Every crate must include `[lints] workspace = true` in `Cargo.toml`. Tests may unwrap (`clippy.toml`).

---

## Non-negotiables
- **Clean-Room:** Adobe Media Encoder is observed strictly black-box (measured UI, export parameters, workflow behavior). Never disassemble, decompile, or copy Adobe binaries, icons, artwork, presets (`.epr`), or proprietary wording. Never copy GPL/AGPL/LGPL code (e.g. FFmpeg C internals, HandBrake, Avidemux).
- **Assets:** No Adobe iconography—ever. All icons are original vector art or CC0/MIT licensed, attributed in `ASSETS.md`.
- **Fonts live in `storytold/craft-fonts`:** Never commit font binary files to this repository.
- **Everything is a command:** Every user-visible action is a `CommandSpec` in `crates/engine/src/cmd/*` (`id`, `label`, params, `run`). The GUI, CLI, loopback IPC, and MCP reach all commands identically.
- **Layering:** Strictly enforced by `cargo xtask layers`. Lower layers cannot depend on higher layers. Nothing below L6 depends on egui, eframe, winit, or rfd. The transcoding engine is 100% headless.
- **Pure-Rust & Hardware Isolation:** Workspace enforces `#![forbid(unsafe_code)]`. Hardware encoders (VideoToolbox, NVENC, VAAPI) are quarantined inside `crates/platform` under ADR 0001.
- **Quality Gates:** Before every commit, verify with `cargo xtask ci` (formatting, clippy `-D warnings`, tests, layer checks, wasm builds).

---

## Running and Testing
- **Desktop GUI:** `cargo run --release -p encodercraft`
- **Headless CLI:** `cargo run --release -p encodercraft-cli -- encode input.mov -p "YouTube 4K" -o output.mp4`
- **MCP Server:** `cargo run --release -p encodercraft-cli -- mcp`
- **Background Daemon:** `cargo run --release -p encodercraft-cli -- daemon --port 8998`
- **Drive via IPC:** JSON lines on `/tmp/encodercraft-$UID.sock` or `127.0.0.1:8998`:
  `{"id":1,"method":"queue.submit","params":{"source":"/path/to/media.mov","preset":"YouTube 4K"}}`
- **Quality Check:** `cargo xtask ci`

---

## Contributor Credits
- Contributor credits for username **`el-j`** are configured in `contributors/people.toml`.
- When preparing PRs, ensure attribution aligns with ArtCraft standard credit workflows.
