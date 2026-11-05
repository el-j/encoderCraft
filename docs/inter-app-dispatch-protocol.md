# Inter-App Dispatch Protocol Specification: "Send to EncoderCraft"

> **Protocol Version:** 1.0.0 (Draft)  
> **Target:** ArtCraft Application Ecosystem (`storytold/*`)  
> **Prepared for:** `encoderCraft` (`el-j/encodercraft` → `storytold/encodercraft`)  
> **Status:** Architecture & Implementation Specification  

---

## 1. Architectural Motivation

In professional workflows, media creation applications (NLEs, motion compositors, DAWs, photo editors, 3D suites) should not lock their user interface while rendering multi-gigabyte exports or complex multi-target delivery batches.

In the Adobe ecosystem, Adobe Premiere Pro, After Effects, and Audition delegate heavy encoding tasks to Adobe Media Encoder via dynamic background linking ("Queue in Media Encoder").

In the ArtCraft ecosystem, **`encoderCraft`** provides this centralized role. Any ArtCraft application—as well as third-party tools, shell scripts, and autonomous AI agents—can offload rendering, format conversion, and batch queuing to `encoderCraft`.

```mermaid
flowchart TD
    subgraph CreativeApps["ArtCraft Creative Applications"]
        FC["FilmCraft\n('Queue in EncoderCraft')"]
        EC["EffectCraft\n('Export Composition')"]
        SC["SoundCraft\n('Batch Stems & LUFS')"]
        PC["PhotoCraft\n('Batch RAW / ICC')"]
        TC["TextureCraft\n('Batch PBR / UDIM')"]
        DC["DesignCraft\n('Prepress PDF/X')"]
    end

    subgraph DispatchLayers["Unified Dispatch Layer (craft-dispatch crate)"]
        IPC["Local Domain Socket / Named Pipe\n(High throughput, zero network)"]
        RPC["Loopback JSON-RPC (127.0.0.1:8998)\n(Cross-process / IDE / Web)"]
        CLI["Headless CLI: encodercraft queue add\n(Shell scripts / xtask)"]
        MCP["MCP Server Protocol\n(Agentic pair programmers)"]
        WF["Watch Folder Ingestion\n(Hot folder file drop)"]
    end

    subgraph EncoderCraftCore["encoderCraft Daemon & Engine"]
        Queue["Persistent Queue & Scheduler"]
        WorkerPool["Multi-Threaded Worker Pool"]
        DualEngine["Dual Engine:\nPure-Rust Native + HW / FFmpeg"]
    end

    CreativeApps --> DispatchLayers
    DispatchLayers --> EncoderCraftCore
    EncoderCraftCore --> Output["Final Multi-Format Deliverables\n(Broadcast, Web, Archive, PBR, Print)"]
```

---

## 2. Dispatch Paradigms

To accommodate different media types, file sizes, and hardware constraints, the protocol defines **three dispatch modes**:

### Mode A: File Source Hand-off (Direct / Mezzanine)
- **Use Case:** The file already exists on disk (e.g., camera RAW photos, 3D meshes, audio recordings, video rushes), or the host app wrote a fast, uncompressed intermediate file (e.g., ProRes 4444 or 32-bit float WAV).
- **Mechanism:** The host app sends a job specification containing absolute paths to the source file(s) and the desired output presets.
- **Overhead:** Near-zero IPC overhead; disk I/O only.

### Mode B: Dynamic Zero-Copy Frame Streaming (Direct Pipe)
- **Use Case:** Heavy timelines (FilmCraft video sequences, EffectCraft motion graphics compositions) where writing hundreds of gigabytes of intermediate uncompressed frames to disk would be slow and wasteful.
- **Mechanism:**
  1. The host app opens a dedicated IPC connection (Unix Domain Socket, Windows Named Pipe, or shared memory ring buffer) to `encoderCraft`.
  2. The host app renders frames on-demand in its headless background worker and writes raw video frames (`-f rawvideo -pix_fmt rgba64le` or `yuv422p10le`) and planar audio chunks (`-f f32le`).
  3. `encoderCraft` consumes the stream directly into its encoder pipelines without touching disk storage.
