# Architecture Decision Record 0002: User-Installable Out-of-Process Codec Provider System

## Status
Accepted

## Context
`encoderCraft` is a clean-room, pure-Rust media transcoding engine enforcing `#![forbid(unsafe_code)]` and targeting zero panics and memory safety. While encoderCraft ships with a rich set of built-in pure-Rust codecs harvested across the ArtCraft ecosystem (ProRes, DNx, H.264, AV1, VP9, HEVC, FLAC, Vorbis, Opus, etc.), professional workflows and render farms frequently require specialized, legacy, or proprietary external codec engines (e.g. experimental research codecs, specialized broadcast distribution encoders, or external hardware SDKs) that are not compiled into official releases.

However, loading arbitrary third-party dynamic libraries (`.so`, `.dylib`, `.dll`) in-process in Rust introduces critical risks:
1. Rust lacks a stable cross-compiler C++ / ABI runtime for plugins.
2. An in-process panic, memory corruption, or segmentation fault in a third-party codec terminates the host process, destroying multi-hour render batches and corrupting queue state.
3. In-process dynamic loading breaches the workspace's `#![forbid(unsafe_code)]` quality gate.
4. Distributing or linking proprietary third-party libraries violates license neutrality and clean-room principles.

## Decision
We establish a **local, out-of-process Codec Provider Architecture** operating over a bounded, versioned standard I/O protocol:

1. **Process Boundary & Fault Isolation:**
   - External codec implementations run strictly as separate child processes.
   - Fault isolation is absolute: if a provider worker crashes, runs out of memory, or aborts, `encoderCraft` survives intact, catches the failure as a typed `EncoderError::ProviderWorkerExited`, safely cleans up temporary assets, records the error in the queue journal, and proceeds to the next job.

2. **Explicit Threat Model & Trust Policy:**
   - **Subprocess execution is NOT an OS-level security sandbox.** An enabled provider process executes with the privileges of the active local user.
   - **Zero Execution on Discovery:** The discovery scanner strictly reads and validates declarative metadata manifests (`provider.json`). Under no circumstances is provider code executed during application startup or folder scanning.
   - **Explicit Opt-in Enablement:** All discovered providers are installed in a `Disabled` state by default. A provider can only execute after explicit review and confirmation by the user in the GUI or CLI (`provider.enable` command).
   - **Path Containment & Integrity:** The declared executable must reside within the provider's directory; directory traversal (`..`) or symbolic links escaping the root are rejected.

3. **Built-in Primacy & Zero Silent Shadowing:**
   - Built-in pure-Rust codecs remain the first-class default and are never shadowed or replaced by discovered providers.
   - Standard preset names and generic codec identifiers (`h264`, `hevc`, `prores`) map to built-in codecs unless an explicit provider namespace (`provider_id:codec_id`) is requested.
   - Missing or disabled providers return explicit, typed errors; they never silently fallback to a different codec, preserving reproducible master delivery.

4. **Bounded Streaming Protocol & Memory Limits:**
   - Communication uses length-prefixed binary frames and JSON control messages over standard I/O pipes.
   - Host enforces strict backpressure via bounded ring buffers (maximum 8 uncompressed frames in flight).
   - Whole media files are never buffered in memory.
   - Strict startup, frame decode/encode, and flush timeouts prevent worker deadlocks.

5. **Layering & Command Integration:**
   - Manifest data schemas and validation live in Layer 1 (`crates/core/src/provider.rs`).
   - Provider lifecycle, process supervision, and registry live in Layer 5 (`crates/engine/src/provider/`).
   - All management actions (list, inspect, test, enable, disable, remove) are exposed as `CommandSpec` commands, providing 100% parity across GUI, CLI, loopback IPC, and MCP.

## Consequences
- **Positive:**
  - Complete crash immunity: buggy or hostile external codecs cannot bring down the desktop application or daemon.
  - `#![forbid(unsafe_code)]` is preserved 100% across the codebase.
  - Headless render farms and desktop users share identical management and discovery mechanisms.
  - Clear user trust UX with explicit metadata, license, and author disclosure before execution.
- **Negative:**
  - Inter-process communication adds IPC memory copying overhead for raw uncompressed video frames compared to direct in-memory pointers. (This is mitigated via page-aligned chunked transfers and bounded ring buffers).
