# User-Installable Codec Provider System

This document specifies the architecture, manifest schema, wire protocol, CLI/GUI workflows, and authoring guidelines for user-installable codec providers in **encoderCraft**.

---

## 1. Architectural Model & Threat Boundaries (ADR 0002)

`encoderCraft` is a pure-Rust, `#![forbid(unsafe_code)]` transcoding engine designed never to crash. Users frequently need exotic or specialized codecs (e.g. proprietary hardware drivers, experimental codecs, or licensed broadcast formats) not bundled in standard releases.

### Core Architectural Guarantees:
1. **Out-of-Process Isolation:** Providers execute in separate supervised child processes communicating with `encoderCraft` exclusively via standard pipes (`stdin`, `stdout`, `stderr`).
2. **Crash & Fault Isolation:** If a provider worker panics, segfaults, exits with an error code, or hangs, the parent process survives unaffected. Buffered `stderr` is captured up to 64 KB and mapped directly to typed `EncoderError::Io` diagnostics.
3. **Built-In Primacy (Non-Shadowing):** Built-in first-party encoders (`H.264`, `HEVC`, `ProRes`, `DNxHR`, `AV1`, `VP9`, `GIF`, `MJPEG`) **always take absolute precedence**. An external provider can never shadow or hijack a built-in codec identifier unless the caller explicitly namespaces it (e.g., `my-provider:h264`).
4. **Explicit Trust & Enablement:** Discovered providers remain **disabled by default**. They will not be executed unless the user explicitly reviews and enables them in the CLI or GUI.
5. **Zero-Execution Discovery:** Installing or scanning providers only parses the declarative `provider.json` manifest. No binaries or scripts are executed during app launch or directory scans.

---

## 2. Directory Layout & Discovery

`encoderCraft` discovers providers in standard system and user configuration directories:

- **Linux / BSD:** `~/.config/encodercraft/providers/<provider_id>/`
- **macOS:** `~/Library/Application Support/encoderCraft/providers/<provider_id>/`
- **Windows:** `%APPDATA%\encoderCraft\providers\<provider_id>\`

A provider package must be a self-contained directory containing:
```
providers/
  org.svt.av1/
    provider.json          # Declarative metadata manifest (<= 1 MB)
    bin/
      svt-av1-worker       # Executable binary (relative path, no '..')
    README.md
    LICENSE
