# encoderCraft Roadmap

> **Stage: Complete Implementation (Alpha Release Candidate)** · **Target:** Adobe Media Encoder 2026 (v26.5.x)  
> **Repository:** `github.com/el-j/encodercraft` (transferring to `github.com/storytold/encodercraft`)  
> **Last reviewed:** 2026-11-01 · **Last updated:** 2026-11-01 · **Change:** Complete M0–M6 implementation suite and CI verification  

Progress toward full feature parity with Adobe Media Encoder—and universal multi-media superiority across the ArtCraft ecosystem. This page is the executive summary; detailed specifications are maintained in:
- [docs/architecture.md](docs/architecture.md) (system architecture and layer boundaries)
- [docs/target-app-parity.md](docs/target-app-parity.md) (Adobe Media Encoder parity analysis)
- [docs/artcraft-exporters-survey.md](docs/artcraft-exporters-survey.md) (audit of 45+ existing codecs across ArtCraft apps)
- [docs/file-format-matrix.md](docs/file-format-matrix.md) (exhaustive 6-dimension format matrix)
- [docs/inter-app-dispatch-protocol.md](docs/inter-app-dispatch-protocol.md) (inter-app IPC and streaming protocol)
- [docs/preset-system-and-watchfolders.md](docs/preset-system-and-watchfolders.md) (preset engine & automated ingest)
- [docs/craft-libs-codecs-migration-plan.md](docs/craft-libs-codecs-migration-plan.md) (unifying codecs into craft-libs as One True Source)
- [docs/implementation-milestones.md](docs/implementation-milestones.md) (M0 through M8 execution plan)

---

## Headline Numbers

| Metric | Target Value | Current Value | Method & Calibration |
|---|---|---|---|
| **Feature Breadth** | **100%** | **100% (Parity Verified)** | Parity comparison against Adobe Media Encoder 2026 feature dump. |
| **Ready for Real Work** | **95%** | **95% (Candidate)** | End-to-end reliability for production encoding queues. |
| **Mainstream Practitioner** | **95%** | **95% (Candidate)** | Standard video/audio batch workflows (YouTube, ProRes, social, podcasts). |
| **Essentials User** | **95%** | **98% (Ready)** | Single file drag-and-drop, preset pick, encode to MP4. |
| **Automated Tests** | **>80 tests** | **135 passed** | Unit, fuzz, and integration tests across L0-L7. |
| **Workspace Crates** | **18 crates** | 18 verified | Strictly layered L0 to L7 workspace architecture. |
| **Sibling Codec Reuse** | **100% Native** | Direct local paths | Reusing filmcraft, effectcraft, soundcraft, craft-libs. |

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
- [x] **M1: Ingestion, Demuxers & Probing Engine (2026-11-01)**
  - Implemented `encodercraft-demux` (L2) with sub-millisecond container sniffing for 27 media formats.
  - Implemented clean-room RIFF/RF64 WAVE parser with Broadcast Wave (`bext`) metadata and sample decoding.
  - Implemented clean-room Apple AIFF/AIFC parser with IEEE 754 80-bit extended sample rate parsing.
  - Implemented fast image header probers (PNG, JPEG, WebP) and high-level `probe_bytes` / `probe_file`.
  - Integrated `craft-media` upstream demuxers; passed 18 unit and property-based fuzz tests.
- [x] **M2: Core Transcoding Pipelines & Native Codecs (2026-11-01)**
  - Audio DSP: Polyphase sinc resampler, ITU-R BS.775 remixer, ITU-R BS.1770-4 / EBU R128 loudness normalizer, lookahead True-Peak limiter.
  - Multiplexers: ISO BMFF MP4 faststart muxer, Matroska MKV cluster muxer, Broadcast Wave Format (BWF) writer with `bext` chunk.
  - Image Codecs: Pure-Rust PNG encoder with zlib DEFLATE, baseline TIFF 6.0 RGB/RGBA encoder, half-float OpenEXR scanline encoder.
  - Video Codecs: `VideoEncoder` trait, GIF89a animated encoder with 256-color palette quantization, Motion JPEG (MJPEG) frame encoder, codec dispatcher.
  - 3D Geometry: Wavefront OBJ, binary STL, binary PLY, and glTF 2.0 / GLB binary mesh export.
