# Architecture Decision Record 0001: Hardware Acceleration Quarantine

## Status
Accepted

## Context
`encoderCraft` targets Adobe Media Encoder parity, requiring blistering performance for high-resolution video exports (4K / 8K, 60–120 FPS). Hardware media engines—such as Apple VideoToolbox, NVIDIA NVENC, AMD AMF, Intel QuickSync, and Linux VA-API—provide 5x to 15x real-time transcoding speeds and conserve CPU power.

However, invoking OS and GPU media libraries inherently requires native platform APIs (C Foreign Function Interfaces / Objective-C bindings). If platform code panics, leaks memory, or invokes undefined behavior, it threatens the stability of the entire render engine.

## Decision
1. **Quarantine Layer (L3):** All platform-specific hardware acceleration bindings and device discovery logic MUST be strictly quarantined inside `crates/platform` (Layer 3).
2. **Pure-Rust Non-Negotiable Workspace Rule:** All crates outside `crates/platform` enforce `#![forbid(unsafe_code)]`.
3. **Fail-Safe Software Fallback:** Every hardware-accelerated pipeline MUST provide an automatic, zero-panic fallback to our pure-Rust software codecs (`filmcraft-h264enc`, `filmcraft-prores`, `effectcraft-hevcenc`, `effectcraft-av1enc`, `effectcraft-vp9enc`).
4. **Boundary Invariants:**
   - Any external platform call that could throw a signal or trap must be caught and converted to a typed `EncoderError::HardwareFailure`.
   - Hardware detection must never block the main thread or panic on headless servers lacking display/GPU access.
   - Resource allocation (GPU surfaces, session contexts) must implement RAII cleanup via `Drop`.

## Consequences
- **Positive:** Maximum rendering throughput on supported Apple Silicon, NVIDIA, and Intel hardware without compromising the memory safety of the core transcode graph, queue engine, or user interface.
- **Negative:** Hardware paths require platform-specific conditional compilation and unit test mocks for CI environments lacking physical GPUs.
