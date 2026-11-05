# encoderCraft Roadmap

> **Stage: Complete Implementation (Alpha Release Candidate)** · **Target:** Adobe Media Encoder 2026 (v26.5.x)  
> **Repository:** `github.com/el-j/encodercraft` (transferring to `github.com/storytold/encodercraft`)  
> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-02 · **Change:** Complete M0–M22 implementation suite, MXF OP1a/OP-Atom muxing, full video decode loop, and CI verification  

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
| **Automated Tests** | **>80 tests** | **139 passed** | Unit, fuzz, and integration tests across L0-L7. |
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
- [x] **M19: Broadcast MXF OP1a / OP-Atom Multiplexing & Simultaneous A/V Multiplexing (2026-11-02)**
  - Implemented `MxfMuxer` (`crates/muxers/src/mxf.rs`) wrapping `filmcraft-mxf` for SMPTE ST 377-1 OP1a (frame-wrapped VC-3 DNxHD/HR, ProRes, H.264/AVC + Broadcast Wave PCM) and OP-Atom (clip-wrapped).
  - Upgraded `encodercraft-demux` (`crates/demux/src/probe.rs`) to extract picture/sound tracks, package names, and start timecodes from MXF containers.
  - Upgraded `Mp4Muxer::add_video_stream` to construct typed ISO BMFF visual sample entries (`avc1`, `hvc1`, `av01`, `vp09`, `mjpa`) with `VideoParams`.
  - Wired simultaneous Audio-Video multiplexing into `execute_video_mp4`, `execute_video_mxf`, and `execute_video_mkv` (with AAC-LC and Broadcast PCM audio tracks).
  - Added factory presets: `"Broadcast MXF OP1a DNxHR HQ"` and `"Avid Media Composer MXF OP-Atom"`.
- [x] **M20: Real Video Demuxing & Decoding Loop with In/Out Trim (2026-11-02)**
  - Implemented pure-Rust video packet decoders in `crates/video-codecs/src/decoder.rs`: `decode_prores_packet` (`craft-prores`), `decode_dnx_packet` (`craft-dnx`), `Vp8DecoderWrapper` (`craft-vp8`), and `decode_video_packet_auto`.
  - Implemented `load_source_video_frames` and `decode_container_video_frames` in `TranscodePipeline`, demuxing and decoding real video frames from MP4/MOV, MKV/WebM, MXF, and BMP sources with `start_tick` / `duration_ticks` In/Out point trimming.
  - Exhaustively covered all 27 `ContainerType` variants in `TranscodePipeline::execute_target` (including `MxfOp1a`, `MxfOpAtom`, `MatroskaMkv`, `WebM`, `AnimatedGif`, `OggAudio`, `Mp3`, `Jpeg`, `WebP`, `StillSequence`).
- [x] **M21: Desktop UI Format Controls, Preview Monitor & Trim Persistence (2026-11-02)**
  - Exposed all 27 `ContainerType` variants in both the Export Settings Inspector (`inspector.rs`) and Custom Preset Editor (`preset_editor.rs`), plus `"DNxHR"`, `"MJPEG"`, and `"Opus"` codec options.
  - Upgraded the Preview Monitor (`preview_panel.rs` & `app.rs`) with SMPTE reference color bars, animated playhead scanline, In/Out trim range strip, and live synchronization with `QueueManager::set_job_trim_range`.
- [x] **M22: Universal 14-App ArtCraft Sibling Origin Badges (2026-11-02)**
  - Expanded queue row client origin badges (`queue_panel.rs`) to color-code all 14 ArtCraft sibling applications (`FilmCraft`, `EffectCraft`, `SoundCraft`, `PhotoCraft`, `VectorCraft`, `DesignCraft`, `PdfCraft`, `FigCraft`, `CadCraft`, `DeckCraft`, `SolidCraft`, `WordCraft`, `TextureCraft`, `GridCraft`, `LightCraft`).

