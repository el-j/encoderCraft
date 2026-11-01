Plan: ffmpeg encoding backend for EffectCraft + FilmCraft
How encoding works today
FilmCraft (crates/export, ~9.7k lines)

VideoEncoder trait (lib.rs): sample_entry(), timescale(), encode(&EncoderFrame) -> Vec<EncodedPacket> (data, key, duration, composition_offset), flush(), media_start(), pass_stats().
AudioEncoder trait: sample_entry(), encode(planar f32), flush(), priming(), frame_size().
Encoder selection is a factory registry: register_encoder / register_format_probe / register_hdr_probe. Registered factories are tried before built-ins.
Built-in video factories: h264_factory (filmcraft-h264enc), prores_factory, dnx_factory, apv_factory, mjpeg_factory. Audio: aac_factory (+ built-in PCM/FLAC paths).
HEVC/AV1 have no built-in encoder — they only exist when a platform hardware encoder registers (crates/platform: VAAPI, VideoToolbox, Media Foundation, NVENC). This is the one unsafe-allowed crate (ADR 0001); the rest of the workspace is unsafe_code = "forbid".
Muxing is FilmCraft's own: filmcraft-isobmff (MP4/MOV incl. edit lists, udta metadata, HDR static metadata, timecode) and filmcraft-mxf (OP-1a/OP-Atom, start timecode). Captions burn-in happens at the frame-pipeline level; caption sidecars are separate files.
ffmpeg exists only as a test oracle (crates/testkit/src/oracle.rs finds ffmpeg/ffprobe on PATH).
EffectCraft (crates/export, ~2.6k lines)

Simpler VideoEncoder trait in encode.rs: sample_entry(), encode(rgba, index) -> Vec<Packet> (data, key, cto), flush(), media_start().
Dispatch is per OutputFormat in lib.rs: encode::movie for H264/HEVC/AV1/ProRes/Uncompressed → MP4/MOV via filmcraft-isobmff; webm::webm for VP9/AV1 + Opus; uncompressed for raw MOV; still sequences, GIF, WAV/AIFF.
Encoders: FilmCraft's h264enc/prores/aac + its own hevcenc, av1enc, vp9enc, opusenc. filmcraft-* crates are pinned git deps (rev 2c5ba7a…), with a commented [patch] block for local checkouts.
Shared constraints

"Pure Rust" is a stated project value; browser/WASM is a supported target (cfg!(target_arch = "wasm32") appears in export).
unsafe_code = "forbid" everywhere except crates/platform (FilmCraft) — ADR 0001.
AGENTS.md rule 0: never crash, no unwrap/panic, all input hostile.
Options
A. ffmpeg CLI subprocess (recommended)
Spawn ffmpeg once per export; pipe rendered frames on stdin (-f rawvideo -pix_fmt rgba), audio as a second input (-f f32le on pipe:3, or a temp WAV/f32le file on Windows-safe paths); ffmpeg encodes and muxes straight to the output file; -progress pipe: for progress, child kill for cancel.

Pro: zero unsafe, zero linking, zero GPL/LGPL build-time surface, works with whatever ffmpeg the user has; one integration point covers every movie format incl. ones with no native encoder.
Con: bypasses the native muxers (edit lists, udta metadata, HDR static metadata, MXF timecode must be re-expressed as ffmpeg args); runtime dependency on an ffmpeg binary; not usable on WASM; output is not bit-identical with native encoders.
B. Linked libav* via ffmpeg-next / ffmpeg-sys-next
Implement VideoEncoder/AudioEncoder over libavcodec (frames in, AVPacket→EncodedPacket out — the traits map almost 1:1). Native muxers stay.

Pro: minimal disruption to the export architecture; keeps own muxers, edit lists, metadata, MXF paths; per-codec so native and ffmpeg encoders can mix.
Con: unsafe required — must live in crates/platform or a new exempted crate; ffmpeg dev libraries needed at build and runtime; LGPL (GPL if using libx264/libx265 builds); larger effort; WASM dead end anyway.
C. Hybrid
Feature-gated ffmpeg cargo feature + register_encoder factories backed by FFI, or a Backend::Ffmpeg job path backed by the CLI. Native stays default; ffmpeg is opt-in per export or per build.

Recommended shape
Option A as an opt-in backend ("switch the encoding backend" as a setting, not a rewrite):

FilmCraft: add EncodeBackend { Native, Ffmpeg } on ExportSettings (serde, default Native). In job.rs, when Ffmpeg and format is a movie format, run a new ffmpeg_out.rs instead of the Exporter loop: same Pipeline frame source, but frames stream into the ffmpeg child instead of VideoEncoder+muxer. Map Format/Multiplexer → codec+muxer args:
H264→libx264, Hevc→libx265 (or hevc_videotoolbox/hevc_vaapi per platform), Av1→libsvtav1/libaom-av1, ProRes→prores_ks, DnxHr→dnxhd, Apv→apv (ffmpeg 8+), Mjpeg→mjpeg, AAC→aac, PCM→pcm_s16le/s24le/s32le/f32le, FLAC→flac, Opus→libopus; mp4/mov/webm/mkv/mxf/wav/aiff muxers.
Carry over: -timecode, -metadata (ExportMetadata.udta equivalent), burn-in captions (already in the frame pipeline), HDR (BT.2020/PQ color tags + mastering metadata via -x265-params or mov write_colr paths).
Image sequences, GIF, sidecars stay native.
Discovery: reuse the testkit-style PATH search; Format::available/register_format_probe-style probe so the UI can gray out ffmpeg-dependent choices when no binary exists.
EffectCraft: same pattern — EncodeBackend on the render-queue output module; ffmpeg_out(cx, comp, w, h, st) alongside encode::movie/webm::webm; same format→codec map (H264/HEVC/AV1→mp4, VP9→libvpx-vp9+libopus→webm, ProRes→mov). Since its filmcraft-* crates are pinned git deps, a shared implementation could live in FilmCraft (filmcraft-export or a small filmcraft-ffmpeg crate) and be consumed by EffectCraft via the git dep — otherwise the module is duplicated.
Progress/cancel: -progress pipe:2 parsed into the existing Report/status channel; cancel = kill child + delete partial file.
Tests: oracle-gated like testkit (skip when no ffmpeg); round-trip a few frames each format; verify file opens, frame count, duration, A/V sync via ffprobe.
WASM: backend setting forced to Native; Ffmpeg returns a clean "not supported on this platform" ExportError.
Open questions for you
Replace or toggle? Keep native encoders and add ffmpeg as a selectable backend (my assumption), or make ffmpeg the only movie-encode path?
CLI vs linked? I recommend CLI subprocess (Option A). If you want bit-macro control and native muxing, that's FFI (Option B) — bigger change, needs an unsafe exemption beyond crates/platform.
ffmpeg binary: expect it on PATH, or bundle/download one (sidecar-style)? Licensing: distributing an ffmpeg binary built with x264/x265 is GPL — fine to invoke, more care needed to ship.
Scope: all movie formats through ffmpeg, or keep some native (e.g. ProRes/MXF mastering paths stay native, ffmpeg only for H.264/HEVC/AV1/WebM)?
Target repos: work lands on storytold/EffectCraft + storytold/FilmCraft directly — do you have push rights, or fork → PR? And should the shared piece live in FilmCraft so EffectCraft picks it up through the existing git dep?