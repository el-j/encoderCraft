# User-Installable Codec Provider System — Planning Proposal

> **Status:** Proposed; planning only. No plugin runtime is implemented.
> **Target milestone:** M46, following the active M42–M45 work. GitHub still marks M39–M41 open, although this repository documents their tasks as complete.
> **Scope:** Let users add compatible codec implementations that are not compiled into an encoderCraft release.

## 1. Current State and Gap

The current video path is built into the application:

1. `VideoCodecKind` is a closed enum and parses known codec names and FourCCs.
2. `CODEC_CAPABILITIES` and `codecs_for_container` list compiled-in capabilities and container compatibility.
3. `create_video_encoder_with` matches each enum variant to an in-process encoder.
4. The pipeline calls that factory directly.

This is a sound approach for codecs shipped with encoderCraft, but an installed codec cannot currently register itself at runtime. The codec harvest and `craft-libs` migration plans centralize and reuse source code at build time; they do not define a user plugin API, runtime discovery, installation, or isolation.

There is an adjacent, narrower idea in [the format matrix](./file-format-matrix.md), the implementation milestone risk table, and [plan-draft-00](./plan-draft-00.md): an optional subprocess bridge to a system FFmpeg executable for formats without native support. That is a specific backend/fallback proposal, not a general codec-provider framework: it does not define a provider manifest, multi-provider capability registry, lifecycle/trust UI, or a protocol reusable by other codec vendors. M46 should reconcile with that proposal rather than silently replace it.

## 2. Recommendation

Build a **local, out-of-process codec provider API**, not a Rust dynamic-library loader.

- Built-in Rust codecs remain the preferred, first-party path and retain their current in-process performance.
- A user-installed provider is a separate executable communicating through a versioned protocol. The application discovers its manifest but does not execute it until the user explicitly enables and invokes it.
- The engine remains responsible for media jobs, stream scheduling, and muxing. A provider supplies only declared codec operations and elementary stream data; it does not replace arbitrary parts of the pipeline.
- Start with one video encoder provider as a vertical slice, then extend the same protocol to audio encoders and decoders only after the video contract proves out.
- M46 v1 is local installation only: no in-app marketplace, automatic downloads/updates, or bundled third-party provider binaries.

### Why not load dynamic libraries in-process?

Rust does not offer a stable cross-version plugin ABI. Loading arbitrary libraries would also undermine the workspace's `unsafe_code = "forbid"` boundary, make third-party panics and memory corruption process-fatal, and complicate platform packaging. A subprocess boundary avoids those problems and allows provider failure to be reported as a failed operation without terminating the queue worker or application. It is process isolation, **not** a complete OS sandbox: an enabled executable runs with the user's permissions.

## 3. Proposed User Experience

- Users install a provider by placing its executable and manifest in a documented per-user provider directory (or by selecting a local provider package in the GUI).
- Discovery validates and lists metadata without running the executable.
- The user reviews the provider identity, version, declared codec operations, executable path, and license information, then explicitly enables it.
- The CLI and GUI can list, inspect, test, enable, and disable providers. Removing or disabling a provider never changes built-in codecs or existing job definitions.
- Provider-specific codec options are presented only when a provider declares them. Unsupported settings are rejected, not silently ignored.
- A queued job records the provider identity/version and codec settings used. If that provider later disappears, the job reports a clear unavailable-provider error rather than silently selecting a different codec.

## 4. Proposed Architecture and Boundaries

### Provider manifest and discovery

Use a versioned, bounded manifest with at least:

- Stable provider ID, display name, provider version, protocol version, and supported encoderCraft version range.
- Relative executable path, declared codec IDs and aliases, media domains, encode/decode operations, profiles, parameter schemas, and supported elementary-stream formats.
- Container compatibility declarations, validated against encoderCraft's muxer capabilities rather than trusted as authority.
- License identifier and attribution/source information for display and review.
- Optional integrity metadata for diagnostics; integrity metadata alone is not proof of trust.

Discovery must reject malformed, oversized, incompatible, duplicate, or path-escaping manifests with actionable diagnostics. Discovery must never run provider code. Initial installation should accept local files only, and the application must not install a provider by extracting untrusted archives without strict path and size checks.

### Provider registry and selection

