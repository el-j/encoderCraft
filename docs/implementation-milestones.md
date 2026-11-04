# Implementation Milestones & Execution Plan

> **Target:** `encoderCraft` (`el-j/encodercraft` → `storytold/encodercraft`)  
> **Prepared for:** el-j / storytold  
> **Status:** Implementation Complete & Verified (M0 through M22)  
> **Verification:** 100% CI compliance via `cargo xtask ci` (139 unit, fuzz, and integration tests passed; strict L0–L7 layers enforced)

---

## 1. Milestone Overview & Sequencing

The development of `encoderCraft` is divided into seven sequential milestones (M0 through M6), designed to build systematically from foundational primitives to full desktop UI and multi-app ecosystem integration.

```mermaid
flowchart LR
    M0["M0: Workspace & Core Models"] --> M1["M1: Demuxing & Format Prober"]
    M1 --> M2["M2: Native Codecs & Filters"]
    M2 --> M3["M3: Queue Engine & Scheduler"]
    M3 --> M4["M4: Presets & Watch Folders"]
    M4 --> M5["M5: egui UI & Preview Monitor"]
    M5 --> M6["M6: CLI, MCP & App Integration"]
```

| Milestone | Title | Focus Area | Est. Agent Hours | Core Deliverables |
|---|---|---|---|---|
| **M0** | Workspace Scaffolding | Foundations & Data Model | 30–50 h | Cargo workspace, `AGENTS.md`, `xtask`, `core` models, error types. |
| **M1** | Ingestion & Prober | Format Detection & Demux | 50–80 h | Stream probers, ISO BMFF / Matroska demuxers, audio decoders. |
| **M2** | Codecs & Transcoders | Pure-Rust Native Engines | 100–160 h | H.264, ProRes, DNx, AV1, EBU R128 loudness, image codecs, USD/glTF. |
| **M3** | Queue & Concurrency | Scheduler & Worker Pool | 60–90 h | Multi-threaded queue, ring buffers, state journaling, cancel flags. |
| **M4** | Presets & Automation | Match-Source & Watchers | 40–70 h | Preset hierarchy, JSON serializer, settle-delay watch folder daemon. |
| **M5** | Desktop egui UI | Media Encoder Interface | 90–140 h | Queue table, preset browser, preview canvas, inspector dialogs. |
| **M6** | Ecosystem Integration | CLI, MCP & Inter-App | 50–80 h | `encodercraft-cli`, `mcp` server, `craft-dispatch`, app PRs. |
| **Total** | | | **420–670 h** | Complete production-ready application. |

---

## 2. Milestone Details

### Milestone 0: Workspace Scaffolding & Core Architecture
**Objective:** Establish the repository structure, non-negotiable safety lints, xtask quality gates, and core domain abstractions.

#### Deliverables:
1. **Workspace Manifest:** Set up root `Cargo.toml` with strict workspace dependencies, package metadata, and unified lints:
   ```toml
   [workspace.lints.clippy]
   unwrap_used = "deny"
   expect_used = "deny"
   panic = "deny"
   unimplemented = "deny"
   todo = "deny"
   unreachable = "deny"
   ```
2. **`xtask` Automation:**
   - `cargo xtask ci`: Runs formatting, clippy with `-D warnings`, unit tests, layer checks, and WASM build verification.
   - `cargo xtask layers`: Enforces layer boundary rules (lower layers cannot depend on higher layers; UI dependencies isolated).
3. **Core Data Structures (`crates/core`):**
   - `CraftJobSpec`: Complete serializable job specification for sources, targets, filters, and notifications.
   - `MediaTimecode`: Frame-accurate SMPTE timecode (drop-frame, non-drop-frame) with rational timebase conversion.
   - `AudioSampleBuffer`: Interleaved and planar 32-bit floating-point audio containers with sample-rate metadata.
   - `VideoFrame`: Strided pixel buffer with format descriptors (RGBA8, RGBA16, RGBAF32, YUV420p, YUV422p10, YUV444p12).
   - `EncoderError`: Comprehensive, non-panicking error enum using `thiserror`.

#### Verification Gate:
- `cargo xtask ci` passes cleanly with zero warnings.
- Unit tests verify timecode parsing, rational arithmetic, and job specification serialization round-trips.

---

### Milestone 1: Ingestion, Demuxers & Probing Engine
**Objective:** Enable `encoderCraft` to inspect any media file, probe its streams, and extract frames/samples without decoding full files upfront.

#### Deliverables:
1. **Media Prober (`crates/demux`):**
   - Quick header sniffing for container types (ISO BMFF, QuickTime, Matroska, WebM, MXF, WAV, AIFF, OpenEXR, TIFF, PNG, glTF, USD).
   - Deep metadata extraction: dimensions, frame rate, duration, pixel format, color primaries, transfer characteristics, audio channel count, bit depth.
2. **Video Demuxing:**
   - Wrap `craft-media` ISO BMFF and Matroska parsers to extract compressed sample packets with presentation timestamps (PTS) and composition offsets (DTS/CTO).
   - Frame seek indexer: builds an in-memory keyframe index for fast random-access scrubbing.
3. **Audio Demuxing & Decoding:**
   - Integrate `soundcraft-audio-io` and `symphonia` for universal decoding of PCM WAV, BWF, AIFF, FLAC, MP3, AAC, and Vorbis into standard planar f32 buffers.
4. **Fuzzing & Hostile Input Tests:**
   - Fuzz probers with truncated, randomized, and corrupted files (`proptest`) ensuring no panic or crash ever occurs.

#### Verification Gate:
- CLI command `encodercraft-cli probe <file>` outputs complete, accurate JSON metadata for all test files in under 5 milliseconds.
- Zero crashes on 10,000 fuzzed inputs.

---

### Milestone 2: Core Transcoding Pipelines & Native Codecs
**Objective:** Assemble the encoding engines, container multiplexers, and digital signal processing (DSP) filters.

