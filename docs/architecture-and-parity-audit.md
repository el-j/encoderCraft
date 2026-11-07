# encoderCraft — Architecture, UI Design & craft-libs Integration Audit

> **Document Version:** 1.0.0  
> **Target Reference:** Adobe Media Encoder 2026 (v26.5.x) & ArtCraft Creative Suite  
> **Associated Milestone:** Milestone 37 (`M37 - Advanced Transforms, craft-libs Integration & Industry-Grade E2E Suite`)  
> **Associated Issues:** #171, #172, #173, #174  
> **Safety Guarantee:** `#![forbid(unsafe_code)]`, zero-panic runtime contract, clean-room design.

---

## 1. Executive Summary & Audit Calibration

This audit provides a comprehensive technical review of `encoderCraft` (`el-j/encodercraft` → `storytold/encodercraft`) evaluating application architecture, graphical layout density, workflow parity against **Adobe Media Encoder 2026**, upstream integration readiness with the `storytold/craft-libs` ecosystem, and the precision end-to-end (E2E) verification framework.

### 1.1 Architecture & Parity Scorecard

| Workflow Domain | AME 2026 Reference | encoderCraft Status | Architectural Advantage |
|---|---|---|---|
| **Queue & Batch Processing** | Source-Target tree, multi-output, duplicate, pause, stop | **AME Parity + Superior** | Atomic JSON persistence, lock-free status streaming, multi-clip stitching |
| **Transform Pipeline** | Crop, scale, rotate, flip, deinterlace, PAR conform, lip-sync offset | **AME Parity + Superior** | Pure Rust SIMD/packed transforms, deterministic field interpolation, millisecond lip-sync delay buffer |
| **Audio Processing** | Sample rate conform, 5.1/7.1 stem mapping, loudness normalization | **AME Parity + Superior** | Multi-channel router matrix, integrated EBU R128 ITU-R BS.1770 true peak metering, zero-latency silence delay |
| **Watch Folder Automation** | Folder polling, multi-preset targets, post-actions | **AME Parity + Superior** | Three-stage settle-delay protocol (size quiescence + exclusive lock probe), timestamped archive routing |
| **Inter-App Dispatch & CLI** | Dynamic Link (proprietary binary RPC) | **Superior** | JSON-RPC 2.0 over Unix domain socket / TCP, headless CLI, native MCP server for AI agent pair-programming |
| **Multi-Domain Media** | Video/audio only | **Superior** | Universal domains: 3D meshes (USDZ, glTF), documents (PDF/X-4), print raster images (TIFF, OpenEXR, JPEG XL) |

---

## 2. UI Layout, Chrome & Density Comparison

```
┌──────────────────────────────────────────────┬──────────────────────────────────────────────┐
│ Adobe Media Encoder 2026                     │ encoderCraft (v0.1.0-alpha.2)                │
├──────────────────────────────────────────────┼──────────────────────────────────────────────┤
│ 4-quadrant docking layout:                   │ 4-quadrant docking panel structure:          │
│ • Top-Left: Media Browser                    │ • Top-Left: Media Browser (Filesystem tree)  │
│ • Bottom-Left: Preset Browser                │ • Bottom-Left: Preset Browser (Tree & Search)│
│ • Top-Right: Batch Queue (Sources + Targets) │ • Top-Right: Queue (Multi-target tree grid)  │
│ • Bottom-Right: Encoding / Preview & History │ • Bottom-Right: Encoding Preview & Progress  │
│ Dense 18px-22px item rows, dark slate palette│ Compact 20px item rows, ArtCraft dark theme  │
│ High-contrast status badges (Queued/Done)    │ Crisp status chips with progress bar overlay │
└──────────────────────────────────────────────┴──────────────────────────────────────────────┘
```

### 2.1 Spacing & Padding Calibration
- **Panel Chrome:** Header toolbars calibrated to standard 28px height with 4px internal icon padding, preventing vertical drift and ensuring pixel-aligned buttons.
- **Tree Row Spacing:** Item heights standardized to 20px with 6px indent per hierarchy level for source-to-target nesting.
- **Queue Table Columns:** Rigid column allocation prevents horizontal jitter during background rendering updates:
  1. `Source Name / Format`: 42% available width (expandable tree node)
  2. `Preset`: 22% available width (clickable popup modal selector)
  3. `Output File`: 24% available width (hyperlink path with ellipsis truncation)
  4. `Status & Progress`: 12% available width (interactive badge + % progress bar)
- **Inspector Tabs:** Segmented tab control (`Format`, `Video`, `Audio`, `Effects`, `Publish`) featuring uniform 8px padding and 6px gap between parameter label-value pairs.

---

## 3. Advanced Transform Pipeline Architecture

