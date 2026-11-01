# ArtCraft Shared Codecs & Media Libraries Migration Blueprint
## Unifying Media Codecs into `craft-libs` as the One True Source

> **Status:** Canonical Architectural Proposal & Migration Plan  
> **Target:** Ingestion into `github.com/storytold/craft-libs`  
> **Author:** `el-j` for the Storytold ArtCraft Organization  
> **Reviewed against:** `filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, `encodercraft`  
> **Date:** 2026-11-02  

---

## 1. Executive Summary & Problem Statement

### 1.1 The Problem: Codec Fragmentation Across Applications
Currently across the ArtCraft ecosystem, high-performance, pure-Rust media codecs and format processors are isolated inside single consumer applications:
- **`filmcraft`** hosts `filmcraft-h264enc` (H.264 video encoder), `filmcraft-prores` (Apple ProRes encoder/decoder), `filmcraft-aac` (AAC-LC audio encoder), `filmcraft-captions` (subtitle engine), and `filmcraft-bitstream`.
- **`effectcraft`** hosts `effectcraft-hevcenc` (HEVC/H.265 video encoder), `effectcraft-av1enc` (AV1 video encoder), and `effectcraft-vp9enc` (VP9 video encoder).
- **`soundcraft`** hosts `soundcraft-audio-io` (universal audio decoder for MP3, FLAC, Ogg Vorbis, AAC, ALAC, CAF, WAV, and native FLAC/AIFF encoders) and `soundcraft-dsp`.
- **`encodercraft`** requires **all** of these codecs simultaneously to deliver complete **Adobe Media Encoder** parity (universal transcoding, batch queues, watch folders).

Having codecs reside inside individual desktop applications causes:
1. **Inverted Dependency Graphs:** Sibling applications (`encodercraft`) must depend on sibling repositories (`filmcraft`, `effectcraft`, `soundcraft`), creating unwanted cross-app coupling.
2. **Duplication Risk:** Enhancements, bug fixes, SIMD accelerations, and fuzz testing done in one app are not automatically shared with other applications.
3. **Packaging Barriers:** Downstream CLI tools, render daemons, and headless render farm nodes cannot consume codecs without pulling in application-specific dependencies.

### 1.2 The Solution: `craft-libs` as the One True Source
The Storytold ArtCraft organization maintains **`craft-libs`** (`github.com/storytold/craft-libs`) specifically as the centralized repository for shared foundation code (`craft-media`, `craft-ui`, `craft-text`, `craft-cms`, `craft-crw`, `craft-jpeg`).

**Architectural Principle:** All codec implementations, format parsers, bitstream readers, DSP algorithms, and subtitle processors **must reside in `craft-libs`**, enabling every ArtCraft application (`filmcraft`, `effectcraft`, `soundcraft`, `photocraft`, `encodercraft`) to consume them from one canonical, versioned source.

---

## 2. Complete Inventory & Target Mapping

The following matrix identifies every codec and media component currently in individual applications and maps it to its target location in `craft-libs`:

| Component / Crate | Current Host Repo | Primary Capabilities | Target Location in `craft-libs` | Target Crate Name | Priority | Consuming Apps |
|---|---|---|---|---|---|---|
| **`h264enc`** | `filmcraft/crates/h264enc` | Pure-Rust H.264 Baseline/Main video encoder | `crates/codecs/h264enc` | `craft-h264enc` (or `craft-codecs::h264`) | **P0** | `filmcraft`, `effectcraft`, `encodercraft` |
| **`prores`** | `filmcraft/crates/prores` | Pure-Rust Apple ProRes encoder (Proxy, LT, Standard, HQ, 4444) | `crates/codecs/prores` | `craft-prores` (or `craft-codecs::prores`) | **P0** | `filmcraft`, `effectcraft`, `encodercraft`, `soundcraft` |
| **`hevcenc`** | `effectcraft/crates/hevcenc` | Pure-Rust HEVC / H.265 video encoder | `crates/codecs/hevcenc` | `craft-hevcenc` (or `craft-codecs::hevc`) | **P0** | `effectcraft`, `filmcraft`, `encodercraft` |
| **`av1enc`** | `effectcraft/crates/av1enc` | Pure-Rust AV1 encoder with OBU packing | `crates/codecs/av1enc` | `craft-av1enc` (or `craft-codecs::av1`) | **P0** | `effectcraft`, `filmcraft`, `encodercraft` |
| **`vp9enc`** | `effectcraft/crates/vp9enc` | Pure-Rust Google VP9 video encoder | `crates/codecs/vp9enc` | `craft-vp9enc` (or `craft-codecs::vp9`) | **P0** | `effectcraft`, `filmcraft`, `encodercraft` |
| **`aac`** | `filmcraft/crates/aac` | Pure-Rust CBR AAC-LC encoder + AudioSpecificConfig | `crates/codecs/aac` | `craft-aac` (or `craft-codecs::aac`) | **P0** | `filmcraft`, `effectcraft`, `encodercraft` |
| **`audio-io`** | `soundcraft/crates/audio-io` | Universal audio decode (MP3/FLAC/Ogg/AAC/CAF) + FLAC/AIFF encode | `crates/audio-io` | `craft-audio-io` | **P0** | `soundcraft`, `filmcraft`, `encodercraft`, `effectcraft` |
| **`captions`** | `filmcraft/crates/captions` | SRT, WebVTT, SCC, MCC, STL, TTML parser, sidecar writer, raster burner | `crates/captions` | `craft-captions` | **P0** | `filmcraft`, `encodercraft`, `soundcraft` |
| **`bitstream`** | `filmcraft/crates/bitstream` | Exp-Golomb bitstream reader/writer, RBSP parsing, NAL framing | `crates/bitstream` | `craft-bitstream` | **P0** | Universal foundation |
| **`time`** | `filmcraft/crates/time` | Nanosecond tick arithmetic (`254_016_000_000/s`), exact rational time | `crates/time` | `craft-time` | **P1** | Universal foundation |
| **`dnx`** | `filmcraft/crates/dnx` | Avid DNxHD and DNxHR video codec | `crates/codecs/dnx` | `craft-dnx` | **P1** | `filmcraft`, `encodercraft` |
| **`apv`** | `filmcraft/crates/apv` | Apple Advanced Pixel Video / ProRes RAW codec | `crates/codecs/apv` | `craft-apv` | **P1** | `filmcraft`, `encodercraft` |
| **`audio-dsp`** | `encodercraft` / `soundcraft` | Polyphase sinc resampler, ITU-R BS.1770-4 / EBU R128 loudness, limiter | `crates/audio-dsp` | `craft-audio-dsp` | **P1** | `soundcraft`, `filmcraft`, `encodercraft` |
| **`lut`** | `encodercraft/crates/pipeline/lut` | Adobe `.cube` 3D LUT parser and trilinear interpolator | `crates/color/lut` | `craft-color::lut` | **P1** | `photocraft`, `filmcraft`, `effectcraft`, `encodercraft` |
| **`spatial-io`** | `encodercraft/crates/spatial-io` | 3D mesh geometry export: Wavefront OBJ, binary STL, binary PLY, glTF 2.0 | `crates/spatial-io` | `craft-spatial-io` | **P2** | `texturecraft`, `encodercraft`, `cadcraft` |

---

## 3. Target Architecture in `craft-libs`

### 3.1 Proposed Workspace Layout for `craft-libs`

```text
craft-libs/
├── Cargo.toml                      (workspace root)
├── crates/
│   ├── media/                      (existing: container demuxers & ISOBMFF/Matroska)
│   ├── bitstream/                  (NEW: craft-bitstream - Exp-Golomb bitstream primitives)
│   ├── time/                       (NEW: craft-time - exact tick & rational timecodes)
│   ├── codecs/                     (NEW: craft-codecs - umbrella or discrete crates)
│   │   ├── h264/                   (H.264 encode/decode)
│   │   ├── prores/                 (Apple ProRes encode/decode)
│   │   ├── hevc/                   (HEVC/H.265 encode/decode)
│   │   ├── av1/                    (AV1 encode/decode)
│   │   ├── vp9/                    (VP9 encode/decode)
│   │   └── aac/                    (AAC-LC encode/decode)
│   ├── audio-io/                   (NEW: craft-audio-io - universal audio decode/encode)
│   ├── audio-dsp/                  (NEW: craft-audio-dsp - resampling, BS.1770 loudness)
│   ├── captions/                   (NEW: craft-captions - SRT, VTT, SCC, MCC, TTML, burner)
│   ├── spatial-io/                 (NEW: craft-spatial-io - OBJ, STL, PLY, glTF 2.0)
│   ├── cms/                        (existing: color management system)
│   ├── jpeg/                       (existing: pure-Rust JPEG decoder/encoder)
│   ├── crw/                        (existing: Camera RAW processing)
│   ├── pdf/                        (existing: PDF document export)
│   ├── text/                       (existing: HarfRust/Skrifa typography layout)
│   └── ui/                         (existing: shared egui tokens and widgets)
```

### 3.2 Canonical Public Traits for Media Codecs

To guarantee seamless interchange, all video and audio encoders in `craft-libs` must implement standard object-safe or monomorphic traits:

```rust
// In craft-codecs (or craft-media::codec)
pub trait VideoEncoder: Send {
    /// Encodes a raw video frame into one or more compressed packets.
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, MediaError>;
    