#### Deliverables:
1. **Video Codec Integration (`crates/video-codecs`):**
   - Integrate pure-Rust encoders: `filmcraft-h264enc`, `filmcraft-prores`, `filmcraft-dnx`, and `filmcraft-apv`.
   - Integrate `effectcraft-hevcenc`, `effectcraft-av1enc`, and `effectcraft-vp9enc`.
   - Implement motion-JPEG and animated GIF encoders with adaptive palette quantization.
2. **Audio Processing & Loudness (`crates/audio-dsp`):**
   - High-quality polyphase sinc resampler for 44.1 kHz ↔ 48 kHz / 96 kHz conversion.
   - Channel remixing matrix (Mono, Stereo, 5.1 Surround, 7.1).
   - EBU R128 / ITU-R BS.1770 loudness normalization with 4x oversampled True-Peak limiter.
   - Integrate pure-Rust AAC encoder (`filmcraft-aac`), FLAC encoder, and Opus encoder (`effectcraft-opusenc`).
3. **Image & 3D Codecs (`crates/image-codecs`, `crates/spatial-io`):**
   - Multi-layer OpenEXR, 16-bit TIFF, PNG, JPEG, WebP, and AVIF encoders.
   - 3D mesh converters: USD/USDC, glTF 2.0 / GLB, Wavefront OBJ, STL, and PLY.
4. **Multiplexing Layer (`crates/muxers`):**
   - ISO BMFF (MP4/MOV) muxer with timescale normalization, edit lists, and HDR metadata.
   - Matroska (MKV/WebM) EBML chunking and cluster muxer.
   - MXF OP-1a and OP-Atom broadcast muxer.

#### Verification Gate:
- Oracle tests verify encoded MP4, MOV, MKV, and WebM files play cleanly in VLC, QuickTime, and Chrome.
- Output video frame counts, durations, and audio sync match source references within 1 audio sample tolerance.

---

### Milestone 3: The Queue Engine & Concurrency Scheduler
**Objective:** Build the multi-threaded execution queue, ring buffers, priority scheduling, and state persistence.

#### Deliverables:
1. **Worker Pool & Backpressure (`crates/pipeline`):**
   - Bounded crossbeam-channel ring buffer between demuxer and encoder (capped at 8 frames to limit memory footprint).
   - Rayon parallel thread pool for frame filtering, scaling (Lanczos3), and color space conversion.
2. **Atomic Progress & Cancellation:**
   - Atomic fixed-point progress counters (`AtomicU64`) and instantaneous FPS trackers.
   - Real-time cancellation flags (`AtomicBool`) allowing immediate abort without orphan processes.
3. **Queue State Journaling (`crates/engine`):**
   - Atomic state writes (`queue-state.json.tmp` → rename) preserving queue across app restarts or power outages.
   - Automatic recovery of interrupted jobs.
4. **Command Registry (`cmd/*`):**
   - Engine commands: `queue.add`, `queue.remove`, `queue.start`, `queue.pause`, `queue.stop`, `queue.duplicate`, `queue.reorder`.

#### Verification Gate:
- High-stress test: queue 50 parallel jobs, randomly pause, resume, cancel, and kill process; verify zero file corruption and accurate state recovery.

---

### Milestone 4: Preset Registry & Watch Folder Automation
**Objective:** Implement the universal preset system, dynamic "Match Source" evaluator, and automated directory monitoring.

#### Deliverables:
1. **Preset System (`crates/presets`):**
   - Embedded factory presets: Web (YouTube, Vimeo, TikTok), Broadcast (ProRes, DNxHR, MXF), Audio (BWF, FLAC, Podcast), 3D (glTF, USDZ).
   - Dynamic "Match Source" evaluator: interrogates source media and fills dimensions, frame rates, and bitrates automatically.
   - User preset serializer/deserializer: saves custom recipes to `$APP_DATA/encodercraft/presets/`.
2. **Watch Folder Engine (`crates/watch`):**
   - Continuous directory monitoring via `notify` crate with cross-platform FSEvents / inotify support.
   - Three-stage settle-delay protocol: size quiescence check + exclusive lock probe + quiet window.
   - Post-processing router: handles `MoveToProcessed`, `DeleteSource`, and `MoveToErrors` with diagnostic logs.

#### Verification Gate:
- Drop a 2 GB video file over a slow simulated network copy; verify the watcher waits until the write completes before beginning transcode.

---

### Milestone 5: Desktop User Interface & Preview Monitor
**Objective:** Build the complete professional dark-themed desktop application using `egui` and `wgpu`.

#### Deliverables:
1. **Queue Panel:**
   - Hierarchical data table showing source items, child output targets, status badges, progress bars, throughput metrics, and ETA.
2. **Preset Browser:**
   - Searchable, collapsible tree view of factory and user presets; drag-and-drop preset assignment onto queue items.
3. **Interactive Preview Monitor:**
   - Hardware-accelerated frame canvas with interactive scrub bar, In/Out point trimming handles, and frame stepping controls.
   - Side-by-side and split-slider Source vs. Output comparison mode.
4. **Export Settings Inspector:**
   - Modal/drawer dialog for fine-tuning Video, Audio, Effects, Captions, and Metadata parameters.
5. **Theme Tokens & i18n:**
   - Unified ArtCraft dark styling (`theme::Tokens`), HiDPI vector rendering, and multi-language TSV localization catalogs.

#### Verification Gate:
- Desktop application runs smoothly at 60+ FPS on macOS (Metal), Windows (DirectX 12), and Linux (Vulkan).
- Screenshot validation verifies UI layout matches ArtCraft visual standards.

---

### Milestone 6: Ecosystem Integration, CLI & MCP Server
**Objective:** Unify `encoderCraft` with all sibling applications, command-line environments, and AI agents.