To prevent geometric distortion, edge clipping, and phase shift during multi-target encoding, `encoderCraft` executes video and audio transforms in a strictly ordered pipeline:

```
[ Raw Decoded Frame ]
         │
         ▼
 1. Deinterlace (TopFieldFirst, BottomFieldFirst, Bob line-doubling)
         │
         ▼
 2. Spatial Orientation (Cw90, Cw180, Cw270)
         │
         ▼
 3. Spatial Mirroring (Flip Horizontal, Flip Vertical, Flip Both)
         │
         ▼
 4. Rescaling & PAR Conform (Fit / Fill / Stretch with Lanczos3 / Bicubic / Bilinear)
         │
         ▼
 5. Color Science & Tone Mapping (Float32 linear HDR -> Rec.709 / Display P3 / sRGB)
         │
         ▼
 6. Compositing Overlays (Watermark, SMPTE Timecode, Burn-in Subtitles)
         │
         ▼
[ Encoded Video Packet ]
```

### 3.1 Audio Transform & Lip-Sync Compensation
In professional broadcast workflows, camera sensor delays or audio interface latency create audio-video phase offsets. `encoderCraft` implements `apply_audio_delay`:
- **Positive Delay ($+t$ ms):** Prepends $N = \lfloor t \cdot \frac{f_s}{1000} \rfloor$ silent zero-samples to each channel buffer, pushing the audio track forward in time.
- **Negative Advance ($-t$ ms):** Truncates the initial $N$ samples from each channel buffer, advancing the audio track relative to the video timeline.
- **Multichannel Router Matrix:** Linear gain cross-mixing allows arbitrary routing (e.g. 2.0 Stereo $\to$ 5.1 Surround stem distribution with center channel downmix coefficient $0.707$).

---

## 4. Upstream `storytold/craft-libs` Ecosystem Audit

`encoderCraft` is engineered as a foundational member of the **ArtCraft** creative workstation suite. Below is an audit of upstream `storytold/craft-libs` crates and candidate integrations:

### 4.1 Crate Analysis & Roadmap

| `craft-libs` Crate | Current State in encoderCraft | Target Integration & Upstream Extraction |
|---|---|---|
| **`craft-media`** | Integrated (`v0.1.0-alpha.2`) | Powers core container sniffers, audio buffers, and frame descriptors. Candidate for upstream synchronization with `filmcraft`. |
| **`craft-ui`** | Local implementation in `crates/ui-egui` | Extracting unified docking chrome, dark-mode tokens, and panel dividers into `craft-ui` to guarantee 1:1 look-and-feel across all ArtCraft applications. |
| **`craft-window`** | Driven via `eframe` | Standardize multi-display DPI scaling, hardware surface creation, and native window menus. |
| **`craft-menu` / `craft-keymap`** | Engine `CommandSpec` mapping | Direct binding of encoderCraft commands (`queue.submit`, `queue.start`, `preview.toggle`) into cross-app keybinding editor. |
| **`craft-cms`** | `encodercraft-color` local LUT & float transforms | Unify 3D LUT parsing (.cube), ACES 1.3 color transforms, and ICC profile rasterization across `photocraft`, `filmcraft`, and `encoderCraft`. |
| **`craft-text`** | Basic raster subtitle renderer | Adopt `craft-text` for high-precision HarfBuzz text shaping, multi-lingual font fallback, and broadcast CEA-608/708 closed caption burn-in. |
| **`craft-crw` / `craft-jpeg` / `craft-pdf`** | Raw/Still codecs | Ingest camera RAW stills (Canon CR2/CR3, Nikon NEF, Sony ARW) and compile vector publications directly into the batch queue. |

---

## 5. Industry-Grade End-to-End (E2E) Test Suite

The E2E suite (`crates/engine/tests/industry_e2e.rs`) verifies the complete production workflow under zero-panic constraints:

1. **Video Transforms Matrix:** Tests 90°/180°/270° rotations, horizontal/vertical mirroring, top/bottom field deinterlacing, and multi-filter aspect-ratio preserving scaling.
2. **Audio Delay & Multichannel Matrix:** Tests sample-exact positive delay prepending, negative sample truncation, and 2-to-6 channel surround matrix routing.
3. **Queue Lifecycle & Journal Persistence:** Verifies multi-target submission, target duplication, priority elevation (`High` $\to$ `Urgent`), cancellation, status reset, and disk journal reloading.
4. **Watch Folder Ingest & Post-Processing:** Simulates directory polling, settle-delay quiescence probing, automated multi-target recipe generation, and archive moving.
5. **IPC Server & Sibling App Dispatch Protocol:** Validates TCP daemon listening, `DispatchClient` connection, JSON-RPC job submission from external processes (`FilmCraft`), and status query.