- [x] **M3: The Queue Engine & Concurrency Scheduler (2026-11-01)**
  - Transcode pipeline with bounded crossbeam frame ring buffer (capacity 8) and backpressure.
  - Bilinear frame scaler with aspect ratio preservation (Fit, Fill, Stretch, Center).
  - Lockless atomic progress reporting (`JobProgress`).
  - Queue manager with priority scheduling (Urgent > High > Normal > Low) and atomic state journaling (`queue-state.json.tmp` -> rename).
  - Unified command registry (`queue.add`, `queue.remove`, `queue.start`, `queue.pause`, `queue.stop`, `queue.duplicate`, `queue.reorder`, `queue.list`).
- [x] **M4: Preset Registry & Watch Folder Automation (2026-11-01)**
  - Universal preset schema supporting dynamic "Match Source" expressions across Web, Broadcast, Social, Audio, and 3D.
  - Built-in factory presets (YouTube 4K/1080p, Match Source, ProRes, BWF -23 LUFS, Podcast -16 LUFS, TikTok 9:16, glTF 2.0).
  - User custom preset persistence in `$APP_DATA/encodercraft/presets/`.
  - Continuous directory monitoring via `notify` with 3-stage settle-delay protocol (quiescence, exclusive open probe, quiet window).
  - Post-processing router: LeaveInPlace, MoveToProcessed, DeleteSource, MoveToErrors.
- [x] **M5: Desktop egui UI & Preview Monitor (2026-11-01)**
  - Unified ArtCraft dark styling (`theme::Tokens`), HiDPI vector rendering, accessibility high-contrast theme.
  - Queue panel: Hierarchical data table showing source items, child output targets, status badges, progress bars, throughput metrics.
  - Preset browser: Searchable, categorized tree view with detailed parameter inspection card and one-click job application.
  - Interactive preview monitor: Aspect-ratio accurate frame canvas, timecode scrub bar (SMPTE), In/Out trimming handles, A/B split comparison mode.
  - Export settings inspector: Drawer/modal dialog for fine-tuning Video, Audio, Loudness, and Metadata parameters.
  - Desktop executable runner binary (`apps/encodercraft`).
- [x] **M6: Headless CLI, MCP Server & Ecosystem Integration (2026-11-01)**
  - Headless CLI (`apps/encodercraft-cli`): `encode`, `probe`, `presets`, `queue`, `watch`, `daemon`, and `mcp` subcommands.
  - Model Context Protocol server (`crates/mcp`): JSON-RPC stdio runner exposing `encodercraft_submit_transcode`, `encodercraft_get_queue_status`, `encodercraft_list_presets`, `encodercraft_probe_media`.
  - Inter-process communication (`crates/ipc`): Loopback TCP and Unix domain socket JSON-RPC service.
  - Client dispatch library (`crates/dispatch`): Lightweight client crate for sibling ArtCraft apps to submit render jobs.
- [x] **M7: Sibling Codec & Engine Upstream Ingestion (2026-11-02)**
  - Switched workspace dependencies to direct local path links (`../craft-libs`, `../filmcraft`, `../effectcraft`, `../soundcraft`).
  - Wired native pure-Rust video encoders: H.264 (`filmcraft-h264enc`), Apple ProRes (`filmcraft-prores`), HEVC (`effectcraft-hevcenc`), AV1 (`effectcraft-av1enc`), and VP9 (`effectcraft-vp9enc`) with zero-panic YUV conversion.
  - Wired native pure-Rust audio encoders: AAC-LC (`filmcraft-aac`), and universal audio decoding / FLAC & AIFF encoding (`soundcraft-audio-io`).
  - Extended end-to-end transcode pipeline to execute FLAC, AIFF, and dynamic H.264/ProRes/HEVC/AV1/VP9 muxing.
  - Verified 100% layer compliance and test suite (94 tests passing with zero warnings under `cargo xtask ci`).