#### Deliverables:
1. **Headless CLI (`apps/encodercraft-cli`):**
   - Command-line syntax: `encodercraft encode <input> -p "YouTube 4K" -o <output>`.
   - Batch conversion and daemon control subcommands.
2. **Model Context Protocol Server (`crates/mcp`):**
   - Exposes tools: `encodercraft_submit_transcode`, `encodercraft_batch_convert`, `encodercraft_get_queue_status`, `encodercraft_list_presets`.
3. **Shared Dispatch Library (`craft-dispatch`):**
   - Lightweight client crate published to `craft-libs/crates/dispatch`.
   - Two-line integration for any Rust application:
     ```rust
     let client = craft_dispatch::Client::connect_default()?;
     client.submit_job(&my_job_spec)?;
     ```
4. **Sibling Application Integrations (PRs):**
   - **FilmCraft:** Add "Queue in EncoderCraft" to the Export mode toolbar.
   - **EffectCraft:** Add "Add to EncoderCraft Queue" in the Render Queue panel.
   - **SoundCraft:** Add "Export via EncoderCraft" for multi-channel stems and podcast LUFS mastering.
   - **PhotoCraft / TextureCraft / DesignCraft:** Add batch export hand-off options.

#### Verification Gate:
- FilmCraft and EffectCraft successfully delegate export jobs to `encoderCraft` via IPC with zero UI lockup.
- AI coding agent successfully queries queue status and submits transcode jobs via MCP.

---

### Milestone 7: Sibling Codec & Upstream Reuse Integration
**Focus:** Connect local pure-Rust exporters from sibling ArtCraft repositories directly into the encoding pipeline, eliminating redundant code.

#### Deliverables:
1. **Local Sibling Path Integration:**
   - Updated root `Cargo.toml` workspace dependencies to link directly to local sibling paths (`../filmcraft`, `../effectcraft`, `../soundcraft`, `../craft-libs`).
   - Configured `[patch."https://github.com/storytold/craft-libs"]` redirecting transitive git dependencies to local working copies.
2. **Video Codecs Wiring (`crates/video-codecs`):**
   - Implemented wrappers implementing `VideoEncoder`:
     - `H264EncoderWrapper` -> `filmcraft-h264enc::Encoder`
     - `ProResEncoderWrapper` -> `filmcraft-prores::Encoder`
     - `HevcEncoderWrapper` -> `effectcraft-hevcenc::Encoder`
     - `Av1EncoderWrapper` -> `effectcraft-av1enc::Encoder`
     - `Vp9EncoderWrapper` -> `effectcraft-vp9enc::Vp9Encoder`
   - Added `crates/video-codecs/src/yuv.rs` providing safe, zero-panic conversion from RGB/RGBA frames to planar YUV 4:2:0 and 4:2:2.
   - Updated `create_video_encoder` dispatcher factory to route to all 7 native codecs.
3. **Audio DSP & Codec Wiring (`crates/audio-dsp`):**
   - Implemented `AacEncoderWrapper` -> `filmcraft-aac::Encoder` (CBR AAC-LC with AudioSpecificConfig generation).
   - Implemented `crates/audio-dsp/src/io.rs` -> `soundcraft-audio-io` (universal audio decode for MP3/FLAC/Ogg/AAC/CAF, and native FLAC & AIFF encoding).
4. **End-to-End Pipeline Expansion (`crates/pipeline`):**
   - Added FLAC and AIFF target execution paths.
   - Dynamic codec routing based on preset names and target containers in MP4/MOV multiplexing.

#### Verification Gate:
- `cargo xtask ci` passes 100% with 94 automated tests, zero clippy warnings, and zero layer violations.
- End-to-end integration tests verify real frame encoding across H.264, ProRes, HEVC, AV1, VP9, AAC, FLAC, and AIFF.

---

### Milestone 8: Multi-Clip Stitching, Captions & Preset Editor
**Focus:** Full AME parity for multi-clip stitching, subtitle sidecar import/export and burn-in, custom preset creation UI, and automated benchmarking.

#### Deliverables:
1. **Multi-Clip Concatenation Engine (`crates/pipeline/src/stitch.rs`):**
   - Implemented `ClipStitcher`: stitches multiple video and audio inputs into continuous timeline outputs.
   - Built `stitch_audio_buffers`: normalizes sample rates, remixes channels, and stitches continuous planar PCM.
   - Built `stitch_video_sequences`: handles resolution scaling with `FrameScaler`, continuous PTS/DTS timeline advances, and multiplexing into MP4/MOV.
   - Extended `SourceDescriptor::StitchedClips` in `crates/core/src/job.rs`.
2. **Sidecar Captions & Video Burn-In (`crates/pipeline/src/captions.rs`):**
   - Integrated `filmcraft-captions` upstream library for subtitle format detection (`detect`), parsing, and serialization.
   - Supports sidecar generation across SubRip (`.srt`), WebVTT (`.vtt`), Scenarist (`.scc`), MacCaption (`.mcc`), and TTML (`.ttml`).
   - Implemented `CaptionBurner`: rasterizes subtitle cues onto video frames with dark pill background and high-contrast typography at frame presentation timestamps.
   - Integrated into `OutputTarget` schema and `TranscodePipeline::execute_target`.
3. **Custom Preset Editor Dialog & Inspector Captions Tab (`crates/ui-egui`):**
   - Built `PresetEditorState` and `render_preset_editor` modal dialog for creating and validating user presets with resolution, bitrate, audio channels, and LUFS targets.
   - Added `Captions` tab to the Export Settings Inspector for toggling open captions burn-in, sidecar generation, and timecode offsets.
   - Added `+ New` button in the Preset Browser and `Preset > Create Encoding Preset...` top menu item.
