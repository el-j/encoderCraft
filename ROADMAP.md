# encoderCraft Roadmap

> **Stage: Planning & Architecture (Pre-Alpha)** · **Target:** Adobe Media Encoder 2026 (v26.5.x)  
> **Repository:** `github.com/el-j/encodercraft` (transferring to `github.com/storytold/encodercraft`)  
> **Last reviewed:** 2026-11-01 · **Last updated:** 2026-11-01 · **Change:** Initial architecture & full implementation planning suite  

Progress toward full feature parity with Adobe Media Encoder—and universal multi-media superiority across the ArtCraft ecosystem. This page is the executive summary; detailed specifications are maintained in:
- [docs/architecture.md](docs/architecture.md) (system architecture and layer boundaries)
- [docs/target-app-parity.md](docs/target-app-parity.md) (Adobe Media Encoder parity analysis)
- [docs/artcraft-exporters-survey.md](docs/artcraft-exporters-survey.md) (audit of 45+ existing codecs across ArtCraft apps)
- [docs/file-format-matrix.md](docs/file-format-matrix.md) (exhaustive 6-dimension format matrix)
- [docs/inter-app-dispatch-protocol.md](docs/inter-app-dispatch-protocol.md) (inter-app IPC and streaming protocol)
- [docs/preset-system-and-watchfolders.md](docs/preset-system-and-watchfolders.md) (preset engine & automated ingest)
- [docs/implementation-milestones.md](docs/implementation-milestones.md) (M0 through M6 execution plan)

---

## Headline Numbers

| Metric | Target Value | Current Value | Method & Calibration |
|---|---|---|---|
| **Feature Breadth** | **100%** | **~85% (Planned)** | Parity comparison against Adobe Media Encoder 2026 feature dump. |
| **Ready for Real Work** | **95%** | **0% (Pre-Alpha)** | End-to-end reliability for production encoding queues. |
| **Mainstream Practitioner** | **95%** | **0% (Pre-Alpha)** | Standard video/audio batch workflows (YouTube, ProRes, social, podcasts). |
| **Essentials User** | **95%** | **0% (Pre-Alpha)** | Single file drag-and-drop, preset pick, encode to MP4. |
| **Est. Agent Hours to Beta** | **~350–500 h** | — | Calibrated against PR velocity across ArtCraft sibling repositories. |
| **Est. Agent Hours to 1.0** | **~420–670 h** | — | Full desktop UI, watch folder daemon, MCP server, and sibling app PRs. |
| **Workspace Crates** | **15 crates** | 15 designed | Layered L0 to L7 workspace architecture. |

---

## Readiness by Audience

| Audience Profile | Target Parity | Est. Agent Hours to 95% | Dominant Work Items |
|---|---|---|---|
| **Essentials User** | ~82% | 120–180 h | Drag-and-drop ingest, MP4/H.264/AAC muxer, "Match Source" preset, basic egui queue. |
| **Mainstream Practitioner** | ~68% | 220–320 h | Multi-target outputs, ProRes/DNxHR, watch folders with settle-delay, EBU R128 loudness. |
| **Full Post & Enterprise** | ~52% | 380–550 h | Hardware acceleration, MXF OP-1a, SMPTE timecodes, 3D LUTs, multi-channel stems, MCP. |

---

## Progress by Dimension

| Dimension | Readiness | Est. Hours | Target Specification |
|---|---|---|---|
| **Core Architecture & Models** | Planned (100% specified) | 30–50 h | [docs/architecture.md](docs/architecture.md) |
| **Format Ingestion & Demuxers** | Planned (Surveyed) | 50–80 h | [docs/artcraft-exporters-survey.md](docs/artcraft-exporters-survey.md) |
| **Native Codecs & DSP Filters** | Planned (Existing in Craft) | 100–160 h | [docs/file-format-matrix.md](docs/file-format-matrix.md) |
| **Queue Engine & Concurrency** | Planned (100% specified) | 60–90 h | [docs/architecture.md §3](docs/architecture.md#3-concurrency--memory-architecture) |
| **Preset Engine & Watch Folders**| Planned (100% specified) | 40–70 h | [docs/preset-system-and-watchfolders.md](docs/preset-system-and-watchfolders.md) |
| **egui Desktop User Interface** | Planned (Designed) | 90–140 h | [docs/target-app-parity.md](docs/target-app-parity.md) |
| **CLI, MCP & App Integration** | Planned (100% specified) | 50–80 h | [docs/inter-app-dispatch-protocol.md](docs/inter-app-dispatch-protocol.md) |

---

## Implementation Milestones & Log

- [x] **M0.0: Comprehensive Planning & Exporter Survey Complete (2026-11-01)**
  - Audited all 15+ ArtCraft repositories (`filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, `texturecraft`, `designcraft`, `vectorcraft`, `pdfcraft`, `cadcraft`, `craft-libs`).
  - Documented complete architecture, 6-dimension format matrix, inter-app dispatch protocol, and preset/watchfolder specifications.
- [x] **M0: Workspace Scaffolding & Core Architecture (2026-11-01)**
  - Setup root `Cargo.toml`, strict lints (`clippy::unwrap_used`), `xtask` quality gates (`ci`, `layers`).
  - Implemented `CraftJobSpec`, `MediaTimecode`, `AudioSampleBuffer`, `VideoFrame`, `Rational64`, and typed error enums.
  - Verified 100% pass across 20 unit and property-based fuzz tests; verified layer compliance.
- [ ] **M1: Ingestion, Demuxers & Probing Engine (Active)**
  - Fast format probers for ISO BMFF, Matroska, MXF, WAV, AIFF, EXR, glTF, USD.
  - Bounded packet stream extractors and keyframe indexers.
- [ ] **M2: Core Transcoding Pipelines & Native Codecs**
  - Integrate pure-Rust video codecs (H.264, ProRes, DNx, APV, AV1, HEVC, VP9).
  - Integrate audio DSP (polyphase resampler, EBU R128 loudness, True-Peak limiter).
  - Integrate image and 3D mesh converters (EXR, TIFF, JXL, USD, glTF).
- [ ] **M3: The Queue Engine & Concurrency Scheduler**
  - Bounded 8-frame ring buffers with backpressure.
  - Lockless atomic progress reporting (`AtomicU64`) and cancellation flags.
  - Atomic queue journaling (`queue-state.json`) with startup recovery.
- [ ] **M4: Preset Registry & Watch Folder Automation**
  - Curated factory presets and user JSON preset serializer.
  - Cross-platform watch folder daemon with 3-stage settle-delay stabilization.
- [ ] **M5: Desktop egui UI & Preview Monitor**
  - Queue data table, preset browser tree, interactive canvas monitor with scrub bar.
  - Side-by-side Source vs. Output comparison view.
- [ ] **M6: Headless CLI, MCP Server & Ecosystem Integration**
  - `encodercraft-cli` batch runner and `encodercraft-mcp` tool suite.
  - Publish `craft-dispatch` client crate to `craft-libs`.
  - Upstream PRs into FilmCraft, EffectCraft, and SoundCraft for "Queue in EncoderCraft" hand-off.