    /// Flushes any delayed frames or bitstream buffers at the end of stream.
    fn flush(&mut self) -> Result<Vec<EncodedPacket>, MediaError>;
    
    /// Returns the FOURCC code representing the encoded bitstream.
    fn fourcc(&self) -> [u8; 4];
    
    /// Returns codec-private extradata (e.g. AVC SPS/PPS, HEVC VPS/SPS/PPS, AV1 config OBU).
    fn codec_private(&self) -> &[u8];
}

pub trait AudioEncoder: Send {
    /// Encodes planar floating-point audio samples into compressed packets.
    fn encode_buffer(&mut self, buffer: &AudioSampleBuffer) -> Result<Vec<Vec<u8>>, MediaError>;
    
    /// Flushes any priming or encoder tail samples.
    fn flush(&mut self) -> Result<Vec<Vec<u8>>, MediaError>;
    
    /// Returns AudioSpecificConfig or channel configuration header bytes.
    fn audio_specific_config(&self) -> Vec<u8>;
}
```

---

## 4. Phase-by-Phase Migration Plan

### Phase 1: Bitstream, Time & Video Codecs (P0)
1. **Move Bitstream:** Migrate `filmcraft/crates/bitstream` to `craft-libs/crates/bitstream`. Update `filmcraft` to depend on `craft-bitstream`.
2. **Move Video Encoders:**
   - Copy `filmcraft/crates/h264enc` -> `craft-libs/crates/codecs/h264enc`
   - Copy `filmcraft/crates/prores` -> `craft-libs/crates/codecs/prores`
   - Copy `effectcraft/crates/hevcenc` -> `craft-libs/crates/codecs/hevcenc`
   - Copy `effectcraft/crates/av1enc` -> `craft-libs/crates/codecs/av1enc`
   - Copy `effectcraft/crates/vp9enc` -> `craft-libs/crates/codecs/vp9enc`
3. **Verify Pure-Rust Compliance:** Ensure `#![forbid(unsafe_code)]` and `#![deny(clippy::unwrap_used)]` across all moved crates.