4. **Automated Benchmarking Suite (`xtask/src/bench.rs`):**
   - Added `cargo xtask bench` command benchmarking throughput and speed factors across Frame Scaler, 3D LUT Color Engine, H.264, ProRes, Audio Sinc Resampling, EBU R128 Loudness, and Caption Burn-In.

#### Verification Gate:
- `cargo xtask ci` passes 100% with 117 automated tests, zero clippy warnings, and zero layer violations.
- Benchmarks verify high-performance real-time multiples across all transcoding operations.

---

### Milestone 9: Hardware Acceleration Bridge, Multichannel Stems & XMP Metadata
**Focus:** Platform hardware acceleration detection & quarantine under ADR 0001, multichannel audio stem extraction, and Adobe XMP/Dublin Core metadata injection.

#### Deliverables:
1. **Hardware Acceleration Bridge (`crates/platform` & ADR 0001):**
   - Implemented `encodercraft-platform` (Layer 3) strictly isolating hardware acceleration under ADR 0001.
   - Detects hardware capabilities across Apple VideoToolbox (Apple Silicon / Intel T2), NVIDIA NVENC, Linux VA-API, and Windows Media Foundation.
   - Transparent zero-panic fallback to pure-Rust software encoders.
2. **Multichannel Audio Stem Export (`crates/pipeline/src/pipeline.rs`):**
   - Implemented `export_audio_stems` supporting `DiscreteMono` (splitting multichannel mix into `{stem}_Stem_L.wav`, `{stem}_Stem_R.wav`, etc.) and `StereoPairs`.
   - Generates compliant EBU Tech 3285 BWF audio files with sample rate and channel layout preservation.
3. **Dublin Core & Adobe XMP Metadata Injection (`crates/core/src/metadata.rs` & `crates/muxers`):**
   - Implemented `XmpMetadata` structure with Dublin Core (`dc:title`, `dc:creator`, `dc:description`, `dc:rights`), XMP (`xmp:CreateDate`, `xmp:ProjectName`), and Dynamic Media (`xmpDM:startTimecode`).
   - Implemented XML/RDF packet serialization and embedded into MP4/MOV via standard Adobe XMP UUID box (`be7acfcb-97a9-42e8-9c71-999491e3afac`) and BWF WAV via `axml` chunk.
   - Added automated sidecar `.xmp` file generation alongside exported media.
4. **Export Settings UI & CLI Enhancements:**
   - Extended egui Export Settings Inspector (`crates/ui-egui/src/inspector.rs`): added hardware acceleration toggle to Video tab, multichannel audio stems selector to Audio tab, and Dublin Core / XMP metadata editor with sidecar toggle to Metadata tab.
   - Extended CLI (`apps/encodercraft-cli`): added `--stems`, `--hardware`, `--xmp-title`, `--xmp-creator`, `--sidecar-xmp`, and `--caption` flags.

#### Verification Gate:
- `cargo xtask ci` passes 100% with 120 automated tests, zero clippy warnings, and zero layer violations.
- Benchmarks verify real-time multipliers across all pipelines.

---

### Milestone 10: Batch Operations, Multi-Selection, Dedicated Watch Folders & Native Pickers
**Focus:** Full Adobe Media Encoder batch queue parity, keyboard selection navigation, unified watch folder management, and native system file pickers.

#### Deliverables:
1. **Multi-Job Batch Selection:**
   - Multi-job selection in Queue Panel (`selected_job_ids`, range select `Shift+Click`, toggle select `Cmd/Ctrl+Click`, select all `Cmd+A`).
   - Batch operations toolbar: Batch Duplicate (`Cmd+D`), Batch Remove (`Delete`/`Backspace`), Batch Status Reset, and Batch Output Target Appending.
   - Batch Preset Application: double-clicking or applying presets from Preset Browser updates all selected jobs simultaneously.
2. **Global Batch Queue Summary:**
   - Real-time batch progress header with total, queued/ready, encoding, completed, and failed job counters plus overall aggregate progress bar.
3. **Dedicated Watch Folders Panel (`crates/ui-egui/src/watch_panel.rs`):**
   - Dual-tab workspace switcher (`[ ≡ Queue ]  [ 📁 Watch Folders ]`).
   - Interactive watch folder configuration table, manual directory rescan trigger, and background settling monitor.
4. **Native File Dialogs (`rfd = "0.17"`):**
   - Multi-file batch add dialog (`Cmd+I`), multi-clip stitching picker, and watch folder directory pickers.

#### Verification Gate:
- `cargo xtask ci` passes 100% with 124 automated tests, zero clippy warnings, and zero layer violations.

---

### Milestone 11: Background Workers, Preferences, Activity Log & Context Menus
**Focus:** Non-blocking background worker execution, user preferences persistence, activity audit log, and right-click context menus.

#### Deliverables:
1. **Asynchronous Background Execution Worker:**
   - Implemented `QueueManager::start_background_worker` maintaining lock-free 20 Hz progress polling while keeping the UI responsive at 60 FPS.
2. **Interactive Preferences Modal (`crates/ui-egui/src/prefs_dialog.rs`):**
   - `Edit > Preferences...` (`Cmd+,`) modal configuring queue completion chime, auto-start on submission, parallel worker count, and default destination directories.
3. **Persistent Activity Audit Log (`crates/ui-egui/src/log_dialog.rs`):**
   - `File > Show Encoding Log...` (`Cmd+L`) modal displaying searchable transcode history, execution durations, status badges, and copy-to-clipboard export.
4. **Context Menus:**
   - Right-click context menus on queue jobs and target rows for "Reveal in Finder/Explorer", "Copy Output Path", "Inspect Settings", "Duplicate", and "Stitch Selected Clips".
5. **CLI & MCP Extensions:**
   - Added CLI `queue start`, `queue pause`, `queue clear`, and `queue log` commands.
   - Added MCP `encodercraft_get_log` and `encodercraft_set_output_dir` AI agent tools.