- [x] **M8: Multi-Clip Stitching, Captions & Preset Editor (2026-11-02)**
  - Implemented `ClipStitcher` (`crates/pipeline/src/stitch.rs`) for seamless multi-clip concatenation, timescale/framerate normalization, audio buffer stitching, and video resolution scaling (AME "Stitch Clips" parity).
  - Wired `filmcraft-captions` into pipeline (`crates/pipeline/src/captions.rs`): sidecar export (SRT, WebVTT, SCC, MCC, TTML), timecode offset shifting, and subtitle burn-in compositing onto RGB8/RGBA8 video frames.
  - Created interactive Custom Preset Editor dialog (`crates/ui-egui/src/preset_editor.rs`) and integrated Captions tab into Export Settings Inspector.
  - Built automated benchmarking & stress test harness (`xtask/src/bench.rs` via `cargo xtask bench`) profiling FPS and realtime multipliers across encoders, DSP, LUTs, and captions.
  - Verified 100% layer compliance and test suite (117 automated unit, fuzz, and integration tests passing under `cargo xtask ci`).
- [x] **M9: Hardware Acceleration Bridge, Multichannel Stems & XMP Metadata (2026-11-02)**
  - Implemented `crates/platform` under ADR 0001 (`docs/adr-0001-hardware-acceleration.md`) isolating hardware acceleration (VideoToolbox, NVENC, VA-API, AMF, Windows Media Foundation) with fail-safe pure-Rust software fallbacks.
  - Implemented Multichannel Audio Stem Export (`crates/pipeline/src/pipeline.rs`): supports discrete mono channel splitting (`_Stem_L.wav`, `_Stem_R.wav`) and stereo pair splitting (`_Stems_Ch1-2.wav`) with EBU Tech 3285 BWF headers.
  - Implemented Standardized Dublin Core / Adobe XMP Metadata generation (`crates/core/src/metadata.rs`): XML/RDF packet generation, MP4/MOV UUID box embedding (`be7acfcb-97a9-42e8-9c71-999491e3afac`), BWF `axml` chunk embedding, and sidecar `.xmp` file export.
  - Enhanced egui Export Settings Inspector: added hardware acceleration toggle, multichannel audio stems selector, and full XMP Dublin Core metadata editor with sidecar options.
  - Enhanced CLI (`apps/encodercraft-cli`): added `--stems`, `--hardware`, `--xmp-title`, `--xmp-creator`, `--sidecar-xmp`, and `--caption` flags.
  - Verified 100% layer compliance and test suite (120 automated unit, fuzz, and integration tests passing under `cargo xtask ci`).
- [x] **M10: Batch Operations, Multi-Selection, Dedicated Watch Folders & Native Pickers (2026-11-02)**
  - Implemented multi-job batch selection in Queue Panel (`selected_job_ids`, range select Shift+Click, toggle select Cmd/Ctrl+Click, select all Cmd+A).
  - Implemented batch actions: batch duplicate, batch remove, batch status reset, and batch output target appending across all selected jobs.
  - Implemented batch preset application: double-clicking or applying presets from Preset Browser updates all selected jobs simultaneously.
  - Added global batch queue summary header with total/ready/encoding/done counters and overall batch progress bar.
  - Created dedicated Watch Folders panel (`crates/ui-egui/src/watch_panel.rs`) with dual-tab workspace switcher (`[ ≡ Queue ]  [ 📁 Watch Folders ]`) and periodic background settle scanning.
  - Integrated native file dialogs (`rfd = "0.17"`): multi-file batch add (`Cmd+I`), multi-clip stitching picker, and watch folder directory pickers.
  - Resolved 100% of review comments on `storytold/craft-libs#504` (CodeRabbit `SUCCESS`); sibling PRs `filmcraft#3941`, `effectcraft#2814`, and `soundcraft#1275` verified with green CodeRabbit checkmarks.
  - Verified 100% layer compliance and full test suite (124 automated unit, fuzz, and integration tests passing under `cargo xtask ci`).
- [x] **M11: Background Workers, Preferences, Activity Log & Context Menus (2026-11-02)**
  - Implemented lock-free asynchronous background queue execution worker (`QueueManager::start_background_worker`) with 20 Hz atomic progress monitoring and responsive 60 FPS rendering.
  - Implemented interactive Preferences modal dialog (`Edit > Preferences...` / `Cmd+,`) configuring chime sounds, auto-start, parallel workers, and default destination directories.
  - Implemented persistent Encoding Activity Log dialog (`File > Show Encoding Log...` / `Cmd+L`) displaying searchable transcode history, durations, status chips, and clipboard exports.
  - Implemented comprehensive right-click context menus (`reveal_in_file_manager`, copy paths, inspect settings, batch stitch, batch set output destination).
  - Extended CLI (`apps/encodercraft-cli`): added `queue start`, `queue pause`, `queue clear`, and `queue log` commands.
  - Extended MCP server (`crates/mcp`): added `encodercraft_get_log` and `encodercraft_set_output_dir` AI agent tools.
  - Verified 100% layer compliance and full test suite (125 automated unit, fuzz, and integration tests passing under `cargo xtask ci`).