Introduce a runtime provider registry alongside—not in place of—the built-in codec registry. Resolve requests using an explicit provider ID or an unambiguous codec capability match. Do not silently shadow a built-in codec, select a different provider when one is requested, or advertise an operation that the selected output container cannot mux.

The registry is the source for runtime capability presentation. Keep generated format documentation and CLI output consistent with it, while preserving stable built-in behavior when no providers are installed.

### Worker protocol and lifecycle

Specify a versioned control protocol and a bounded binary media transport. Negotiate protocol and operation capabilities before processing; stream frames/packets with timestamps, format descriptors, and explicit end/error messages. Enforce maximum message/frame sizes, bounded buffering and backpressure, cancellation, startup/operation timeouts, and clean shutdown. Do not buffer whole media files in memory.

The host supervises each worker, captures bounded diagnostic output, detects exit/protocol violations, and maps those failures to typed job errors. A failed provider job must not corrupt queue state or prevent unrelated jobs from running.

### Trust, security, and licensing

- Never auto-execute on discovery, and never enable a provider without explicit user action.
- Show the canonical executable path and declared publisher/license metadata before enabling.
- Resolve executable paths under the provider installation root; reject traversal and unexpected executable replacement where practical.
- Document that enabled providers execute with the current user's permissions; do not imply that subprocess execution is a security sandbox.
- Do not fetch or update code automatically in v1. A future catalog would need a separate signing, review, revocation, and update design.
- Keep provider binaries out of encoderCraft releases. Review license compatibility before first-party distribution or listing; metadata supplied by a provider is not legal verification.

## 5. Milestone M46 — User-Installable Codec Providers