#### Verification Gate:
- `cargo xtask ci` passes 100% with 125 automated tests, zero clippy warnings, and zero layer violations.

---

### Milestone 12: ArtCraft Procedural Vector Icon System & Brand Alignment
**Focus:** Clean-room procedural vector icon painter (`Pen16`), removal of non-rendering Unicode strings/emojis, and repo rename to canonical `encoderCraft`.

#### Deliverables:
1. **Procedural Vector Icon System (`crates/ui-egui/src/icons.rs`):**
   - Implemented `Pen16` pixel-snapped vector painter on a 16×16 grid (`crisp`, `stroke_w`, `snap_pos`, `line`, `closed`, `fill`, `rect`, `rect_fill`, `circle`, `dot`, `arc`).
   - Defined `Icon` enum covering `Play`, `Pause`, `Stop`, `StepBack`, `StepFwd`, `Plus`, `Minus`, `Trash`, `Duplicate`, `ArrowUp`, `ArrowDown`, `Reset`, `ChevronRight`, `ChevronDown`, `ChevronLeft`, `Folder`, `Gear`, `Search`, `Preset`, `Link`, `Clock`, `Check`, `Close`, `Sliders`, `ListView`, and `Info`.
   - Reusable `button`, `button_with_label`, `small_button`, and `icon_label` widgets supporting hover, active, and pressed states with theme token integration.
2. **Theme Tokens Expanded (`crates/ui-egui/src/theme.rs`):**
   - Added `icon`, `icon_active`, `hover`, `pressed`, and `radius_sm` tokens across `Tokens::dark()` and `Tokens::high_contrast()`.
3. **Replaced All Unicode Placeholders:**
   - Cleanly replaced raw Unicode strings and emoji symbols across queue panel, preview transport, watch folders, preset browser, logs dialog, preferences, dual drop zones, and workspace navigation tabs.
4. **Repository Brand Alignment:**
   - Standardized repository naming to canonical ArtCraft convention: `encoderCraft` (`github.com/el-j/encoderCraft`).

#### Verification Gate:
- `cargo run -p xtask -- ci` passes 100% with formatting, zero clippy warnings, zero layer violations, and all automated tests.
- Desktop UI displays razor-sharp vector icons across all panels without reliance on platform emoji fonts.

---

### Milestone 13: Centralize Media Codecs in `craft-libs` & Avid DNxHR Integration
**Objective:** Unify all clean-room, pure-Rust media codecs into `craft-libs` as the One True Source for the ArtCraft ecosystem, and wire Avid DNxHR/DNxHD into `encoderCraft`.

#### Deliverables:
1. **Upstream Codecs in `craft-libs`:**
   - Added `craft-dnx`: Pure-Rust SMPTE ST 2019-1 Avid DNxHD/DNxHR progressive CBR encoder and full-profile decoder.
   - Added `craft-opus`: Pure-Rust RFC 6716 / RFC 8251 Opus audio packet parser and decoder.
   - Added `craft-vp8`: Pure-Rust RFC 6386 VP8 frame decoder.
   - Re-exported via `craft-codecs` and pushed branch `feat/centralize-media-codecs` to `git@github.com:el-j/craft-libs.git`.
2. **`encoderCraft` Codec Integration:**
   - Wired `craft-dnx` into `encodercraft-video-codecs` with `DnxEncoderWrapper`.
   - Connected `VideoCodecKind::DnxHd` in `create_video_encoder` and `TranscodePipeline`.
   - Added factory presets: `"Avid DNxHR HQ 1080p"` and `"Avid DNxHR HQX 4K (10-bit)"`.

---

### Milestone 14: Media Stream Classification & Ingestion Format Guardrails
**Objective:** Intelligently detect media stream payloads on ingestion, preventing mismatched codec applications (e.g. converting an MP3 to an MP4 video).

#### Deliverables:
1. **Media Stream Kind Classification (`crates/core`, `crates/demux`):**
   - Defined `MediaStreamKind`: `AudioOnly`, `VideoWithAudio`, `VideoOnly`, `StillImage`, `ImageSequence`, `Spatial3D`, `Unknown`.
   - Implemented `classify_stream_kind` analyzing elementary streams with file extension fallback.
2. **Smart Ingestion Routing (`crates/ui-egui/src/app.rs`):**
   - `add_file_job` auto-selects `.wav` and `"Broadcast Wave BWF (-23 LUFS)"` for audio-only files (`.mp3`, `.wav`, `.flac`, `.aiff`, `.ogg`, `.m4a`).
   - Auto-selects `.glb` and `"glTF 2.0 Binary (.glb)"` for 3D spatial models.
   - Auto-selects `.mp4` and `"Match Source - High Bitrate"` for video containers.
3. **Format Guardrails (`crates/ui-egui/src/preset_browser.rs`, `queue_panel.rs`):**
   - Disables video preset application on audio-only files with clear warning badges.
   - Dynamically targets matching container types when clicking `+ Add Output`.

---

### Milestone 15: Desktop Viewport Clamping & Bounded Action Popups
**Objective:** Ensure all queue row action menus, context dialogs, and dock sidebars remain strictly inside window bounds without overflow or clipping.

#### Deliverables:
1. **Anchored Popups with Upward Flipping (`crates/ui-egui/src/queue_panel.rs`):**
   - Replaced unconstrained `ui.menu_button("•••")` with an anchored `egui::Area` popup clamped to `ctx.input(|i| i.raw.screen_rect)`.
   - Automatically flips upward when items are located near the bottom of the window.
   - Handled outside clicks and `Escape` key dismissal.
2. **Horizontal Ellipsis Truncation:**
   - Truncated source names (>38 chars) and output target paths (>32 chars) with hover tooltips, preventing row expansion.
3. **Sidebar Dock Constraints:**
   - Set `.min_size(260.0).max_size(480.0)` on the right dock.