```

---

## 3. Manifest Specification (`provider.json`)

The manifest file must be named `provider.json`, encoded in UTF-8, and strictly capped at 1 MB.

```json
{
  "id": "org.svt.av1",
  "name": "SVT-AV1 Community Codec Provider",
  "version": "1.8.0",
  "protocolVersion": 1,
  "encodercraftVersion": ">=0.1.0-alpha.2",
  "description": "High-performance Scalable Video Technology AV1 encoder",
  "author": "Alliance for Open Media",
  "license": "BSD-3-Clause-Clear",
  "homepage": "https://gitlab.com/AOMediaCodec/SVT-AV1",
  "executable": "bin/svt-av1-worker",
  "operations": [
    {
      "operationKind": "videoEncode",
      "codecId": "svt-av1",
      "displayName": "SVT-AV1 Video Encoder",
      "aliases": ["svtav1", "av1-svt"],
      "profiles": ["main", "high"],
      "pixelFormats": ["yuv420p", "yuv420p10"],
      "containers": ["mp4", "mkv", "webm"],
      "parameters": [
        {
          "name": "preset",
          "label": "Encoding Speed Preset",
          "paramType": {
            "integer": {
              "min": 0,
              "max": 13,
              "default": 8
            }
          },
          "description": "Speed vs compression tradeoff (0 is highest quality, 13 is fastest)"
        }
      ]
    }
  ]
}
```

### Operation Kinds:
- `videoEncode`: Normalized video frames -> compressed video bitstream packets.
- `audioEncode`: Interleaved `f32` PCM buffers -> compressed audio frames.
- `videoDecode`: Compressed video packets -> normalized uncompressed video frames.
- `audioDecode`: Compressed audio packets -> interleaved `f32` PCM buffers.

---

## 4. Binary Wire Protocol (Version 1)

Communication flows over anonymous OS pipes (`stdin`/`stdout`).

### Framing:
Every message starts with a 4-byte big-endian unsigned integer indicating the length of the JSON control header:
```
+--------------------------+---------------------------+------------------------+
| 4 bytes: JSON length (N) | N bytes: JSON UTF-8 text | Optional binary payload|
+--------------------------+---------------------------+------------------------+
```
Limits:
- Control JSON: maximum 64 KB (`65,536` bytes).
- Binary payload: maximum 64 MB (`67,108,864` bytes).

### Host -> Worker Messages:
1. **`Hello` (Handshake):**
   ```json
   {
     "type": "hello",
     "protocolVersion": 1,
     "codecId": "svt-av1",
     "operationKind": "videoEncode",
     "width": 1920,
     "height": 1080,
     "fps": { "num": 30, "den": 1 },
     "pixelFormat": "yuv420p",
     "bitrateKbps": 5000,
     "parameters": { "preset": 8 }
   }
   ```
2. **`EncodeFrame` (Video Frame Delivery):**
   ```json
   {
     "type": "encodeFrame",
     "pts": 0,
     "dts": 0,
     "durationTicks": 3000,
     "payloadBytes": 3110400
   }
   ```
   Followed immediately by raw contiguous plane bytes (`Y`, `U`, `V`).
3. **`EncodeAudio` (Audio Sample Delivery):**
   ```json
   {
     "type": "encodeAudio",
     "pts": 0,
     "frames": 1024,
     "payloadBytes": 8192
   }
   ```
   Followed immediately by raw little-endian `f32` interleaved PCM samples.
4. **`DecodePacket` (Bitstream Decoding):**
   ```json
   {
     "type": "decodePacket",
     "pts": 0,
     "dts": 0,
     "payloadBytes": 4210
   }
   ```
5. **`Flush`:**
   Flushes all pending buffered frames.
6. **`Cancel`:**
   Cancels immediately and terminates.

### Worker -> Host Messages:
1. **`HelloAck`:**
   ```json
   {
     "type": "helloAck",
     "protocolVersion": 1,
     "codecId": "svt-av1",
     "configRecordBase64": "0164001F...",
     "fourcc": "av01"
   }
   ```
2. **`Packet` (Compressed Video Packet):**
   ```json
   {
     "type": "packet",
     "pts": 0,
     "dts": 0,
     "isKeyframe": true,
     "durationTicks": 3000,
     "payloadBytes": 12450
   }
   ```
3. **`AudioChunk` (Compressed Audio Chunk):**
   ```json
   {
     "type": "audioChunk",
     "pts": 0,
     "payloadBytes": 340
   }
   ```
4. **`DecodedFrame`:**
   Metadata header followed by decoded uncompressed plane bytes.
5. **`DecodedAudio`:**
   Metadata header followed by decoded `f32` interleaved PCM samples.
6. **`FlushAck`:**
   Signals that flushing is complete.
7. **`Error`:**
   ```json
   {
     "type": "error",
     "code": "OUT_OF_MEMORY",
     "message": "Encoder internal allocation failed"
   }
   ```

---

## 5. Command Interface & Headless CLI

Codec providers are managed using standard `CommandSpec` commands:

- `provider.list`: List installed providers and runtime status.
- `provider.inspect`: Inspect full manifest, licensing, and capabilities.
- `provider.enable`: Enable a provider (persists to user preferences).
- `provider.disable`: Disable a provider.
- `provider.test`: Execute an out-of-process protocol handshake conformance ping.

### CLI Usage:
```bash
# List discovered providers
encodercraft-cli provider list

# Inspect provider metadata
encodercraft-cli provider inspect org.svt.av1

# Test provider protocol conformance
encodercraft-cli provider test org.svt.av1

# Enable provider for transcode resolution
encodercraft-cli provider enable org.svt.av1

# Encode using provider codec
encodercraft-cli encode input.mov -o output.mp4 -p "SVT-AV1 4K"
```

---

## 6. Desktop GUI Integration

In the desktop interface:
1. Open **Preferences** (`Cmd+,` / `Ctrl+,` or **Edit > Preferences...**).
2. Select the **Codec Providers** tab.
3. Review installed providers, license attributions, and trust warnings.
4. Click **Enable** or **Disable** to control active resolution.