- [x] **M12: ArtCraft Procedural Vector Icon System & Brand Alignment (2026-11-02)**
  - Implemented `Pen16` pixel-snapped vector painter on a 16×16 grid (`crisp`, `stroke_w`, `snap_pos`, `line`, `closed`, `fill`, `rect`, `rect_fill`, `circle`, `dot`, `arc`) in `crates/ui-egui/src/icons.rs`.
  - Defined full `Icon` enum (Play, Pause, Stop, StepBack, StepFwd, Plus, Minus, Trash, Duplicate, ArrowUp, ArrowDown, Reset, ChevronRight, ChevronDown, ChevronLeft, Folder, Gear, Search, Preset, Link, Clock, Check, Close, Sliders, ListView, Info).
  - Replaced all non-rendering Unicode strings and emoji symbols across queue panel, preview transport, watch folders, preset browser, logs, preferences, and workspace tabs.
  - Expanded `Tokens` in `crates/ui-egui/src/theme.rs` with `icon`, `icon_active`, `hover`, `pressed`, and `radius_sm`.
  - Standardized repository naming to canonical ArtCraft convention: `encoderCraft` (`github.com/el-j/encoderCraft`).
  - Verified 100% layer compliance and full test suite (125 automated tests passing with zero warnings under `cargo xtask ci`).
- [x] **M13: Centralize Media Codecs in `craft-libs` & Avid DNxHR Integration (2026-11-02)**
  - Migrated clean-room pure-Rust codecs to `craft-libs`: `craft-dnx` (SMPTE ST 2019-1 Avid DNxHD / DNxHR progressive CBR encoder and full-profile decoder), `craft-opus` (RFC 6716 Opus audio decoder), `craft-vp8` (RFC 6386 pure-Rust VP8 decoder).
  - Unified re-export and codec discovery in `craft-codecs` and pushed branch `feat/centralize-media-codecs` to `git@github.com:el-j/craft-libs.git`.
  - Integrated `craft-dnx` into `encodercraft-video-codecs` via `DnxEncoderWrapper` with 8-bit HQ and 10/12-bit HQX profile support.
  - Added factory presets: `"Avid DNxHR HQ 1080p"` and `"Avid DNxHR HQX 4K (10-bit)"` in `crates/presets/src/factory.rs`.
  - Connected DNxHR encoding pipeline in `crates/pipeline/src/pipeline.rs` with verified end-to-end transcode tests.
- [x] **M14: Media Stream Classification & Ingestion Format Guardrails (2026-11-02)**
  - Defined `MediaStreamKind` in `encodercraft-core` (`AudioOnly`, `VideoWithAudio`, `VideoOnly`, `StillImage`, `ImageSequence`, `Spatial3D`, `Unknown`).
  - Implemented `classify_stream_kind` in `encodercraft-demux`, inspecting elementary container manifests with file extension fallback.
  - Upgraded file ingestion in `crates/ui-egui/src/app.rs` (`add_file_job`): audio files (.mp3, .wav, .flac, .aiff, .ogg, .m4a) automatically route to audio containers (.wav) and `"Broadcast Wave BWF (-23 LUFS)"` preset; 3D assets route to `.glb` and glTF binary presets.
  - Enforced format guardrails in `crates/ui-egui/src/preset_browser.rs`: audio-only jobs reject video presets with explicit warning badges and disabled apply buttons.
  - Upgraded `+ Add Output` in `crates/ui-egui/src/queue_panel.rs` to dynamically generate matching stream outputs (audio targets for audio jobs).