#### Verification Gate:
- `cargo run -p xtask -- ci` passes 100% with formatting, zero clippy warnings, zero layer violations, and all automated tests passing.

---

### Milestone 16: Universal Cross-Field Media Matrix, Audio/Vector/Document Ingestion & Transformation
**Objective:** Expand `encoderCraft` beyond audio/video/3D into a universal media transformation engine handling lossless audio (FLAC, OGG, MP3), 2D vector graphics (SVG, AI, EPS), and page layout documents (PDF, DOCX, IDML, INDD, EPUB).

#### Deliverables:
1. **Stream Classification & Sniffing Extensions (`crates/core`, `crates/demux`):**
   - Added `Vector2D` and `Document` variants to `MediaStreamKind`.
   - Added `OggAudio`, `Mp3`, `Svg`, `Docx`, `Idml` variants to `ContainerType`.
   - Added magic-byte sniffing for DOCX/PPTX/XLSX (ZIP PK), IDML (designmap.xml), EPUB, InDesign binary (`INDD`), Illustrator (`AI` with `/AIPrivateData`), PostScript/EPS (`%!PS-Adobe`), Photoshop (`8BPS`), and BMP (`BM`).
   - Extended `classify_stream_kind` with comprehensive MIME/file extension mappings.
2. **Audio Decoding & Cross-Transcoding Pipeline (`crates/audio-dsp`, `crates/pipeline`):**
   - Added `load_source_audio` dynamically reading file source bytes and decoding MP3, FLAC, Ogg Vorbis, AAC, CAF, WAV, and AIFF into planar float buffers.
   - Connected `execute_audio_flac`, `execute_audio_aiff`, and `execute_audio_wav` to transcode real incoming audio samples with channel remixing and loudness normalization.
3. **Vector & Document Transformation Pathways (`crates/pipeline`):**
   - Implemented `execute_vector_svg` transcoding vector sources to clean, well-formed SVG.
   - Implemented `execute_document_pdf` with valid, clean-room ISO 15930 PDF 1.7 generation and XMP metadata inclusion.
4. **Factory Presets & UI Guardrails (`crates/presets`, `crates/ui-egui`):**
   - Added factory presets: `"Lossless FLAC Master (24-bit 48kHz)"`, `"Apple Lossless AIFF (24-bit 48kHz)"`, `"Web Vector SVG (Optimized)"`, and `"Print-Ready PDF/X-4"`.
   - Added `PresetCategory::Vector` and `PresetCategory::Document` filter chips in Preset Browser.
   - Enhanced format guardrails preventing video preset applications on audio-only or document/vector files without explicit rasterization.

#### Verification Gate:
- `cargo run -p xtask -- ci` passes 100% with all 129 automated tests, zero clippy warnings, zero layer violations.
- Synthetic regression tests verify cross-format audio transcode (FLAC -> WAV), Vector transcode (SVG), and Document transcode (DOCX -> PDF).

---

### Milestone 17: Universal 3D Spatial & Raster Image Transcoding Pipelines
**Objective:** Complete universal transcode coverage for 3D polygon mesh models (glTF/GLB, Wavefront OBJ, binary STL, binary PLY, Pixar USDZ) and raster image formats (PNG, TIFF, OpenEXR, BMP) with dimensional scaling and format guardrails.

#### Deliverables:
1. **3D Mesh Serializers & Parsers (`crates/spatial-io`):**
   - Implemented `parse_glb` parsing Khronos binary glTF 2.0 containers into `TriangleMesh`.
   - Implemented `write_usda` and `write_usdz` packaging 3D meshes into Apple AR Quick Look `.usdz` archives.
   - Added container types: `ContainerType::Obj`, `ContainerType::Stl`, `ContainerType::Ply`.
2. **Raster Image Codecs (`crates/image-codecs`):**
   - Implemented `encode_bmp` and `decode_bmp` for uncompressed Windows Bitmap (BMP) images.
   - Added container types: `ContainerType::Png`, `ContainerType::Tiff`, `ContainerType::OpenExr`, `ContainerType::Bmp`.
3. **Execution Pipelines (`crates/pipeline`):**
   - Implemented `execute_spatial_3d` routing mesh models across GLB, OBJ, STL, PLY, and USDZ.
   - Implemented `execute_image_transcode` decoding source raster images, applying `FrameScaler` resampling, and encoding to PNG, TIFF, OpenEXR, or BMP.
4. **Factory Presets & UI Controls (`crates/presets`, `crates/ui-egui`):**
   - Added 3D presets: `"Wavefront OBJ (.obj)"`, `"3D Print Binary STL (.stl)"`, `"Stanford Polygon PLY (.ply)"`, `"Apple AR Quick Look USDZ (.usdz)"`.
   - Added image presets: `"Still Image PNG (Lossless)"`, `"Broadcast TIFF (16-bit RGB)"`, `"VFX OpenEXR (Half Float)"`, `"Windows Bitmap BMP (Uncompressed)"`.
   - Added `PresetCategory::StillImage` category filter chip in the Preset Browser.

#### Verification Gate:
- `cargo run -p xtask -- ci` passes 100% with all 135 automated tests, zero warnings, and full layer compliance.
- Integration tests verify OBJ -> USDZ, OBJ -> GLB, BMP -> PNG, and BMP -> TIFF conversions.

---

### Milestone 18: Upstream Codec Alignment & Sibling Ecosystem Sync (2026-11-02)
**Objective:** Synchronize and align `encoderCraft` with upstream `storytold/craft-libs#504` and sibling creative apps (`filmcraft`, `effectcraft`, `soundcraft`), addressing maintainer review feedback and establishing clean domain boundaries.

