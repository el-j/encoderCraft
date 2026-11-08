# Changelog

All notable changes to `encoderCraft` are documented here. The project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0-alpha.2] - 2026-11-08

Alpha 2 is a comprehensive stabilization and universal format expansion release delivering full parity with Adobe Media Encoder workflows, resolving all alpha.1 defects, harvesting pure-Rust codecs across the ArtCraft ecosystem, and expanding into 3D, scientific imaging, color LUTs, subtitles, broadcast MPEG-TS, and Apple Spatial Video.

### Highlights & Fixed Issues from Alpha 1

- **Multi-Item Queue Execution (#6, #53, #54):** Fixed the single-item execution bug. Multi-item batches now drain completely from a single Start click with worker pool scheduling honoring `max_parallel_jobs`.
- **Full Video Transcoding Pipeline (#10, #15, #17, #18, #52):** Fixed empty video outputs. Integrated robust decoding via `encodercraft-source` for H.264, HEVC, VP8, VP9, AV1, MPEG-2, Apple ProRes, and Avid DNxHD/HR with frame-accurate timing, faststart MP4 muxing, and bounded memory streaming.
- **Studio Still Image Pipeline (#20, #22, #23, #24, #47, #56, #57):** Fixed grey-image output. Added full still image conversion matrix (PNG, JPEG, TIFF, WebP, GIF, BMP, EXR), numbered image sequences (`shot_%04d.png`), 16-bit TIFF, CMYK JPEG, and EXIF orientation handling.
- **Live Preview Monitor & Video Scopes (#25, #27, #28, #29, #55):** Replaced placeholder colour bars with a live video preview canvas, JKL transport controls, frame-accurate In/Out trimming handles, A/B split comparison slider, waveform, vectorscope, RGB parade, histogram scopes, and real-time encoding progress.
- **Dockable Workspace Shell & Theme Neutrality (#30, #31, #32, #33, #115, #167):** Eliminated garbled text glyphs. Added dockable workspace tabs, CommandSpec registry with remappable shortcuts, native macOS menu bar with `--in-window-menus` override, saved window geometry, and authentic Adobe reference neutral greys for Medium Dark theme.

### Universal Media Domain Expansion (M30)

- **CAD & Technical Drafting (#70, #71):** Direct ingest and conversion for AutoCAD DXF, DWG, HPGL plotters, and `.cadcraft` to SVG and vector blueprint PDF.
- **Prepress & Publication (#72):** Ingest and export for Adobe InDesign INDD, IDML, and `.designcraft` to EPUB 3 and ISO 15930 PDF/X-4 with OutputIntents.
- **Layered Graphics & Photography (#73):** Photoshop PSD, PSB large documents, and `.photocraft` container conversion with multi-layer preservation and multi-part OpenEXR.
- **Vector Graphics & Illustration (#74):** Clean-room dual-stream Illustrator AI, multi-artboard `.vectorcraft`, SVG 2.0, and PostScript EPS interop.
- **Motion Graphics & NLE Timelines (#75):** After Effects AEP and Premiere PRPROJ sequence ingestion, RIFX parsing, Lottie JSON, Final Cut Pro XML, and OpenTimelineIO.
- **Audio DAW & Multitrack Sessions (#76):** Audition SESX and SoundCraft conversion with DAW session stems, clip gain/pan mixing, and EBU R128 loudness normalization.

### Professional Broadcast, Transforms & Effects Pipeline (M31–M37)

- **Export Settings Effects (#139, #140, #141, #142, #143):** Burn-in timecode overlays (BITC), graphic watermarks, dailies name overlays, broadcast video limiting (IRE 100/103), SDR conform HDR tone-mapping (HLG/PQ to Rec.709), and interactive source frame cropping.
- **Advanced Video & Audio Transforms (#171, #172, #173, #174):** Horizontal/vertical/both frame flipping, field deinterlacing (`TopFieldFirst`, `BottomFieldFirst`, `Bob`), pixel aspect ratio conform (`Source`, `Square`, `Custom`), millisecond-exact lip-sync audio delay offset compensation (-5000 ms to +5000 ms), and industry-grade E2E test verification.
- **Broadcast Delivery Profiles (#149):** Factory compliance presets for Sony XDCAM HD422, Panasonic AVC-Intra 100, DPP AS-11 UK HD Delivery (25p / EBU R128), and AMWA AS-10 in MXF OP-1a with 8-channel 24-bit 48 kHz PCM audio.
- **Multichannel Audio Matrix (#148):** Visual matrix routing supporting discrete mono stems, stereo pairs, 5.1 surround, and 7.1 surround track layouts.
- **Queue Automation & Watch Folders (#144, #145, #146, #147):** Idle timeout auto-start, unique output filename collision resolution (`_1`, `_2`), multi-target watch folder output routing with automatic source archiving, audio completion chime, and clean exit queue pruning.
- **UI Table Precision & Source Inspection (#160, #161, #162, #164, #165, #166):** Strict column boundary locking with bounded cell clipping (`render_column_cell`), ISO date timestamps (`YYYY-MM-DD HH:MM`), adaptive preset columns, stream essence inspection modal (`Cmd+I`), and sequence assembly stitch dialog.

### Production Workflow, UI/UX Polish & AME Parity Fixes (M38 & M41)

- **Batch Multi-Selection & Instant Dropdown Application (#177, #192, #197):** Simultaneous format and preset switching across all highlighted queue items with matched default presets and output file extensions; immediate job state updates and UI repaints on inline dropdown selection.
- **Domain-Filtered Formats & Essence Badges (#179, #193, #201):** Context-aware format filtering restricting dropdowns strictly to valid containers for the source media domain (e.g. audio files only show audio formats); distinctive color-coded visual chips (`[VID]`, `[AUD]`, `[IMG]`, `[DOC]`, `[3D]`, `[SEQ]`) in the Queue table.
- **Zero egui ID Clashes & Bounded Preset Browser (#176, #194, #202):** Eliminated widget ID collisions across Media Browser, Preset Browser, and Queue panels using explicit `ui.push_id` and `id_salt`; fixed Preset Browser table header outside bounded scroll area to prevent overflow.
- **Multi-File Selection & Recursive Folder Ingestion (#178, #195, #196):** Native Shift+Click range and Cmd/Ctrl+Click toggle selection in both system dialogs and Media Browser; recursive folder ingestion (`AddFolder`) with automatic stream classification.
- **Synchronized In/Out Range Trimming (#180, #198):** Connected Set In, Set Out, and Reset Range controls across the Preview Monitor, Inspector modal, and transcode execution pipeline for both video and audio.
- **Granular Preferences & Official Audio Metadata Auto-Tagger (#181, #199, #200):** Tabbed Preferences dialog with GPU/CPU renderer selection, parallel worker concurrency (`1..=16`), memory/VRAM budgets, and watch folder settle timing; full ID3/Vorbis/RIFF audio tag editor with automated metadata lookup from MusicBrainz and iTunes Search APIs.

### Ecosystem Pure-Rust Codec Harvest (M39)

- **Pure-Rust MP3 & Ogg Vorbis Native Encoders (#183):** Clean-room MPEG-1 Layer 3 encoder with ID3v2.4 metadata headers and Ogg Vorbis encoder with Vorbis comment tags (`crates/audio-dsp/src/mp3.rs`, `vorbis.rs`).
- **Apple Core Audio Format (.caf) & Standalone Ogg Opus (.opus) (#184):** 64-bit Apple `.caf` container encoder/decoder supporting >4 GB files and surround channels, plus RFC 7845 `.opus` container multiplexing with `OpusHead`/`OpusTags` (`crates/audio-dsp/src/caf.rs`, `opus.rs`).
- **GPU DirectDraw Surface (DDS), SMPTE DPX & JPEG XL (#185):** DirectX GPU block-compressed textures (`.dds` BC1/BC2/BC3), 10-bit SMPTE 268M `.dpx` cinema film scans with timecode headers, and next-gen JPEG XL (`.jxl`) (`crates/image-codecs/src/dds.rs`, `dpx.rs`, `jxl.rs`).
- **Deep OpenEXR, Radiance HDR, Medical DICOM & Astronomical FITS (#186):** Multi-sample Deep OpenEXR (`DEEPSCANLINE`), Radiance RGBE (`.hdr`) environment maps, NEMA PS3.10 Medical DICOM (`.dcm`), and NASA/IAU FITS (`.fits`) scientific imaging (`crates/image-codecs/src/scientific.rs`).
- **Real 3D Spatial Formats: FBX, Alembic, Collada, USDC & Gaussian Splats (#187):** Clean-room 3D interchange for Autodesk FBX (`.fbx` ASCII & Kaydara binary), Alembic (`.abc` Ogawa), Collada (`.dae` ISO/PAS 17506), Pixar Binary Crate (`.usdc` `PXR-USDC`), and 3D Gaussian Splats (`.splat` SH0 + covariance) (`crates/spatial-io/src/`).
- **Publications & Vector Motion: EPUB 3, PowerPoint PPTX, Lottie & WMF (#188):** Reflowable/fixed-layout EPUB 3 (`.epub`), OpenXML PowerPoint slide decks (`.pptx`), Bodymovin/Lottie vector animations (`.json`), and Windows Metafile (`.wmf` Aldus Placeable 16-bit GDI) (`crates/pipeline/src/publications.rs`).

### Advanced Broadcast, Color & Delivery Paradigms (M40)

- **3D Look-Up Table (LUT) Transcoding & Baking Engine (#189):** Bidirectional 1D/3D LUT conversion across Adobe/Iridas `.cube`, Autodesk/Lustre `.3dl`, and Lumetri `.look` with tetrahedral interpolation, lattice resizing (`17³`, `33³`, `65³`), and direct video frame color baking (`crates/color/src/lut.rs`).
- **Universal Subtitle & Closed Caption Transcoding Matrix (#190):** Bidirectional subtitle conversion across SubRip (`.srt`), WebVTT (`.vtt`), Scenarist CEA-608 (`.scc`), MacCaption CEA-708 (`.mcc`), SMPTE-TT / IMSC1 (`.ttml`), SubStation Alpha (`.ass`/`.ssa`), and EBU-STL (`.stl` GSI/TTI binary blocks) with rational framerate retiming and timecode offset shifting (`crates/pipeline/src/captions.rs`).
- **Broadcast MPEG-TS Multiplexing & Apple Spatial Video (MV-HEVC) (#191):** Clean-room 188-byte MPEG-TS transport stream packetizer (`.ts`, `.m2ts`, `.mts`) with `0x47` sync byte alignment, PAT (PID `0x0000`), PMT (PID `0x0100`), 90 kHz PCR timestamps, and Annex B / ADTS elementary stream encapsulation (`crates/muxers/src/ts.rs`), plus Apple Vision Pro stereoscopic MV-HEVC signaling (`svhd`, `st3d`, `lhvC` boxes) in QuickTime/MP4 containers (`crates/muxers/src/mp4.rs`).

### Quality, Robustness & CI Verification (M29)

- **Real-World Media QA Matrix (#41):** CI regression matrix testing multi-format media sources against independent `ffmpeg` and `ffprobe` decoders across Ubuntu, Windows, and macOS.
- **Fuzzing & Robustness Suite (#43):** 7 bounded libFuzzer targets (`fuzz/fuzz_targets/`), 14 crate integration fuzz suites, and nightly robustness CI testing malformed media files.
- **Packaging Smoke Tests (#42):** Clean-runner verification scripts in `packaging/smoke/` for macOS DMG, Windows MSI, and Linux AppImage/deb/rpm/tarball.
- **Zero Panics:** Pure Rust enforcement across workspace with `#![forbid(unsafe_code)]` and workspace-wide `#![deny(clippy::unwrap_used)]`.

---

## [0.1.0-alpha.1] - 2026-11-04

Initial public alpha release scaffolding the core architecture, headless CLI, initial egui shell, and basic codec plumbing.