### Phase 2: Audio I/O & Audio Encoders (P0)
1. **Move Audio I/O:** Migrate `soundcraft/crates/audio-io` to `craft-libs/crates/audio-io`.
2. **Move AAC Encoder:** Migrate `filmcraft/crates/aac` to `craft-libs/crates/codecs/aac`.
3. **Deprecate App-Specific Wrappers:** Replace duplicate FLAC/AIFF encoders with the unified `craft-audio-io` implementation.

### Phase 3: Subtitles & Captions Engine (P0)
1. **Move Captions:** Migrate `filmcraft/crates/captions` to `craft-libs/crates/captions`.
2. **Standardize Dependencies:** Ensure `craft-captions` depends on `craft-time` and `craft-text` within `craft-libs`.
3. **Shared Consumption:** Both `filmcraft` and `encodercraft` update their imports to `craft-captions`.

### Phase 4: Audio DSP & Post-Production Effects (P1)
1. **Move Audio DSP:** Extract sinc resampler, ITU-R BS.1770-4 loudness normalizer, and True-Peak limiter into `craft-libs/crates/audio-dsp`.
2. **Move 3D LUT Engine:** Extract the clean-room `.cube` parser and trilinear interpolation engine from `encodercraft/crates/pipeline/src/lut.rs` into `craft-libs/crates/cms` or `craft-libs/crates/color`.

### Phase 5: Complete Downstream Integration
Once all media crates are merged and tagged in `storytold/craft-libs`:
1. `filmcraft`, `effectcraft`, `soundcraft`, and `encodercraft` update their root `Cargo.toml` to depend directly on `craft-libs`.
2. All cross-sibling dependencies (`encodercraft` -> `filmcraft`, `effectcraft`, `soundcraft`) are removed.
3. Every application references the single official source: `https://github.com/storytold/craft-libs.git`.

---

## 5. Current Implementation in `encoderCraft`

Pending the completion of the `craft-libs` upstream migration:
- **Canonical Manifests:** `crates/*/Cargo.toml` and root `Cargo.toml` declare **official GitHub repositories** (`git = "https://github.com/storytold/..."`) as the authoritative sources.
- **Local Developer Patches:** `[patch."https://github.com/storytold/..."]` allows local multi-repo working trees to build seamlessly offline.
- **Decoupled Architecture:** `encodercraft-video-codecs`, `encodercraft-audio-dsp`, and `encodercraft-pipeline` isolate all sibling calls behind clean adapter traits, ensuring that when the crates move to `craft-libs`, only Cargo dependency declarations will need updating—zero transcoding engine code will need to change.