#### Deliverables:
1. **Upstream Review Resolution (`storytold/craft-libs#504`):**
   - Decoupled video codecs from audio to prevent duplicate ownership with the active `#439–#448` (`craft-audio`) PR series.
   - Refined `craft_testkit::with_deadline` to enforce worker thread execution timeouts under `CRAFT_TEST_TIMEOUT_SECS` and `FILMCRAFT_HOSTILE_TIMEOUT_SECS`.
   - Added dedicated GitHub Actions CI workflows for each video crate (`h264enc`, `prores`, `hevcenc`, `av1enc`, `vp9enc`, `vp8`, `dnx`, `testkit`, `codecs`).
2. **Sibling Migration Documentation Sync:**
   - Updated `storytold/filmcraft#3941` (`docs/codecs-migration-to-craft-libs.md`) reflecting #504 video and #443-#444 AAC audio locations.
   - Updated `storytold/soundcraft#1275` (`docs/codecs-migration-to-craft-libs.md`) mapping audio I/O and codecs to the #439–#448 `craft-audio` stack.
   - Updated `storytold/effectcraft#2814` (`docs/codecs-migration-to-craft-libs.md`) clarifying `craft-codecs` video umbrella scope.
3. **`encoderCraft` Workspace Decoupling:**
   - Updated root `Cargo.toml` and patches to source video codecs from `craft-libs#504` and audio from sibling repositories (`filmcraft-aac`, `soundcraft-audio-io`).
   - Verified seamless, zero-breaking changes across all pipeline transcode operations.

#### Verification Gate:
- `cargo run -p xtask -- ci` passes 100% with all 135 automated tests, zero clippy warnings, zero layer violations.
- All 5 repositories (`encoderCraft`, `craft-libs`, `filmcraft`, `soundcraft`, `effectcraft`) clean and synced to remote forks/origins.

---

### Milestone 19: Broadcast MXF OP1a / OP-Atom Multiplexing & Simultaneous A/V Multiplexing (2026-11-02)
**Objective:** Achieve full broadcast television and post-production NLE parity by implementing SMPTE ST 377-1 MXF OP1a / OP-Atom multiplexing and simultaneous Audio-Video track multiplexing across MP4/MOV, MXF, and MKV/WebM.

#### Deliverables:
1. **Broadcast MXF Multiplexer (`crates/muxers/src/mxf.rs`):**
   - Implemented `MxfMuxer` backed by `filmcraft-mxf` supporting SMPTE ST 377-1 OP1a (frame-wrapped VC-3 DNxHD/HR, Apple ProRes, H.264/AVC + Broadcast Wave PCM) and OP-Atom (clip-wrapped).
   - Upgraded `encodercraft-demux` (`crates/demux/src/probe.rs`) to inspect MXF picture/sound tracks, package names, and SMPTE start timecodes.
2. **Simultaneous Audio-Video Multiplexing (`crates/pipeline/src/pipeline.rs` & `crates/muxers/src/mp4.rs`):**
   - Upgraded `Mp4Muxer::add_video_stream` to construct typed `CodecConfig` (`Avc`, `Hevc`, `Av1`, `Vp9`, `Jpeg`) so visual sample entries (`stsd`) include full `VideoParams`.
   - Wired AAC-LC audio encoding (`AacEncoderWrapper`) into `execute_video_mp4` and PCM audio into `execute_video_mxf` and `execute_video_mkv`.
3. **Factory Presets (`crates/presets/src/factory.rs`):**
   - Added `"Broadcast MXF OP1a DNxHR HQ"` and `"Avid Media Composer MXF OP-Atom"`.

---

### Milestone 20: Real Video Demuxing & Decoding Loop with In/Out Trim (2026-11-02)
**Objective:** Complete end-to-end real video frame demuxing and decoding for MP4/MOV, MKV/WebM, and MXF containers with frame-accurate In/Out point trimming and exhaustive container routing.

#### Deliverables:
1. **Pure-Rust Video Packet Decoders (`crates/video-codecs/src/decoder.rs`):**
   - Implemented `decode_prores_packet` (`craft-prores`), `decode_dnx_packet` (`craft-dnx`), `Vp8DecoderWrapper` (`craft-vp8`), and `decode_video_packet_auto`, converting planar YUV 4:2:0 / 4:2:2 / 4:4:4 (8/10/12-bit) to `Rgb8` `VideoFrame`s.
2. **Container Video Demuxing & Trimming (`crates/pipeline/src/pipeline.rs`):**
   - Implemented `load_source_video_frames` and `decode_container_video_frames` demuxing real video samples from ISO BMFF (MP4/MOV), Matroska (MKV/WebM), MXF, and BMP sources with `start_tick` and `duration_ticks` In/Out point trimming.
   - Exhaustively covered all 27 `ContainerType` variants in `TranscodePipeline::execute_target` without a wildcard fallback.

---

### Milestone 21: Desktop UI Format Controls, Preview Monitor & Trim Persistence (2026-11-02)
**Objective:** Expose every container and codec in the Desktop UI and connect the Preview Monitor's In/Out trim controls directly to the queue engine.

#### Deliverables:
1. **Full Container & Codec Selection (`crates/ui-egui/src/inspector.rs`, `preset_editor.rs`):**
   - Exposed all 27 `ContainerType` variants in both the Export Settings Inspector and Custom Preset Editor, along with `"DNxHR"`, `"MJPEG"`, and `"Opus"` codec options.
2. **Interactive Preview Monitor & Trim Persistence (`crates/ui-egui/src/preview_panel.rs`, `app.rs`, `crates/engine/src/queue.rs`):**
   - Added `QueueManager::set_job_trim_range` persisting `start_tick`, `duration_ticks`, and `timebase` onto queued jobs.
   - Enhanced `render_preview_panel` with SMPTE reference color bars, animated playhead scanline, In/Out trim range indicator bar, and automatic synchronization with the selected queue job.

---

### Milestone 22: Universal 14-App ArtCraft Sibling Origin Badges (2026-11-02)
**Objective:** Complete visual queue badge recognition for all 14 ArtCraft sibling applications (`el-j/encoderCraft#1`).