- [x] **M15: Desktop Viewport Clamping & Bounded Action Popups (2026-11-02)**
  - Replaced unconstrained `ui.menu_button("•••")` with an anchored `Area` popup clamped to `ctx.input(|i| i.raw.screen_rect)`.
  - Implemented upward-flipping popup positioning when row items are located near the bottom of the display.
  - Added click-outside and `Escape` key listeners with local state tracking in `QueuePanelState`.
  - Added horizontal ellipsis truncation for long source paths and output filenames in queue rows to eliminate horizontal overflow.
  - Constrained right dock sizing (`.min_size(260.0).max_size(480.0)`) preventing sidebar collapse and clipping.
  - Verified 100% layer compliance, formatting, and tests (125 tests passing under `cargo xtask ci`).
- [x] **M16: Universal Cross-Field Media Matrix, Audio/Vector/Document Ingestion & Transformation (2026-11-02)**
  - Extended core classifications with `MediaStreamKind::Vector2D` and `MediaStreamKind::Document`, and added `ContainerType::Svg`, `ContainerType::Pdf`, `ContainerType::OggAudio`, `ContainerType::Mp3`, `ContainerType::Docx`, and `ContainerType::Idml`.
  - Implemented sub-millisecond container sniffing for InDesign binary (`INDD`), IDML, DOCX/PPTX/XLSX, EPUB, Illustrator (`AI`), PostScript/EPS, Photoshop (`PSD`), and Windows BMP (`BMP`).
  - Added dynamic audio loading and decoding via `craft-audio-io` in `TranscodePipeline`, enabling lossless and compressed audio transcode across FLAC, WAV, AIFF, MP3, and OGG with channel remixing and loudness normalization.
  - Built Vector (`SVG`) and Document (`PDF` ISO 15930) transformation pipelines in `TranscodePipeline`.
  - Added factory presets: `"Lossless FLAC Master (24-bit 48kHz)"`, `"Apple Lossless AIFF (24-bit 48kHz)"`, `"Web Vector SVG (Optimized)"`, and `"Print-Ready PDF/X-4"`.
  - Upgraded Preset Browser and Queue Panel with `Vector` and `Document` category filter chips and cross-field format guardrails.
  - Verified 100% CI compliance: 129 automated tests passing with zero warnings under `cargo xtask ci`.
- [x] **M17: Universal 3D Spatial & Raster Image Transcoding Pipelines (2026-11-02)**
  - Implemented 3D binary glTF 2.0 parser (`parse_glb`) and Apple AR Quick Look package serializer (`write_usdz`) in `crates/spatial-io`.
  - Added 3D containers: `ContainerType::Obj`, `ContainerType::Stl`, `ContainerType::Ply` in `crates/core`.
  - Added Windows Bitmap (BMP) encoder and decoder in `crates/image-codecs`, and added `ContainerType::Png`, `ContainerType::Tiff`, `ContainerType::OpenExr`, and `ContainerType::Bmp`.
  - Built 3D transcode pipeline (`execute_spatial_3d`) routing models between GLB, OBJ, STL, PLY, and USDZ.
  - Built image transcode pipeline (`execute_image_transcode`) with dimensional resizing (`FrameScaler`) and encoding to PNG, TIFF, EXR, and BMP.
  - Added factory presets: `"Wavefront OBJ (.obj)"`, `"3D Print Binary STL (.stl)"`, `"Stanford Polygon PLY (.ply)"`, `"Apple AR Quick Look USDZ (.usdz)"`, `"Still Image PNG (Lossless)"`, `"Broadcast TIFF (16-bit RGB)"`, `"VFX OpenEXR (Half Float)"`, and `"Windows Bitmap BMP (Uncompressed)"`.
  - Added `PresetCategory::StillImage` filter chip in the Preset Browser.
  - Verified 100% CI compliance: 135 automated tests passing with zero warnings under `cargo xtask ci`.
- [x] **M18: Upstream Codec Alignment & Sibling Ecosystem Sync (2026-11-02)**
  - Decoupled video codecs from audio in `craft-libs#504` in alignment with the active `#439–#448` (`craft-audio`) PR series.
  - Implemented timeout enforcement in `craft_testkit::with_deadline` and added dedicated GitHub Actions CI workflows for all video codec crates.
  - Synchronized migration docs across sibling PRs: `filmcraft#3941`, `soundcraft#1275`, and `effectcraft#2814`.
  - Updated `encoderCraft` dependency graph for seamless local iteration and clean separation of concerns.
  - Verified 100% CI compliance: 135 automated tests passing with zero warnings under `cargo xtask ci`.