- **Overhead:** Memory bandwidth only; instantaneous startup.

The encoderCraft-local transport primitive uses an iceoryx2 shared-memory service. Its versioned `ECST` packet header carries the packet kind, PTS, duration, and payload length; payloads are capped at 512 MiB and the service applies two-sample bounded buffering with retry-until-delivered backpressure. A received packet borrows its payload from the shared-memory sample rather than copying it into a `Vec`.

`SourceDescriptor::Pipe` is consumed by the transcode pipeline (`encodercraft_source::pipe::PipeSource`). The producer declares the format in the job (`width`, `height`, `frame_rate`, `sample_rate`, `channels`) and follows this protocol: create the publisher, `wait_for_subscriber`, publish packets, then `finish` (publishes end-of-stream and waits for the receiver to detach so unread packets are not discarded). Video packets carry tightly packed RGBA8 (`width * height * 4` bytes); audio packets carry interleaved little-endian f32 frames. Wrong-sized packets, non-finite samples, invalid declarations, streams above 1 GiB and streams idle for 30 s are rejected with typed errors. A stream is consumed once; the pipeline holds it open for the whole job and releases it afterwards. `crates/pipeline/tests/conformance.rs` encodes a streamed source to a decodable MP4. Project-reference rendering (`SourceDescriptor::ProjectReference`) is not connected yet.

### Mode C: Headless Project Delegation (Deferred Rendering)
- **Use Case:** The user queues a complex project in `encoderCraft` and closes the host application completely (freeing GPU/RAM).
- **Mechanism:**
  1. The host app saves a clean snapshot of its project (`.filmcraft`, `.effectcraft`, `.photocraft`, `.texturecraft`).
  2. The job specification points to the project file path, the target sequence/composition ID, and the host app's CLI binary path.
  3. `encoderCraft` executes the host CLI in headless slave mode (e.g., `filmcraft-cli render --headless --sequence-id seq_01 --pipe`) and pipes the output into its encoding engine.

---

## 3. Data Specification: `CraftJobSpec`

All communication uses standard JSON (or Bincode for internal binary IPC). Every job submission conforms to the following schema:

```json
{
  "version": "1.0.0",
  "jobId": "job_01hgb78y9z0123456789abcdef",
  "clientInfo": {
    "appName": "FilmCraft",
    "appVersion": "0.9.4",
    "pid": 48291,
    "userNotes": "Client Approval Cut v3"
  },
  "priority": "Normal",
  "startPolicy": "AutoStart",
  "source": {
    "type": "File",
    "path": "/Volumes/FastMedia/Projects/Commercial_2026/Master_Edit.mov",
    "range": {
      "startTick": 0,
      "durationTicks": 4320000,
      "timebase": 120000
    },
    "metadata": {
      "title": "Spring Launch Commercial",
      "artist": "Studio ArtCraft",
      "timecodeStart": "01:00:00:00"
    }
  },
  "targets": [
    {
      "targetId": "tgt_youtube_4k",
      "outputPath": "/Volumes/FastMedia/Exports/Commercial_YouTube_4K.mp4",
      "presetName": "YouTube 2160p 4K UHD",
      "format": "H264",
      "container": "IsoBmffMp4",
      "video": {
        "width": 3840,
        "height": 2160,
        "frameRate": [60, 1],
        "bitrateMode": {
          "type": "Vbr1Pass",
          "targetKbps": 45000,
          "maxKbps": 60000
        },
        "colorSpace": "Rec709",
        "hardwareAcceleration": "Auto"
      },
      "audio": {
        "codec": "Aac",
        "sampleRate": 48000,
        "channels": 2,
        "bitrateKbps": 320,
        "loudnessNormalization": {
          "standard": "EbuR128",
          "targetLufs": -14.0,
          "maxTruePeakDbfs": -1.0
        }
      },
      "effects": {
        "watermark": null,
        "timecodeBurnIn": false,
        "lutPath": null
      }
    },
    {
      "targetId": "tgt_prores_archive",
      "outputPath": "/Volumes/FastMedia/Exports/Commercial_ProRes_Master.mov",
      "presetName": "Apple ProRes 422 HQ",
      "format": "ProRes",
      "container": "QuickTimeMov",
      "video": {
        "profile": "ProRes422Hq",
        "colorSpace": "Rec709"
      },
      "audio": {
        "codec": "PcmLinear",
        "sampleRate": 48000,
        "channels": 2,
        "bitDepth": 24
      }
    }
  ],
  "notifications": {
    "onCompleteWebhook": "http://127.0.0.1:48291/api/export-done",
    "systemNotification": true
  }
}
```