#### Deliverables:
1. **14-App Client Origin Badges (`crates/ui-egui/src/queue_panel.rs`):**
   - Distinct semantic badge coloring for `FilmCraft`, `EffectCraft`, `SoundCraft`, `PhotoCraft`, `VectorCraft`, `DesignCraft`, `PdfCraft`, `FigCraft`, `CadCraft`, `DeckCraft`, `SolidCraft`, `WordCraft`, `TextureCraft`, `GridCraft`, and `LightCraft`.

---

### Milestone 30: Universal Format Expansion & ArtCraft Bidirectional Conversion
**Objective:** Expand `encoderCraft` into the centralized universal media conversion and transcoding nexus across all ArtCraft creative applications and standard/proprietary commercial formats without greenfield codec reinvention. Tracked under Epic [#69](https://github.com/el-j/encoderCraft/issues/69) in [Milestone 8](https://github.com/el-j/encoderCraft/milestone/8).

#### Deliverables & Sub-Tasks:
1. [x] **Core & Demux Sniffer ([#70](https://github.com/el-j/encoderCraft/issues/70)) (Completed 2026-11-04):**
   - Universal `ContainerType` and `MediaStreamKind` variants for all Adobe formats (`INDD`, `PSD`, `PSB`, `AEP`, `AI`, `PRPROJ`, `SESX`), CAD formats (`DXF`, `DWG`, `HPGL`), and native ArtCraft formats (`.designcraft`, `.photocraft`, `.vectorcraft`, `.effectcraft`, `.filmcraft`, `.soundcraft`, `.cadcraft`, `.anicraft`).
   - Magic-byte container sniffing and `ContainerType::valid_conversion_targets` smart matrix.
2. [x] **CAD & Technical Drafting Pipeline ([#71](https://github.com/el-j/encoderCraft/issues/71)) (Completed 2026-11-04):**
   - Ingest & convert AutoCAD DXF/DWG and HPGL plotter files via `cadcraft-dxf`, `cadcraft-dwg`, `cadcraft-io` into `CadDrawing` IR, `.cadcraft`, SVG, and blueprint PDF.
3. [x] **Prepress & Publication Pipeline ([#72](https://github.com/el-j/encoderCraft/issues/72)) (Completed 2026-11-04):**
   - Ingest InDesign `.indd` via `storytold/indd-utils` into `DocumentCanvas` IR; bidirectional `.designcraft` <-> `IDML` / `PDF/X-4` / `EPUB 3` via `designcraft-idml` and `designcraft-pdf`.
4. [x] **Layered Graphics & Photography Pipeline ([#73](https://github.com/el-j/encoderCraft/issues/73)) (Completed 2026-11-04):**
   - Ingest & write Photoshop `.psd` / `.psb` with full layer trees via `photocraft-psd`; bidirectional conversion with `.photocraft`, multi-channel OpenEXR, BigTIFF, and DDS GPU texture maps.
5. [x] **Vector Graphics & Illustration Pipeline ([#74](https://github.com/el-j/encoderCraft/issues/74)) (Completed 2026-11-04):**
   - Ingest Illustrator `.ai` (PDF stream + PGF/AI5 layers) via clean-room parser; bidirectional conversion with `.vectorcraft` native binary container (`\x89CRAFT\x00\x01V`), W3C SVG 2.0, PostScript EPS, print PDF, text-to-outlines conversion, and AutoCAD DXF interop.
6. [x] **Motion Graphics & NLE Timeline Pipeline ([#75](https://github.com/el-j/encoderCraft/issues/75)) (Completed 2026-11-04):**
   - Ingest After Effects `.aep` RIFX chunk trees via `effectcraft-aep` clean-room parser (render to ProRes/H.264 or convert to `.effectcraft` binary container `\x89CRAFT\x00\x01E` and W3C Lottie JSON).
   - Ingest Premiere Pro `.prproj` gzipped XML via `filmcraft-prproj` clean-room parser (batch render sequences or convert to `.filmcraft` binary container `\x89CRAFT\x00\x01F`, Final Cut Pro XML `fcpxml` v1.9, and OpenTimelineIO `.otio`).
7. **Audio DAW & Multitrack Pipeline ([#76](https://github.com/el-j/encoderCraft/issues/76)):**
   - Ingest Audition `.sesx` sessions via `soundcraft-audio-io` and `soundcraft-dsp`; bidirectional conversion with `.soundcraft` and discrete BWF stems with EBU R128 loudness.
8. **UI Inspector & Headless CLI Integration ([#77](https://github.com/el-j/encoderCraft/issues/77)):**
   - Dynamic tab visibility and smart destination filtering in Export Settings Inspector.
   - Sequence/composition selection picker for multi-timeline project files.
   - CLI `encodercraft convert <input> --to <format> -o <output>` subcommand.

---

## 3. Risk Mitigation & Technical Contingencies

| Technical Risk | Likelihood | Impact | Architectural Mitigation |
|---|---|---|---|
| **Memory Exhaustion on 8K Video** | Medium | High | Bounded 8-frame ring buffers with backpressure; pooled frame buffer recycling. |
| **Corrupt / Hostile Media Files** | High | Critical | `#![deny(clippy::unwrap_used)]` workspace-wide; `proptest` bitstream fuzzing; `catch_unwind` panic boundaries. |
| **Premature Watch Folder Triggers** | High | Medium | 3-stage settle-delay protocol probing file byte stability and exclusive file locks. |
| **Unsupported Exotic Video Formats** | Low | Low | Transparent subprocess bridge to system `ffmpeg` (Option A from `plan-draft-00.md`) when native codec is unavailable. |
| **UI Stutter During Heavy Encodes** | Low | Medium | Strict decoupling of UI thread from transcode workers; lockless atomic progress polling (`AtomicU64`). |