**Objective:** Deliver a safe, explicit, local extension path for codec implementations absent from a release, without weakening built-in codec reliability or layering. Tracked under GitHub [milestone M46](https://github.com/el-j/encoderCraft/milestone/24).

### Planned tasks

1. **M46.1 — Approve the provider boundary and threat model ([#251](https://github.com/el-j/encoderCraft/issues/251))**
   - Write an ADR comparing in-process libraries, command-line adapters, and a dedicated worker protocol.
   - Decide protocol transport, supported platform/version policy, provider trust UX, job persistence behavior, and the initial codec operation scope.
   - Confirm that built-in Rust codecs remain the default and that user providers are not treated as sandboxed.

2. **M46.2 — Define and validate the provider manifest ([#252](https://github.com/el-j/encoderCraft/issues/252))**
   - Specify schema versioning, IDs, executable layout, API compatibility, codec aliases, encode/decode capabilities, profiles, parameters, container declarations, and license/attribution fields.
   - Add bounded parsing, compatibility checks, duplicate detection, path containment, and actionable validation diagnostics.
   - Document per-user install locations and a minimal provider authoring example.

3. **M46.3 — Add runtime capability registry and deterministic selection ([#253](https://github.com/el-j/encoderCraft/issues/253))**
   - Bridge the closed built-in codec list with runtime provider descriptors without breaking built-in `VideoCodecKind` callers.
   - Enforce explicit provider selection, collision handling, capability negotiation, and muxer/container compatibility.
   - Ensure format listings and presets distinguish built-in and external providers and do not claim unsupported operations.

4. **M46.4 — Specify worker protocol and provider conformance harness ([#254](https://github.com/el-j/encoderCraft/issues/254))**
   - Define version negotiation, control/error messages, media descriptors, timestamp semantics, bounded binary streaming, backpressure, cancellation, and flush behavior.
   - Provide a test worker and conformance harness so third-party authors can validate a provider without the GUI.
   - Fuzz and test malformed messages, oversized payloads, and incompatible protocol versions.

5. **M46.5 — Implement worker supervision and failure handling ([#255](https://github.com/el-j/encoderCraft/issues/255))**
   - Start workers only for enabled operations; supervise startup, progress, stderr/log limits, timeouts, cancellation, exit codes, and protocol faults.
   - Apply bounded queues and resource limits supported consistently by the host; surface errors through the existing typed job/queue error paths.
   - Prove that a worker crash leaves the application alive and the job journal recoverable, and does not stop subsequent jobs.

6. **M46.6 — Integrate one external video encoder end to end ([#256](https://github.com/el-j/encoderCraft/issues/256))**
   - Add a sample provider that consumes normalized video frames and returns timestamped encoded packets/configuration records.
   - Connect provider output to existing muxer compatibility checks and the regular pipeline; verify output metadata and playback with independent fixtures.
   - Keep built-in encoder selection and output byte behavior unchanged when the provider is absent or disabled.

7. **M46.7 — Extend the contract to audio and decoding ([#257](https://github.com/el-j/encoderCraft/issues/257))**
   - Specify and implement audio encode plus video/audio decode operations using the same lifecycle, bounded transport, and typed failures.
   - Validate that demuxing, decoder output, resampling, encoder input, and container muxing agree on sample/frame formats and timestamps.
   - Gate each operation independently so a provider may expose only the capabilities it actually implements.

8. **M46.8 — Expose provider management through commands, CLI, and GUI ([#258](https://github.com/el-j/encoderCraft/issues/258))**
   - Add `CommandSpec`-backed list/inspect/test/enable/disable/remove workflows, with CLI parity for headless hosts.
   - Display trust, path, version, protocol compatibility, capabilities, and diagnostics; require confirmation before enabling executable code.
   - Preserve user settings and queued-job provenance when a provider is disabled, upgraded, or removed.

9. **M46.9 — Complete conformance, packaging, and operator documentation ([#259](https://github.com/el-j/encoderCraft/issues/259))**
   - Add regression, fuzz, protocol, lifecycle, queue-recovery, cross-platform, and built-in compatibility tests.
   - Verify provider discovery and operation from desktop and headless CLI distributions without bundling sample provider binaries.
   - Publish provider authoring/install/removal guidance, compatibility policy, threat model, license responsibilities, and troubleshooting steps.

### Completion gate

M46 is complete when a user can install a compatible local provider that is not in the release, explicitly review and enable it, select a declared codec operation through the normal GUI/CLI job path, and successfully encode through an existing compatible muxer. Invalid manifests, incompatible versions, unsupported settings, cancellation, oversized messages, and worker crashes must produce explicit errors without panicking, hanging, exhausting unbounded memory, corrupting the queue journal, or changing the built-in codec path.

### Deliberately out of scope for M46 v1

- In-process Rust `cdylib` plugins or a stable Rust ABI.
- An online plugin store, automatic code download/update, or provider auto-enable.
- Providers that replace muxers, filters, queue scheduling, commands, or arbitrary engine components.
- A promise of OS-level sandboxing or isolation from malicious code.
- Bundling codecs with unclear or incompatible licenses.
- Automatic fallback to another provider when a requested provider is missing or fails.

## 6. Risks and Open Decisions

| Question / risk | Planning response |
|---|---|
| Does subprocess transport cost too much for high-resolution frames? | Benchmark the bounded transport with representative HD/4K workloads before finalizing the wire format; preserve a future optimization path without making an in-process ABI the default. |
| Can the same protocol cover all media domains? | Keep capabilities domain-specific and operation-scoped; prove video encode first, then add audio and decoding with explicit format contracts. |
| Does process isolation make a provider safe? | No. An enabled process runs with user permissions. Communicate this clearly and never auto-execute discovered code. |
| Could a provider claim unsupported container or profile support? | Treat declarations as input, validate against host muxers, and require conformance tests; fail closed on mismatches. |
| Could provider updates invalidate queued jobs? | Persist provider ID/version and settings; report unavailable or incompatible providers rather than silently substituting another implementation. |
| Can encoderCraft vouch for a third-party provider's license? | No. Display declared metadata, do not bundle providers in v1, and require legal review before first-party distribution. |

## 7. Relationship to Existing Plans

- [The ArtCraft exporters survey](./artcraft-exporters-survey.md) and [codec migration plan](./craft-libs-codecs-migration-plan.md) cover build-time reuse and centralization of source codecs; M46 addresses runtime user extensibility.
- [The format matrix](./file-format-matrix.md) and [plan-draft-00](./plan-draft-00.md) describe a specific optional system-FFmpeg bridge. M46 should decide whether that backend becomes one provider adapter or remains a separate fallback; it is not itself a plugin system.
- [The architecture specification](./architecture.md) remains the description of the current shipped architecture. This proposal does not claim that a runtime plugin layer already exists.
- Open GitHub milestones M42–M45 and their issues remain independent work; M46 follows them rather than absorbing their existing conversion and UI scope. GitHub also marks M39–M41 open even though their local milestone entries are checked off.
