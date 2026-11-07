# Changelog

All notable changes to `encoderCraft` are documented here. The project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0-alpha.2] - 2026-11-07

Alpha 2 is a major stabilization and feature expansion release delivering full parity with Adobe Media Encoder workflows and resolving all alpha.1 defects.

### Highlights & Fixed Issues from Alpha 1

- **Multi-Item Queue Execution (#6, #53, #54):** Fixed the single-item execution bug. Multi-item batches now drain completely from a single Start click with worker pool scheduling honoring `max_parallel_jobs`.
- **Full Video Transcoding Pipeline (#10, #15, #17, #18, #52):** Fixed empty video outputs. Integrated robust decoding via `encodercraft-source` for H.264, HEVC, VP8, VP9, AV1, MPEG-2, Apple ProRes, and Avid DNxHD/HR with frame-accurate timing, faststart MP4 muxing, and bounded memory streaming.
- **Studio Still Image Pipeline (#20, #22, #23, #24, #47, #56, #57):** Fixed grey-image output. Added full 7x7 still image conversion matrix (PNG, JPEG, TIFF, WebP, GIF, BMP, EXR), numbered image sequences (`shot_%04d.png`), 16-bit TIFF, CMYK JPEG, and EXIF orientation handling.
- **Live Preview Monitor & Video Scopes (#25, #27, #28, #29, #55):** Replaced placeholder colour bars with a live video preview canvas, JKL transport controls, frame-accurate In/Out trimming handles, A/B split comparison slider, waveform, vectorscope, RGB parade, histogram scopes, and real-time encoding progress.
- **Dockable Workspace Shell & Theme Neutrality (#30, #31, #32, #33, #115, #167):** Eliminated garbled text glyphs. Added dockable workspace tabs, CommandSpec registry with remappable shortcuts, native macOS menu bar with `--in-window-menus` override, saved window geometry, and authentic Adobe reference neutral greys for Medium Dark theme.

### Universal Media Domain Expansion (M30)

- **CAD & Technical Drafting (#70, #71):** Direct ingest and conversion for AutoCAD DXF, DWG, HPGL plotters, and `.cadcraft` to SVG and vector blueprint PDF.
- **Prepress & Publication (#72):** Ingest and export for Adobe InDesign INDD, IDML, and `.designcraft` to EPUB 3 and ISO 15930 PDF/X-4 with OutputIntents.
- **Layered Graphics & Photography (#73):** Photoshop PSD, PSB large documents, and `.photocraft` container conversion with multi-layer preservation and multi-part OpenEXR.
- **Vector Graphics & Illustration (#74):** Clean-room dual-stream Illustrator AI, multi-artboard `.vectorcraft`, SVG 2.0, and PostScript EPS interop.
- **Motion Graphics & NLE Timelines (#75):** After Effects AEP and Premiere PRPROJ sequence ingestion, RIFX parsing, Lottie JSON, Final Cut Pro XML, and OpenTimelineIO.
- **Audio DAW & Multitrack Sessions (#76):** Audition SESX and SoundCraft conversion with DAW session stems, clip gain/pan mixing, and EBU R128 loudness normalization.

### Professional Broadcast & Effects Pipeline (M31–M36)

- **Export Settings Effects (#139, #140, #141, #142, #143):** Burn-in timecode overlays (BITC), graphic watermarks, dailies name overlays, broadcast video limiting (IRE 100/103), SDR conform HDR tone-mapping (HLG/PQ to Rec.709), and interactive source frame cropping.
- **Broadcast Delivery Profiles (#149):** Factory compliance presets for Sony XDCAM HD422, Panasonic AVC-Intra 100, DPP AS-11 UK HD Delivery (25p / EBU R128), and AMWA AS-10 in MXF OP-1a with 8-channel 24-bit 48 kHz PCM audio.
- **Multichannel Audio Matrix (#148):** Visual matrix routing supporting discrete mono stems, stereo pairs, 5.1 surround, and 7.1 surround track layouts.
- **Queue Automation & Watch Folders (#144, #145, #146, #147):** Idle timeout auto-start, unique output filename collision resolution (`_1`, `_2`), multi-target watch folder output routing with automatic source archiving, audio completion chime, and clean exit queue pruning.
- **UI Table Precision & Source Inspection (#160, #161, #162, #164, #165, #166):** Strict column boundary locking with bounded cell clipping (`render_column_cell`), ISO date timestamps (`YYYY-MM-DD HH:MM`), adaptive preset columns, stream essence inspection modal (Cmd+I), and sequence assembly stitch dialog.

### Quality, Robustness & CI Verification (M29)

- **Real-World Media QA Matrix (#41):** CI regression matrix testing multi-format media sources against independent `ffmpeg` and `ffprobe` decoders across Ubuntu, Windows, and macOS.
- **Fuzzing & Robustness Suite (#43):** 7 bounded libFuzzer targets (`fuzz/fuzz_targets/`), 14 crate integration fuzz suites, and nightly robustness CI testing malformed media files.
- **Packaging Smoke Tests (#42):** Clean-runner verification scripts in `packaging/smoke/` for macOS DMG, Windows MSI, and Linux AppImage/deb/rpm/tarball.
- **Zero Panics:** Pure Rust enforcement across workspace with `#![forbid(unsafe_code)]` and workspace-wide `#![deny(clippy::unwrap_used)]`.

---

## [0.1.0-alpha.1] - 2026-11-04

Initial public alpha release scaffolding the core architecture, headless CLI, initial egui shell, and basic codec plumbing.
