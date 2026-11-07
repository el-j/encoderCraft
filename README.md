# encoderCraft

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org)
[![Parity Target](https://img.shields.io/badge/parity%20target-Adobe%20Media%20Encoder%202026-purple.svg)](docs/target-app-parity.md)
[![Tests](https://img.shields.io/badge/tests->150%20passed-brightgreen.svg)](docs/implementation-milestones.md)
[![Safety](https://img.shields.io/badge/unsafe_code-forbid-brightgreen.svg)](docs/architecture.md)
[![ArtCraft Ecosystem](https://img.shields.io/badge/artcraft-storytold-red.svg)](https://github.com/storytold)

**encoderCraft** is an open-source, clean-room, pure-Rust universal media transcoding, batch queue, and watch folder automation system. Designed for full parity with—and architectural superiority over—**Adobe Media Encoder**, `encoderCraft` serves as the universal export and transformation powerhouse for the entire **ArtCraft** creative ecosystem.

Originally developed under username **`el-j`** (`github.com/el-j/encoderCraft`), `encoderCraft` is built according to ArtCraft's exacting standards to prepare for official digestion into the **`storytold`** organization (`github.com/storytold/encodercraft`).

---

## 🌟 Why encoderCraft?

Adobe Media Encoder has long been the default render tool for video editors. However, modern creative studios operate across multiple media domains that legacy encoders ignore. `encoderCraft` breaks open the silo to deliver universal multi-media transcoding:

| Media Domain | Legacy Encoders (Adobe Media Encoder) | encoderCraft Universal Powerhouse |
|---|---|---|
| **Video & Broadcast** | MP4, MOV, MXF, AVI. | **Full Parity + Broadcast MXF:** H.264, Apple ProRes (Proxy, LT, Standard, HQ, 4444), Avid DNxHD/HR (HQ, HQX 10-bit), HEVC (H.265), AV1, VP9, VP8, MJPEG, animated GIF. Full **SMPTE ST 377-1 MXF OP1a & OP-Atom** muxing/demuxing, simultaneous Audio-Video multiplexing, and frame-accurate In/Out point trimming. |
| **Audio & Music** | AAC, MP3, WAV. | **Full Parity + Broadcast DSP:** Lossless FLAC (24-bit 48kHz), Apple Lossless AIFF, Broadcast Wave BWF (-23 LUFS), MP3, Ogg Opus/Vorbis, AAC-LC, EBU R128 loudness normalization (-23/-14 LUFS), true-peak limiting, multichannel stem splitting, channel remixing (mono/stereo/5.1/7.1), polyphase sinc resampling. |
| **3D & Spatial** | ❌ None. | **Universal 3D Pipelines:** Khronos glTF 2.0 / binary GLB, Wavefront OBJ (.obj + .mtl), 3D Print binary STL, Stanford PLY, Apple AR Quick Look USDZ (normative ZIP packaging + USDA schema). |
| **Raster Images** | Basic JPEG/PNG. | **High Dynamic Range & Studio Still Images:** Lossless PNG (RGBA 8-bit), Broadcast TIFF (16-bit RGB), VFX OpenEXR (half-float 16-bit), Windows Bitmap BMP (uncompressed 24/32-bit), WebP, JPEG with high-fidelity `FrameScaler` resampling (Fit, Fill, Stretch). |
| **Vector & Print** | ❌ None. | **Prepress & Vector Graphics:** Clean-room SVG optimization, Print-Ready PDF/X-4 (ISO 15930) with XMP metadata, sub-millisecond container sniffing for INDD, IDML, DOCX, PPTX, XLSX, EPUB, AI, EPS, PSD. |
| **Format Guardrails** | ⚠️ Allows invalid operations that fail midway. | **Smart Stream Classification:** Elementary stream classification (`AudioOnly`, `VideoWithAudio`, `VideoOnly`, `StillImage`, `Spatial3D`, `Vector2D`, `Document`). Automatic output targeting and preset validation preventing incompatible conversions (e.g. audio will not offer video targets). |
| **Crash Resilience** | ⚠️ Frequent crashes on malformed or corrupt files. | **Never Crash Guarantee:** Pure Rust, `#![deny(clippy::unwrap_used)]`, all input treated as hostile with bounded allocations and panic containment. |
| **Automation & AI** | Limited proprietary ExtendScript. | **Autonomous Agent Control:** Headless CLI, Unix domain sockets / Named Pipes IPC, and native **Model Context Protocol (MCP)** server for autonomous AI coding agents. |
| **Footprint & Licensing** | >3 GB bloat, telemetry, subscription DRM. | **Zero Bloat:** Standalone binary, sub-second startup, zero telemetry, dual-licensed Apache-2.0 / MIT. |

Before a video job reduces bit depth, tone-maps HDR to SDR, discards alpha, or reduces gamut, the desktop app and interactive CLI request approval. The choice can be remembered per source-to-target format pair; unattended CLI, watch-folder, IPC, and MCP jobs stop with a clear error unless an allow decision is already saved.

---

## 🏗️ Architecture & Sibling Ecosystem

`encoderCraft` is strictly layered from L0 to L7 according to ArtCraft architectural specifications, validated on every build via `cargo xtask layers`:

```
┌────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                 ArtCraft Creative Suite (16 Sibling Apps)                              │
│   FilmCraft · EffectCraft · SoundCraft · PhotoCraft · VectorCraft · DesignCraft · PdfCraft · FigCraft  │
│  CadCraft · DeckCraft · SolidCraft · WordCraft · TextureCraft · MaterialCraft · AniCraft · GridCraft   │
│                                      LightCraft · Craft Launcher                                       │
└───────────────────────────────────────────────────┬────────────────────────────────────────────────────┘
                                                    │ "Queue in encoderCraft"
                                                    │ (craft-dispatch IPC & Shared Memory)
                                                    ▼
┌────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                           encoderCraft                                                 │
│                                                                                                        │
│  ┌───────────────────────────┐  ┌─────────────────────────────┐  ┌──────────────────────────────────┐  │
│  │   Desktop UI (egui)       │  │   Headless CLI & Daemon     │  │   Model Context Protocol (MCP)   │  │
│  │   • Multi-target queue    │  │   • Batch encoding scripts  │  │   • Autonomous agent control     │  │
│  │   • Preset browser tree   │  │   • Background watch folder │  │   • Job status & inspection      │  │
│  │   • Viewport-clamped menu │  │   • Settle-delay daemon     │  │   • Automated batch conversion   │  │
│  │   • Live preview monitor  │  │   • Headless CI/CD transcode│  │   • Preset discovery & apply     │  │
│  └─────────────┬─────────────┘  └──────────────┬──────────────┘  └────────────────┬─────────────────┘  │
│                │                               │                                  │                    │
│                └───────────────────────────────┼──────────────────────────────────┘                    │
│                                                ▼                                                       │
│                           ┌─────────────────────────────────────────┐                                  │
│                           │    encodercraft-engine (L5 Session)     │                                  │
│                           │    • Priority Queue Scheduler           │                                  │
│                           │    • Atomic State Journaling            │                                  │
│                           │    • Lockless Progress Atomics          │                                  │
│                           └────────────────────┬────────────────────┘                                  │
│                                                │                                                       │
│         ┌──────────────────────────────────────┼──────────────────────────────────────┐                 │
│         ▼                                      ▼                                      ▼                 │
│  ┌──────────────┐                       ┌──────────────┐                       ┌──────────────┐         │
│  │ Video Codecs │                       │  Audio DSP   │                       │ Spatial & 2D │         │
│  │ • H.264/HEVC │                       │ • EBU R128   │                       │ • glTF / GLB │         │
│  │ • ProRes     │                       │ • Resampler  │                       │ • OBJ / STL  │         │
│  │ • DNxHD/HR   │                       │ • FLAC / WAV │                       │ • PLY / USDZ │         │
│  │ • AV1 / VP9  │                       │ • MP3 / OGG  │                       │ • PNG / TIFF │         │
│  │ • VP8 / MJPEG│                       │ • AAC-LC     │                       │ • EXR / BMP  │         │
│  └──────────────┘                       └──────────────┘                       └──────────────┘         │
│                                                │                                                       │
│                                                ▼                                                       │
│                               ┌─────────────────────────────────┐                                      │
│                               │ Upstream craft-libs Shared Core │                                      │
│                               │ • craft-media (MP4/MOV/MKV/OGG) │                                      │
│                               │ • craft-codecs (H.264, ProRes,  │                                      │
│                               │   DNx, HEVC, AV1, VP9, VP8)     │                                      │
│                               │ • craft-pdf (ISO 15930 PDF/X-4) │                                      │
│                               │ • craft-jpeg / craft-crw        │                                      │
│                               └─────────────────────────────────┘                                      │
└────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Layer Map (L0–L7)
- **L0 Core:** `encodercraft-core`, `encodercraft-geom`, `encodercraft-color`
- **L1 Demux & Classification:** `encodercraft-demux`
- **L2 Platform Isolation:** `encodercraft-platform` (quarantined hardware encoders)
- **L3 Transformation Engines:** `encodercraft-video-codecs`, `encodercraft-audio-dsp`, `encodercraft-image-codecs`, `encodercraft-spatial-io`
- **L4 Muxers & Presets:** `encodercraft-muxers`, `encodercraft-presets`, `encodercraft-pipeline`
- **L5 Queue & Dispatch:** `encodercraft-engine`, `encodercraft-watch`, `encodercraft-ipc`, `encodercraft-dispatch`
- **L6 Application Frontends:** `encodercraft-cli`, `encodercraft-mcp`, `encodercraft-ui-egui`
- **L7 Desktop Application:** `encodercraft`

---

## ⚡ Quickstart

### 1. Launching the Desktop Application
```bash
cargo run --release -p encodercraft
```
- **Universal Drag & Drop:** Drag any video, audio, image, 3D model, vector, or document into the queue.
- **Smart Stream Guardrails:** Audio inputs automatically route to audio formats; 3D models route to 3D targets.
- **Viewport-Clamped Menus:** Action popups automatically clamp within screen bounds and flip vertically near display edges.
- **High-DPI SVG Iconography:** Crisp vector iconography rendered from ArtCraft design tokens.

### 2. Using the Headless CLI
```bash
# Video: Transcode to Apple ProRes 422 HQ
cargo run --release -p encodercraft-cli -- encode input.mov -p "Apple ProRes 422 HQ" -o output.mov

# Video: Transcode to Broadcast MXF OP1a (SMPTE ST 377-1) with Avid DNxHR HQ
cargo run --release -p encodercraft-cli -- encode input.mov -p "Broadcast MXF OP1a DNxHR HQ" -o output.mxf

# Video: Transcode to Avid Media Composer MXF OP-Atom
cargo run --release -p encodercraft-cli -- encode input.mov -p "Avid Media Composer MXF OP-Atom" -o output_atom.mxf

# Image sequence in: numbered files (%04d) become a video; --fps sets the rate (default 24)
cargo run --release -p encodercraft-cli -- encode "shot_%04d.png" -p "YouTube 1080p" --fps 25 -o shot.mp4

# Project, document and CAD sources: list what can be selected, then export just that part
cargo run --release -p encodercraft-cli -- probe show.prproj
cargo run --release -p encodercraft-cli -- encode show.prproj --sequence Social -o social.prproj
cargo run --release -p encodercraft-cli -- convert report.designcraft --to pdf --pages 2-4 -o excerpt.pdf
cargo run --release -p encodercraft-cli -- encode plan.dxf --layers WALLS,DOORS -o walls.dxf

# Stills out: a %04d pattern in the output name exports one image per frame
# (needs an image preset whose container is PNG/TIFF/etc.)

# Audio: Transcode to 24-bit 48kHz Lossless FLAC Master
cargo run --release -p encodercraft-cli -- encode mix.wav -p "Lossless FLAC Master (24-bit 48kHz)" -o master.flac

# Audio: Transcode to Broadcast Wave with EBU R128 (-23 LUFS) Normalization
cargo run --release -p encodercraft-cli -- encode podcast.mp3 -p "Broadcast Wave BWF (-23 LUFS)" -o podcast.wav

# 3D: Convert Wavefront OBJ to Apple AR Quick Look USDZ
cargo run --release -p encodercraft-cli -- encode model.obj -p "Apple AR Quick Look USDZ (.usdz)" -o model.usdz

# 3D: Convert glTF Binary (GLB) to 3D Print STL
cargo run --release -p encodercraft-cli -- encode asset.glb -p "3D Print Binary STL (.stl)" -o asset.stl

# Image: Resample BMP to Lossless PNG
cargo run --release -p encodercraft-cli -- encode texture.bmp -p "Still Image PNG (Lossless)" -o texture.png

# Image: Transcode to 16-bit Broadcast TIFF
cargo run --release -p encodercraft-cli -- encode frame.png -p "Broadcast TIFF (16-bit RGB)" -o frame.tiff
```

The GUI, IPC daemon, CLI client, and MCP server share the engine command registry:
```bash
# Start the local IPC daemon, then list or execute registered commands
cargo run --release -p encodercraft-cli -- daemon
cargo run --release -p encodercraft-cli -- command list
cargo run --release -p encodercraft-cli -- command execute queue.list '{}'
```
IPC clients can call `command.list` and `command.execute` (with `commandId` and JSON `params`); MCP clients can use `encodercraft_list_commands` and `encodercraft_execute_command`.

### 3. Running the Watch Folder Daemon
```bash
# Automated ingest with 3-stage settle-delay verification
cargo run --release -p encodercraft-cli -- watch --dir /Volumes/Media/HotFolder/ --preset "YouTube 4K" --out /Volumes/Media/Delivery/
```

### 4. Running the Model Context Protocol (MCP) Server
```bash
# Launch JSON-RPC stdio server for Claude, Antigravity, and AI Agents
cargo run --release -p encodercraft-cli -- mcp
```

---

## 🛡️ Coding Standard: Never Crash

`encoderCraft` is engineered for automated render farms and multi-hour queues where a panic destroys valuable work.

- **Workspace Zero-Panic Lints:**
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
- **Strict `#![forbid(unsafe_code)]`:** Workspace-wide guarantee. Platform hardware encoders are quarantined in `crates/platform` under ADR 0001.
- **Hostile Input Defense:** All input media files (camera cards, network folders) are treated as hostile. Zero unvalidated indexing (`.get()` only), checked/saturating arithmetic, and bounded memory allocations.
- **Bounded Ring Buffers:** Bounded 8-frame ring buffers with backpressure prevent out-of-memory errors on 4K/8K media streams.
- **Poison-Resilient Concurrency:** Mutexes use poison recovery (`lock().unwrap_or_else(PoisonError::into_inner)`).

---

## 🧪 Quality Gate Verification

Before any commit is accepted, the entire workspace must pass the unified `cargo xtask ci` suite:

```bash
cargo run -p xtask -- ci
```
This runs:
1. `cargo fmt --check` (Zero formatting errors)
2. `cargo clippy --workspace --all-targets -- -D warnings` (Zero warnings)
3. `cargo test --workspace` (All 139 unit, fuzz, and integration tests pass)
4. `cargo xtask layers` (Zero cross-layer dependency violations)

---

## 📚 Comprehensive Documentation Suite

- **[Project Roadmap](ROADMAP.md):** Executive metrics, target audience readiness percentages, and progress logs through Milestone 22.
- **[Implementation Milestones](docs/implementation-milestones.md):** Detailed technical log of all deliverables from M0 to M22.
- **[Universal File Format Matrix](docs/file-format-matrix.md):** Exhaustive capability matrix across all 6 media dimensions.
- **[System Architecture](docs/architecture.md):** L0–L7 crate layering, memory ring buffers, backpressure, and panic containment.
- **[Target App Parity Analysis](docs/target-app-parity.md):** Side-by-side feature comparison against Adobe Media Encoder 2026.
- **[Survey of ArtCraft Exporters](docs/artcraft-exporters-survey.md):** Audit of 45+ existing codecs across sibling applications.
- **[Inter-App Dispatch Protocol](docs/inter-app-dispatch-protocol.md):** IPC architecture, Named Pipes, Unix domain sockets, and frame streaming.
- **[Preset System & Watch Folders](docs/preset-system-and-watchfolders.md):** Adaptive bitrate formulas and the 3-stage settle-delay automated ingest daemon.
- **[craft-libs Migration Plan](docs/craft-libs-codecs-migration-plan.md):** Codec centralization strategy for `storytold/craft-libs`.
- **[Agent Instructions](AGENTS.md):** Non-negotiable coding rules and guidelines for AI agents and contributors.
- **[Developer Cheatsheet](CLAUDE.md):** Quick command cheatsheet for development and testing.

---

## 🤝 Upstream Digesting & Attribution

`encoderCraft` is developed by **`el-j`** with the explicit intent of being donated to and digested by the **ArtCraft** organization (`github.com/storytold`).

- **Licensing:** Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
- **Clean-Room:** Strictly black-box observed; 0% proprietary code, 0% GPL/LGPL contamination.
- **Contributor Credits:** Managed in `contributors/people.toml`.