---

## 4. IPC & Transport Protocols

### 4.1 Transport Layers

| Platform | Primary Transport | Path / Address | Security / Isolation |
|---|---|---|---|
| **macOS & Linux** | Unix Domain Socket | `/tmp/encodercraft-$UID.sock` (UID from the OS; `encodercraft_dispatch::default_socket_path()` is shared by server and clients) | Socket file is created with mode `0600` (current user only); the server refuses to replace a non-socket file at that path. |
| **Windows** | Loopback TCP JSON-RPC (named pipes are not implemented) | `127.0.0.1:8998` (fallback `8999` in the desktop app) | Loopback-only binding. |
| **Local Network / Web** | Loopback TCP JSON-RPC | `127.0.0.1:8998` | Bearer token stored in `~/.config/encodercraft/token`. |
| **Shell & Automation** | Headless CLI command | `encodercraft queue add --spec <file>` | Subprocess exit codes and stdout JSON response. |
| **AI Coding Agents** | Model Context Protocol (MCP) | `stdio` or SSE | Strict tool schemas for agent inspection and submission. |

### 4.2 Lifecycle & Daemon Auto-Launch
When an ArtCraft application prepares to dispatch a job:
1. **Probe Connection:** The host app attempts to connect to the local socket `/tmp/encodercraft-$UID.sock`.
2. **Auto-Launch if Idle:** If connection fails (socket does not exist or refused):
   - The host app checks user preferences (`Launch EncoderCraft GUI` vs. `Spawn Headless Background Daemon`).
   - If GUI is preferred: launches `encodercraft --minimized`.
   - If headless daemon is preferred: spawns `encodercraft-cli daemon --fork`.
3. **Handshake:** Waits up to 3000 ms for socket initialization, then exchanges protocol version.
4. **Submit & Confirm:** Posts `queue.submit` request and receives `jobId`.

---

## 5. Bidirectional JSON-RPC API Methods

### 5.1 Client Requests (Host App → EncoderCraft)

#### `queue.submit`
Submits one or more jobs to the active queue.
- **Params:** `CraftJobSpec`
- **Returns:** `{ "jobId": "...", "status": "Queued", "queuePosition": 2 }`

#### `queue.pause` / `queue.resume`
Pauses or resumes encoding execution across the entire queue or for a specific job.
- **Params:** `{ "jobId": "..." }` (optional; if omitted, pauses the global queue)
- **Returns:** `{ "status": "Paused" }`

#### `queue.cancel`
Immediately aborts processing of a job, terminates active child renderers, and cleans up incomplete temporary files.
- **Params:** `{ "jobId": "...", "deletePartialFiles": true }`
- **Returns:** `{ "status": "Cancelled" }`

#### `queue.list`
Returns real-time status of all queued, active, completed, and failed jobs.
- **Returns:** Array of job statuses with progress, throughput, and estimated completion time.

#### `presets.list`
Returns the full hierarchy of available system, custom, and format presets so the host app can populate its export dialogs dynamically.
- **Returns:** Tree of preset categories, names, formats, and parameter schemas.

### 5.2 Server Notifications (EncoderCraft → Host App)