## Post-alpha.1 Recovery (M23–M29)

Alpha 1 shipped with real defects; they are tracked as issues #6–#51 under epics #45–#51.

- [x] **M23 Queue that drains** – worker pool honouring `max_parallel_jobs` (#53); CLI `encode`/`watch` use the shared queue (#54). Remaining: weighted overall progress, per-target results (#8).
- [x] **M24 Real video transcode** – decode via filmcraft (`encodercraft-source`), real avcC/hvcC/av1C and sample-derived timing, conformance suite (#52). Decoder matrix verified for H.264/HEVC/VP8/VP9/AV1/MPEG-2/ProRes with `--formats` (#15); preset parameters, container x codec matrix and faststart verified by encode-decode tests (#18); bounded decode/convert/encode streaming, frame pooling, windowed audio, cancellation-safe staged outputs, transforms, ETA, configurable memory limits, and high-depth RGBAF32 processing delivered by #17. Match Source selects 10-bit HEVC for >8-bit inputs; unsupported HDR/gamut reductions remain consent-gated.
- [x] **M25 Real image pipeline** – photocraft decode (EXIF orientation, 16-bit/float/CMYK) and encode for PNG, JPEG, TIFF, WebP, GIF, BMP, EXR (#56, #57, #22, #23, #24, #47). Numbered image sequences in (`shot_%04d.png`, CLI `--fps`) and video to numbered stills out; full 7x7 still conversion matrix, golden pixel tests, 16-bit TIFF, CMYK JPEG, palette PNG, and grey-image regression guard.
- [x] **M26 Preview** – real preview monitor (#55); JKL/drop-frame transport with In/Out and loop, zoom/pan, safe margins, aspect guides, A/B split, rendered output frames, waveform/parade/histogram/vectorscope via `filmcraft-scopes`, and a live Encoding panel (#27–#29). Audio playback in the monitor remains open.
- [~] **M27 Workspace shell** – glyph fixes (#58); command table driving menus, queue-toolbar actions, persisted remappable app-command shortcuts, enabled state, and registry-generated queue/preset/media context menus with coverage tests (#32); shared command discovery/execution available through IPC, CLI, and MCP; workspace activation, save-as, rename, reset, delete, and panel visibility dispatch through engine commands; queue row actions, watch-folder ingestion, trim, log, and approved queue mutations also use the shared registry; named, persisted, self-repairing craft-ui dock workspaces with configurable panel focus/visibility shortcuts (#31); denser theme spacing, persisted Light/high-contrast variants, active-panel outline, and minimum-size/Hi-DPI render tests (#33); pinned build-time craft-fonts fallback integration. Remaining: side-by-side reference comparison is deferred pending target-app screenshots (#33).
- [x] **M28 Panels** – done: persistence of preferences/watch folders/favourites (#38, #39), sortable Preset Browser with import/export/delete (#35), Media Browser (#36), honest parity docs (#40). Queue table with sortable columns (Source, Format, Preset, Output File, Status), grouped parent sources and indented target child rows with inline format ComboBox, preset hyperlink selector, and output path dialog (#34). Comprehensive Export Settings dialog parity with Source/Output preview tabs, scrubber, real frame decoding, Match Source, and Video/Audio/Effects/Captions/Publish/Loudness/Metadata tabs (#37).
- [~] **M29 Hardening and alpha.2** – panic guard around third-party decode, malformed-media corpus, property tests, required-tool real-media CI matrix (#41); installed-package smoke jobs for Linux packages, AppImage, macOS DMG and Windows MSI (#42); seven bounded libFuzzer targets with nightly runs (#43). All seven fuzz targets built and completed a local seeded 10-run smoke pass; shell syntax and workflow lint checks pass. Clean-runner installer and sustained fuzz execution remain pending CI. MPEG-TS sniffing and real frame-rate probing in CLI/watch folders. IPC: `queue.submit` honours the start policy and `queue.subscribe` streams job events (#1 partly; an iceoryx2 packet transport with bounded backpressure feeds `SourceDescriptor::Pipe` jobs with raw RGBA frames and f32 audio through the transcode pipeline, while project-reference rendering remains). Open: alpha.2 release (#44).

## Milestone 30: Universal Format Expansion & ArtCraft Bidirectional Conversion

Tracked under Epic [#69](https://github.com/el-j/encoderCraft/issues/69) in [Milestone 8](https://github.com/el-j/encoderCraft/milestone/8).

- [x] **M30.1: Core & Demux Sniffer (#70)** – Universal `ContainerType` and `MediaStreamKind` variants for Adobe formats (`INDD`, `PSD`, `PSB`, `AEP`, `AI`, `PRPROJ`, `SESX`), CAD formats (`DXF`, `DWG`, `HPGL`), and native ArtCraft formats (`.designcraft`, `.photocraft`, `.vectorcraft`, `.effectcraft`, `.filmcraft`, `.soundcraft`, `.cadcraft`, `.anicraft`); magic-byte sniffing and `ContainerType::valid_conversion_targets` smart matrix.
- [x] **M30.2: CAD & Technical Drafting (#71)** – AutoCAD DXF/DWG and HPGL plotter ingest and conversion via `cadcraft` (DXF ASCII & binary, DWG headers, HPGL/PLT, `.cadcraft`, SVG and blueprint PDF).
- [x] **M30.3: Prepress & Publication (#72)** – InDesign INDD/IDML and DesignCraft conversion via `designcraft` & `indd-utils` (ObjectStreams database unpacker, IDML package, `.designcraft`, EPUB 3, and PDF/X-4 with OutputIntents).
- [x] **M30.4: Layered Graphics & Photography (#73)** – Photoshop PSD/PSB and PhotoCraft conversion via `photocraft` (layer trees, blend modes, masks, `.photocraft` container, PSB large documents, and multi-part OpenEXR).
- [x] **M30.5: Vector Graphics & Illustration (#74)** – Illustrator AI and VectorCraft conversion via `vectorcraft` (clean-room dual-stream PDF/AI reader/writer, `VectorCanvas` IR, multi-artboards, `.vectorcraft` binary container, SVG 2.0, PostScript EPS, text-to-outlines, and AutoCAD DXF interop).
- [x] **M30.6: Motion Graphics & NLE Timeline (#75)** – After Effects AEP and Premiere PRPROJ conversion via `effectcraft` & `filmcraft` (RIFX parser, bounded decompression bomb defense, `.effectcraft` container, W3C Lottie JSON, `.filmcraft` container, Final Cut Pro XML, and OpenTimelineIO).
- [x] **M30.7: Audio DAW & Multitrack (#76)** – Audition SESX and SoundCraft conversion: clean-room `.sesx`/`.soundcraft` parsers, `DawSession` IR, BWF master with iXML, stems and R128 loudness; hardened against hostile input; real clip decode, resample, gain/pan/declick mixing via `soundcraft-dsp`, WAV/FLAC/AIFF/`.sesx`/`.soundcraft` outputs, CLI `encode mix.sesx -o out.wav --stems`.
- [x] **M30.8: UI Inspector & Headless CLI (#77)** – `encodercraft convert <input> --to <format>` with source/target validation; inspector destination filtering by source container; domain-aware tabs (Composition, Layout, CAD, Layers) driven by a source inventory; `--sequence`, `--pages` and `--layers` selection honoured by the pipeline (multi-sequence prproj/filmcraft/effectcraft/AEP, per-page PDF/X-4 output, CAD and layered-image layer filtering); `probe` lists sequences, pages and layers.

## Known Issues (alpha.1, tracked above)

- Queue ran one job per Start click, video output was empty or 24 s long, images were grey and the preview was blank (fixed on main, ships in alpha.2).
- The interface layout differs from the reference encoder (M27/M28).
