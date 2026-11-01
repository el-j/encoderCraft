# encoderCraft

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-APACHE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org)
[![Parity Target](https://img.shields.io/badge/parity%20target-Adobe%20Media%20Encoder%202026-purple.svg)](docs/target-app-parity.md)
[![Safety](https://img.shields.io/badge/unsafe_code-forbid-brightgreen.svg)](docs/architecture.md)
[![ArtCraft Ecosystem](https://img.shields.io/badge/artcraft-storytold-red.svg)](https://github.com/storytold)

**encoderCraft** is an open-source, clean-room, pure-Rust media transcoding, batch queue, and watch folder automation system. Designed to provide parity with—and architectural superiority over—**Adobe Media Encoder**, `encoderCraft` acts as the universal export powerhouse for the entire **ArtCraft** creative ecosystem.

Originally developed under username **`el-j`** (`github.com/el-j/encodercraft`), `encoderCraft` is built according to ArtCraft's exacting standards to prepare for official digestion into the **`storytold`** organization (`github.com/storytold/encodercraft`).

---

## 🌟 Why encoderCraft?

Adobe Media Encoder has long been the default render farm tool for video editors. However, modern creative studios operate across multiple media domains that legacy encoders ignore:

| Dimension | Legacy Encoders (Adobe Media Encoder) | encoderCraft |
|---|---|---|
| **Video & Motion** | MP4, MOV, MXF, AVI, WebM. | **Full Parity:** H.264, ProRes 4444, HEVC, AV1, DNxHR, APV, MXF OP-1a. |
| **Audio & Music** | AAC, MP3, WAV. | **Full Parity + Broadcast DSP:** BWF, RF64, CAF, FLAC, EBU R128 (-23/-14 LUFS). |
| **3D & Spatial Assets** | ❌ None. | **Universal:** USD/USDC, glTF 2.0 / GLB, FBX, Wavefront OBJ, STL, PLY. |
| **Raster & Textures** | Basic JPEG/PNG. | **Professional:** OpenEXR (deep/layers), JXL, 16-bit TIFF, BC7 DDS, PBR ORM. |
| **Vector & Print** | ❌ None. | **Prepress:** PDF/X-4 print-ready with bleed, SVG/SVGZ, EPS, IDML. |
| **Crash Resilience** | ⚠️ Frequent crashes on corrupt files. | **Never Crash:** Pure Rust, `#![deny(clippy::unwrap_used)]`, all input hostile. |
| **Automation & AI** | Limited proprietary ExtendScript. | **Autonomous:** Headless CLI, Unix IPC sockets, and native **MCP Server** for AI agents. |
| **Footprint & Licensing** | >3 GB bloat, telemetry, subscription DRM. | **Lightweight:** <50 MB standalone binary, zero telemetry, Apache-2.0 / MIT. |

---

## 🏗️ Architecture & Sibling Ecosystem

`encoderCraft` does not reinvent the wheel: it aggregates and supercharges the proven, pure-Rust exporters already authored across the `storytold` family:

```
┌────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                       ArtCraft Creative Suite                                          │
│  FilmCraft · EffectCraft · SoundCraft · PhotoCraft · TextureCraft · DesignCraft · VectorCraft · PdfCraft│
└───────────────────────────────────────────────────┬────────────────────────────────────────────────────┘
                                                    │ "Queue in EncoderCraft"
                                                    │ (craft-dispatch IPC & Shared Memory)
                                                    ▼
┌────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                           encoderCraft                                                 │
│                                                                                                        │
│  ┌───────────────────────────┐  ┌─────────────────────────────┐  ┌──────────────────────────────────┐  │
│  │   Desktop UI (egui)       │  │   Headless CLI & Daemon     │  │   Model Context Protocol (MCP)   │  │
│  │   • Multi-target queue    │  │   • Batch encoding scripts  │  │   • Autonomous agent control     │  │
│  │   • Preset browser tree   │  │   • Background watch folder │  │   • Job status & inspection      │  │
│  │   • Live preview monitor  │  │   • CI/CD pipeline render   │  │   • Automated batch conversion   │  │
│  └─────────────┬─────────────┘  └──────────────┬──────────────┘  └────────────────┬─────────────────┘  │
│                │                               │                                  │                    │
│                └───────────────────────────────┼──────────────────────────────────┘                    │
│                                                ▼                                                       │
│                           ┌─────────────────────────────────────────┐                                  │
│                           │      encoder-engine (L5 Session)        │                                  │
│                           │      • Priority Queue Scheduler         │                                  │
│                           │      • Atomic State Journaling          │                                  │
│                           │      • Cancellation Atomics             │                                  │
│                           └────────────────────┬────────────────────┘                                  │
│                                                │                                                       │
│         ┌──────────────────────────────────────┴───────────────────────────────────────┐                 │
│         ▼                                      ▼                                      ▼                 │
│  ┌──────────────┐                       ┌──────────────┐                       ┌──────────────┐         │
│  │ Video Codecs │                       │  Audio DSP   │                       │ Spatial & 2D │         │
│  │ • H.264/HEVC │                       │ • EBU R128   │                       │ • USD/glTF   │         │
│  │ • ProRes     │                       │ • Resampler  │                       │ • EXR/TIFF   │         │
│  │ • AV1 / VP9  │                       │ • AAC / FLAC │                       │ • PDF/X-4    │         │
│  │ • MXF OP-1a  │                       │ • BWF / CAF  │                       │ • PBR ORM    │         │
│  └──────────────┘                       └──────────────┘                       └──────────────┘         │
│                                                │                                                       │
│                                                ▼                                                       │
│                               ┌─────────────────────────────────┐                                      │
│                               │ Upstream craft-libs Shared Core │                                      │
│                               │ • craft-media (MP4/MOV/MKV)     │                                      │
│                               │ • craft-cms (ICC Color Mgmt)    │                                      │
│                               │ • craft-jpeg / craft-crw        │                                      │
│                               └─────────────────────────────────┘                                      │
└────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📚 Comprehensive Documentation Suite

Every aspect of `encoderCraft` is thoroughly specified in clean markdown documentation:

- **[Survey of ArtCraft Exporters](docs/artcraft-exporters-survey.md):** Complete audit of 45+ existing codecs, muxers, and libraries across `filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, `texturecraft`, and `craft-libs`.
- **[Universal File Format Matrix](docs/file-format-matrix.md):** Exhaustive capability matrix across 6 media dimensions (Video, Audio, Raster, Vector, 3D, and Documents).
- **[Inter-App Dispatch Protocol](docs/inter-app-dispatch-protocol.md):** How FilmCraft, EffectCraft, and third-party tools send jobs via Unix domain sockets, Named Pipes, and streaming frames.
- **[Preset System & Watch Folders](docs/preset-system-and-watchfolders.md):** Specification of dynamic "Match Source" presets, adaptive bitrate formulas, and the 3-stage settle-delay automated ingest daemon.
- **[Target App Parity Analysis](docs/target-app-parity.md):** Deep side-by-side comparison against Adobe Media Encoder 2026.
- **[System Architecture](docs/architecture.md):** L0–L7 crate layering, bounded ring buffers, backpressure, atomic progress, and panic containment.
- **[Implementation Milestones](docs/implementation-milestones.md):** Step-by-step phased execution plan from M0 to M6.
- **[Project Roadmap](ROADMAP.md):** Executive metrics, audience readiness percentages, and milestone tracking.
- **[Agent Guidelines](AGENTS.md):** Non-negotiable coding rules, "never crash" guidelines, and clippy standards.
- **[Assistant Reference](CLAUDE.md):** Quick command cheatsheet for agents and developers.

---

## ⚡ Quickstart

### Running the Desktop GUI
```bash
cargo run --release -p encodercraft
```

### Running the Headless CLI
```bash
# Convert a video with dynamic "Match Source" high bitrate
encodercraft encode input.mov -p "Match Source - Adaptive High Bitrate" -o output.mp4

# Transcode an audio master to broadcast EBU R128 compliant WAV (-23 LUFS)
encodercraft encode mix_master.wav -p "Broadcast Wave EBU R128" -o master_ebu.wav

# Convert a 3D asset from FBX to Apple AR QuickLook USDZ
encodercraft encode character.fbx -p "Apple AR QuickLook USDZ" -o character.usdz
```

### Launching the Watch Folder Daemon
```bash
encodercraft watch --dir /Volumes/Media/HotFolder/ --preset "YouTube 4K" --out /Volumes/Media/Delivery/
```

### Starting the Model Context Protocol (MCP) Server
```bash
encodercraft mcp
```

---

## 🛡️ Coding Standard: Never Crash

All non-test crates enforce zero-panic safety:
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
- `#![forbid(unsafe_code)]` workspace-wide (isolated platform hardware acceleration quarantined in `crates/platform` under ADR 0001).
- All input-derived data is treated as hostile: bounded allocations, checked arithmetic, no direct slice indexing.
- Bounded 8-frame ring buffers prevent out-of-memory errors on 4K/8K media streams.

---

## 🤝 Upstream Digesting & Attribution

`encoderCraft` is developed by **`el-j`** with the explicit intent of being donated to and digested by the **ArtCraft** organization (`github.com/storytold`).

- **Licensing:** Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
- **Clean-room:** Strictly black-box observed; 0% proprietary code, 0% GPL/LGPL contamination.
- **Contributor Credits:** Managed in `contributors/people.toml`.