#### `job.progress`
Emitted at throttled intervals (default: 4 times per second per active job):
```json
{
  "jsonrpc": "2.0",
  "method": "job.progress",
  "params": {
    "jobId": "job_01hgb78y9z0123456789abcdef",
    "targetId": "tgt_youtube_4k",
    "progressFraction": 0.452,
    "currentFrame": 1952,
    "totalFrames": 4320,
    "encodingFps": 74.8,
    "elapsedSeconds": 26.1,
    "estimatedRemainingSeconds": 31.6,
    "pass": 1,
    "totalPasses": 1
  }
}
```

#### `job.completed`
Emitted when all targets of a job have finished successfully:
```json
{
  "jsonrpc": "2.0",
  "method": "job.completed",
  "params": {
    "jobId": "job_01hgb78y9z0123456789abcdef",
    "durationSeconds": 57.7,
    "results": [
      {
        "targetId": "tgt_youtube_4k",
        "outputPath": "/Volumes/FastMedia/Exports/Commercial_YouTube_4K.mp4",
        "fileSizeBytes": 328491024,
        "loudnessReport": {
          "integratedLufs": -14.02,
          "truePeakDbfs": -1.05,
          "loudnessRangeLu": 6.4
        }
      }
    ]
  }
}
```

#### `job.failed`
Emitted if encoding fails due to I/O error, invalid input, or resource exhaustion:
```json
{
  "jsonrpc": "2.0",
  "method": "job.failed",
  "params": {
    "jobId": "job_01hgb78y9z0123456789abcdef",
    "targetId": "tgt_prores_archive",
    "errorCategory": "DiskSpaceExhausted",
    "message": "Target volume has only 120 MB remaining; required 4.2 GB."
  }
}
```

---

## 6. Model Context Protocol (MCP) Agent Interface

For autonomous pair programming and robotic task pipelines, `encoderCraft` exposes an MCP server (`encodercraft-mcp`). This allows AI agents to inspect and control encoding without opening the graphical interface.

### Registered Tools
1. `encodercraft_submit_transcode(source_path, preset_name, output_path)`: Submits a single file transcode job.
2. `encodercraft_batch_convert(folder_path, glob_filter, target_preset, output_folder)`: Batch converts entire directories.
3. `encodercraft_get_queue_status()`: Returns the queue items, active progress, and failures.
4. `encodercraft_list_presets(category_filter)`: Lists available presets.
5. `encodercraft_inspect_media(file_path)`: Analyzes media streams, codecs, bitrates, audio channels, and color tags.

---

## 7. The Shared Client Crate: `craft-dispatch`

To make adoption effortless across all ArtCraft applications, a shared Rust crate named `craft-dispatch` will be placed in `craft-libs/crates/dispatch`.

Any app needs only two lines of code to dispatch a job:
```rust
use craft_dispatch::{Client, CraftJobSpec};

let client = Client::connect_default()?;
let job_id = client.submit_job(&my_job_spec)?;
```

If `encoderCraft` is installed, the job appears instantly in the queue. If not installed, `craft-dispatch` gracefully returns `DispatchError::EncoderCraftNotInstalled`, allowing the host app to offer a download prompt or fall back to its internal synchronous export.

---

## 8. Sibling Ecosystem Integration Tracking

Every application across the entire ArtCraft creative suite has a dedicated companion issue tracking its export hand-off integration with `encoderCraft`:

| Application | Domain & Media Specialty | Delegated Workflows | Tracking Issue |
| :--- | :--- | :--- | :--- |
| **FilmCraft** | Video Editing & Timeline NLE | Multi-track sequence rendering, rough cuts, mastering | [storytold/filmcraft#3942](https://github.com/storytold/filmcraft/issues/3942) |
| **EffectCraft** | Compositing & Motion Graphics | High-overhead multi-pass compositions, VFX, particle cache | [storytold/effectcraft#2816](https://github.com/storytold/effectcraft/issues/2816) |
| **SoundCraft** | Digital Audio Workstation (DAW) | Multitrack stem exports, surround/Atmos mixdowns, EBU R128 | [storytold/soundcraft#1276](https://github.com/storytold/soundcraft/issues/1276) |
| **PhotoCraft** | Raster Photo & Graphics Editor | Animation sequences, video layers, deep multi-layer PSB/PSD | [storytold/photocraft#10816](https://github.com/storytold/photocraft/issues/10816) |
| **VectorCraft** | Vector Illustration & Icons | Artboard batches, SVG optimization, vector slices, PDF/EPS | [storytold/vectorcraft#5251](https://github.com/storytold/vectorcraft/issues/5251) |
| **DesignCraft** | Desktop Publishing & Prepress | Print-ready PDF/X-1a/4, prepress bleed packaging, EPUB, IDML | [storytold/designcraft#2156](https://github.com/storytold/designcraft/issues/2156) |
| **PdfCraft** | Acrobat-Class PDF Processing | PDF batch optimization, PDF/A archival flattening, linearizing | [storytold/pdfcraft#5753](https://github.com/storytold/pdfcraft/issues/5753) |
| **FigCraft** | UI/UX & Digital Product Design | Multi-density screen assets (@1x, @2x, @3x WebP/PNG/SVG) | [storytold/figcraft#1880](https://github.com/storytold/figcraft/issues/1880) |
| **CadCraft / SolidCraft** | 3D CAD & Parametric Modeling | 3D mesh export (glTF/GLB, OBJ, STL, PLY, USDZ), blueprints | [storytold/cadcraft#2644](https://github.com/storytold/cadcraft/issues/2644) |
| **DeckCraft** | Presentation & Slide Design | PDF decks, animated video presentations (MP4/ProRes), HTML5 | [storytold/deckcraft#1688](https://github.com/storytold/deckcraft/issues/1688) |
| **WordCraft** | Word Processing & Publishing | Batch book publishing, DOCX to PDF/EPUB, clean web outputs | [storytold/wordcraft#5285](https://github.com/storytold/wordcraft/issues/5285) |
| **TextureCraft** | 3D Texturing & PBR Baking | PBR texture map baking (Albedo, Normal, ORM, BC7 DDS, EXR) | [storytold/texturecraft#161](https://github.com/storytold/texturecraft/issues/161) |
| **MaterialCraft** | Node-Based Procedural Material Design | Procedural PBR texture maps (16-bit TIFF, EXR, PNG) & 3D previews | [storytold/materialcraft#27](https://github.com/storytold/materialcraft/issues/27) |
| **AniCraft** | 2D Animation & Motion Authoring | 2D animation timelines, ProRes 4444 alpha, WebM, GIF, sprite sheets | [storytold/AniCraft#6](https://github.com/storytold/AniCraft/issues/6) |
| **GridCraft** | Spreadsheets & Financial Models| Workbook reporting, data sheet exports, print-ready PDF reports | [storytold/gridcraft#3154](https://github.com/storytold/gridcraft/issues/3154) |
| **LightCraft** | Camera RAW Photo Developing | Batch RAW development (DNG, CR2/3, ARW, NEF) to sRGB/P3 | [storytold/lightcraft#3609](https://github.com/storytold/lightcraft/issues/3609) |
| **Craft Launcher** | Official ArtCraft App Suite Launcher | App catalog registration, installation, and daemon auto-discovery | [storytold/craft-launcher#1199](https://github.com/storytold/craft-launcher/issues/1199) |

### Upstream `craft-libs` & Codec Migration Links
- **`craft-dispatch` Shared IPC Crate:** [storytold/craft-libs#505](https://github.com/storytold/craft-libs/issues/505)
- **Video Codecs Centralization:** [storytold/craft-libs#503](https://github.com/storytold/craft-libs/issues/503) / [storytold/craft-libs#504](https://github.com/storytold/craft-libs/pull/504)
- **Sibling Codec Migration PRs:** [storytold/filmcraft#3941](https://github.com/storytold/filmcraft/pull/3941), [storytold/effectcraft#2814](https://github.com/storytold/effectcraft/pull/2814), [storytold/soundcraft#1275](https://github.com/storytold/soundcraft/pull/1275)
- **Master `encoderCraft` Issue:** [el-j/encoderCraft#1](https://github.com/el-j/encoderCraft/issues/1)
