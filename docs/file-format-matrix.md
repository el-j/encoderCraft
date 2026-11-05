# Universal File Format & Codec Matrix

> **Target:** `encoderCraft` (`el-j/encodercraft` → `storytold/encodercraft`)  
> **Prepared for:** ArtCraft Ecosystem & Autonomous Coding Agents  
> **Status:** Specification Document  

---

## 1. Matrix Overview & Capability Classes

`encoderCraft` is engineered as a **universal media transcoding nexus**. While legacy tools like Adobe Media Encoder focus almost exclusively on video and audio, `encoderCraft` operates across **six distinct media dimensions**:

1. **Video & Moving Image** (Web, Broadcast, Cinema, Archival)
2. **Audio & Sound Processing** (Lossless Masters, Streaming Stems, Surround Sound)
3. **Raster Images & Textures** (Photography, HDR, Print, Real-Time GPU, Game Textures)
4. **Vector & 2D Graphics** (Web Vectors, Print Prepress, CAD, UI Assets)
5. **3D Models & Spatial Assets** (VFX Caches, Real-Time glTF, Apple AR USDZ, 3D Printing)
6. **Documents, Presentations & CAD** (Prepress PDF/X, E-Books, Technical Blueprints)

### Capability Legend
- **Decode (D):** Reads, parses, and unpacks the format into memory.
- **Encode (E):** Compresses and serializes internal data models into the format.
- **Mux / Demux (M):** Container multiplexing, track interleaving, and stream splitting.
- **Pass-through / Transmux (T):** Repackages existing compressed bitstreams into different containers without re-encoding (e.g. MOV to MP4, MKV to WebM) in zero time with zero quality loss.
- **Engine Tier:**
  - `Native`: 100% pure-Rust clean-room implementation (no external dependencies, safe, WASM-capable).
  - `Hardware`: Isolated platform hardware acceleration (VideoToolbox on macOS, NVENC/Media Foundation on Windows, VAAPI on Linux).
  - `FFmpeg-Bridge`: Optional subprocess bridge to system FFmpeg binary for rare legacy codecs.

---

## 2. Dimension 1: Video & Moving Image

| Format / Container | File Ext | Codec Profiles Supported | Color Spaces & Bit Depth | Alpha Channel | Engine Tier | Status / Source |
|---|---|---|---|---|---|---|
| **MP4 (ISO BMFF)** | `.mp4`, `.m4v` | H.264 (Baseline, Main, High), HEVC (Main, Main 10), AV1 (Main) | Rec.709, sRGB, Rec.2020 (PQ / HLG HDR10); 8-bit, 10-bit | No (Spec limitation) | `Native` / `Hardware` | Full (D, E, M, T). Derived from `craft-media` + `filmcraft-h264enc` + `effectcraft-hevcenc`. |
| **QuickTime Movie** | `.mov` | Apple ProRes (Proxy, LT, 422, HQ, 4444, 4444 XQ), H.264, HEVC, DNxHD/HR, Motion-JPEG, Animation (RLE), Uncompressed | Rec.709, Display P3, Rec.2020; 8, 10, 12-bit integer | **Yes** (ProRes 4444, Animation, PNG) | `Native` / `Hardware` | Full (D, E, M, T). Native ProRes via `filmcraft-prores`, clean-room muxer in `craft-media`. |
| **WebM** | `.webm` | VP9 (Profile 0, 2), AV1, VP8; Audio: Opus, Vorbis | Rec.709, Rec.2020; 8-bit, 10-bit | **Yes** (VP9 alpha block) | `Native` | Full (D, E, M, T). Matroska EBML engine from `craft-media`, codecs from `effectcraft`. |
| **Matroska Video** | `.mkv` | Any video stream (AV1, HEVC, H.264, VP9, ProRes, DNxHD, FFV1) | Any standard or wide color space; 8, 10, 12, 16-bit | **Yes** | `Native` | Full (D, E, M, T). Native EBML parser/muxer in `craft-media`. |
| **MXF (SMPTE 377M)** | `.mxf` | OP-1a & OP-Atom; Codecs: Avid DNxHD/HR, Apple ProRes, MPEG-2 Broadcast, Uncompressed 10-bit | Rec.709, Rec.2020; 8, 10, 12-bit | No | `Native` | Full (D, E, M). Native MXF partition engine in `filmcraft-mxf`. Broadcast grade. |
| **Animated GIF** | `.gif` | Palette-based LZW frames; 256 colors per frame with local or global adaptive palettes | sRGB 8-bit quantized | **Yes** (1-bit binary transparency) | `Native` | Full (D, E). Adaptive NeuQuant palette quantization from `filmcraft-export`. |
| **Motion JPEG (M-JPEG)** | `.mov`, `.avi` | Intra-frame DCT JPEG slices | Rec.601, Rec.709 YUV 4:2:2; 8-bit | No | `Native` | Full (D, E, M). Native from `filmcraft-export`. |
| **APV Video** | `.apv`, `.mov` | Advanced Professional Video (Profiles 422-10, 444-12) | Wide gamut Rec.2020; 10-bit, 12-bit | **Yes** | `Native` | Full (D, E). Next-gen intra-only video from `filmcraft-apv`. |
| **Image Sequences** | Folder of stills | PNG, TIFF, OpenEXR, DPX, TGA, BMP, JPEG | Full floating point and integer spaces; up to 32-bit float | **Yes** (EXR, TIFF, PNG, TGA) | `Native` | Full (D, E). Automated sequence numbering (`%06d`), custom timecode stamping. |
| **Legacy Video** | `.avi`, `.wmv`, `.flv`, `.mpg` | DivX, Xvid, WMV9, Sorenson Spark, MPEG-1/2 | Standard definition Rec.601; 8-bit | No | `FFmpeg-Bridge` | Fallback (D, E). Executed via subprocess when native codec is unavailable. |

---

## 3. Dimension 2: Audio & Sound

| Format / Container | File Ext | Codec Profiles & Encodings | Sample Rates | Bit Depths & Channels | Loudness & DSP Compliance | Engine Tier | Status / Source |
|---|---|---|---|---|---|---|---|
| **WAV (RIFF / RF64)** | `.wav` | Linear PCM, IEEE 754 Float; Broadcast Wave (`bext`, `ixml` chunks) | 44.1 kHz to 192 kHz | 16, 24, 32-bit int; 32, 64-bit float; 1 to 32 channels | ITU-R BS.1770-4 / EBU R128 compliance, True-Peak limiting | `Native` | Full (D, E). `soundcraft-audio-io`. Supports files >4 GB via RF64. |
| **AIFF / AIFC** | `.aif`, `.aiff` | Big-endian PCM, IEEE Float | 44.1 kHz to 192 kHz | 16, 24, 32-bit int; Mono to 7.1 Surround | Normalization & channel mapping | `Native` | Full (D, E). Native Apple standard format from `soundcraft-audio-io`. |
| **Apple CAF** | `.caf` | Core Audio Format: ALAC, Linear PCM, AAC | 8 kHz to 192 kHz | 16, 24, 32-bit; up to 64 surround channels | Channel mapping matrix | `Native` | Full (D, E). Native 64-bit container from `soundcraft-audio-io`. |
| **FLAC** | `.flac` | Free Lossless Audio Codec; Linear Prediction (LPC) | 44.1 kHz to 96 kHz | 16, 24-bit integer; 1 to 8 channels | ReplayGain metadata, Vorbis comments | `Native` | Full (D, E). Native from `filmcraft-flac` and `soundcraft-audio-io`. |
| **M4A / AAC** | `.m4a`, `.aac` | AAC-LC (Low Complexity), HE-AAC v1/v2 | 32 kHz to 48 kHz | 16-bit; Stereo & 5.1 Surround; 64 to 320 kbps | Target streaming loudness (-14 to -16 LUFS) | `Native` | Full (D, E, M). Native AAC encoder from `filmcraft-aac`, MP4 container from `craft-media`. |
| **Opus** | `.opus`, `.webm` | SILK (speech) and CELT (music) hybrid | 48 kHz native | VBR 6 kbps to 510 kbps; 1 to 255 channels | Low-latency streaming compliance | `Native` | Full (D, E, M). Native from `effectcraft-opusenc`. |
| **MP3 (MPEG-1 Layer 3)** | `.mp3` | Constant (CBR) and Variable (VBR) Bitrate | 32 kHz to 48 kHz | 16-bit; Mono / Stereo; 32 to 320 kbps | ID3v1 and ID3v2.4 tag injection | `Native` / `Bridge` | Full (D via Symphonia, E via native LAME wrapper or FFmpeg bridge). |
| **Ogg Vorbis** | `.ogg` | Ogg Vorbis audio streams | 8 kHz to 96 kHz | VBR quality levels 0.0 to 1.0; Multichannel | Vorbis comment preservation | `Native` | Full (D via Symphonia, E via pure-Rust vorbis). |

---

## 4. Dimension 3: Raster Graphics & Textures

| Format | Extensions | Color Models & Bit Depth | Compression Types | Alpha | Special Features | Status / Source |
|---|---|---|---|---|---|---|
| **PNG** | `.png` | Grayscale, RGB, Indexed; 8-bit, 16-bit per channel | Deflate (Flate2) with heuristic line filters (Sub, Up, Avg, Paeth) | **Yes** (Straight & Premultiplied) | Color management (`iCCP`), sRGB chunks, physical resolution (`pHYs`). | Full (D, E). `photocraft-codecs`. |
| **JPEG** | `.jpg`, `.jpeg` | Grayscale, RGB, CMYK; 8-bit & 12-bit | Baseline DCT, Progressive DCT, Arithmetic Coding (12-bit) | No | EXIF orientation, ICC profiles, Photoshop clipping paths. | Full (D, E). `craft-jpeg` + `photocraft-codecs`. |
| **WebP** | `.webp` | RGB, RGBA; 8-bit | Lossy (VP8 DCT) and Lossless (VP8L predictive) | **Yes** | Ultra-efficient web compression, animated WebP sequences. | Full (D, E). `photocraft-codecs`. |
| **JPEG XL (JXL)** | `.jxl` | RGB, Grayscale; 8-bit, 10-bit, 16-bit, 32-bit float | VarDCT (lossy) and Modular (lossless / responsive) | **Yes** | High Dynamic Range (HDR), wide color gamut, lossless JPEG recompression. | Full (D, E). `photocraft-codecs`. |
| **TIFF / BigTIFF** | `.tif`, `.tiff` | RGB, CMYK, Lab, Grayscale; 8, 16, 32-bit int & float | Uncompressed, LZW, Deflate, PackBits | **Yes** | Multi-page IFDs, BigTIFF (>4 GB), prepress color separation profiles. | Full (D, E). `photocraft-codecs` & `lightcraft-tiff`. |
| **OpenEXR** | `.exr` | RGB, RGBA, Arbitrary Channels; 16-bit half-float, 32-bit float | PIZ, ZIP, ZIPS, RLE, B44, DWAA, DWAB | **Yes** | Multi-part, multi-layer (Beauty, Normal, Depth, Cryptomatte ID channels). | Full (D, E). `photocraft-codecs`. Essential for VFX. |
| **Radiance HDR** | `.hdr`, `.pic` | RGBE (8-bit per channel + shared 8-bit exponent) | Run-length encoded floating point | No | Environment map lighting and image-based lighting (IBL). | Full (D, E). `photocraft-codecs`. |
| **DDS (DirectDraw Surface)** | `.dds` | Compressed GPU formats: BC1, BC2, BC3, BC4, BC5, BC6H (HDR), BC7 | Hardware-decoded block compression | **Yes** | Pre-generated mipmap pyramids, cubemaps, volume textures. | Full (D, E). `photocraft-codecs`. For 3D & games. |
| **DPX (SMPTE 268M)** | `.dpx` | Cinema RGB / YUV; 10-bit, 12-bit, 16-bit packed | Uncompressed planar / interleaved | **Yes** | Film industry standard, SMPTE timecode, keycode, transfer functions. | Full (D, E). `photocraft-codecs`. |
| **HEIF / AVIF** | `.heic`, `.avif` | RGB, YUV 4:2:0, 4:2:2, 4:4:4; 8, 10, 12-bit | HEVC and AV1 intra compression | **Yes** | Next-generation mobile photography standard with depth maps. | Full (D, E). `photocraft-codecs`. |
| **Photoshop PSD / PSB** | `.psd`, `.psb` | RGB, CMYK, Grayscale, Lab; 8, 16, 32-bit float | RLE (PackBits), Raw, Zip | **Yes** | Full layer hierarchy, blend modes, adjustment layers, text data. | Full (D, E). `photocraft-psd`. PSB supports >2 GB files. |
| **Camera RAW** | `.cr2`, `.cr3`, `.nef`, `.arw`, `.dng`, `.raf` | Sensor Bayer Raw, Linear DNG; 12, 14, 16-bit | Lossless JPEG, Deflate, Uncompressed | No | Sensor debayering (AHD, VNG), white balance, camera color calibration. | Full (D via `craft-crw`, `lightcraft-raw`, E to DNG). |

---

## 5. Dimension 4: Vector Graphics & 2D Documents

| Format | Extensions | Standards & Specifications | Color Spaces | Scalability | Export Features | Status / Source |
|---|---|---|---|---|---|---|
| **SVG / SVGZ** | `.svg`, `.svgz` | W3C SVG 1.1 & 2.0; Gzip compressed | sRGB, P3, CSS Color 4 | Infinite resolution independent | Path simplification, text to outlines, embedded fonts, CSS styling. | Full (D, E). `vectorcraft-svg`. |
| **Print-Ready PDF** | `.pdf` | PDF/X-1a, PDF/X-3, PDF/X-4, PDF/A-2b | CMYK, Spot Colors, RGB, DeviceN | Vector paths + high-res raster | Printer's marks (trim, bleed, registration), true font subsetting via `krilla`. | Full (D, E). `designcraft-pdf` + `craft-pdf`. |
| **Encapsulated PostScript** | `.eps` | Adobe PostScript Level 2 & 3 | CMYK, RGB, Grayscale | Vector paths | Embedded TIFF/PICT preview header, EPS bounding box calculation. | Full (D, E). `vectorcraft-eps`. Legacy prepress standard. |
| **Windows Metafile** | `.wmf`, `.emf` | Enhanced Metafile Format | RGB 24-bit | Vector paths + raster records | Interoperability with Microsoft Office and Windows GDI. | Full (D, E). `vectorcraft-metafile`. |
| **Lottie Motion** | `.json` | Bodymovin / Lottie specification | sRGB vectors | Dynamic vector animation | Shape morphing, bezier paths, fills, strokes, keyframed transforms. | Full (D, E). `effectcraft-lottie`. |
| **CAD Drawing Exchange** | `.dxf`, `.dwg` | AutoCAD Release 12 through 2026 | RGB, ACI (AutoCAD Color Index) | Vector lines, arcs, polylines | 2D/3D entities, layers, blocks, dimension styles. | Full (D, E). `cadcraft-dxf` & `cadcraft-dwg`. |

---

## 6. Dimension 5: 3D Models & Spatial Assets

| Format | Extensions | Architecture & Schema | Mesh & Geometry | Materials & Textures | Animation & Rigs | Status / Source |
|---|---|---|---|---|---|---|
| **USD / USDC / USDZ** | `.usd`, `.usdc`, `.usdz` | Pixar Universal Scene Description (Binary & Packaged) | Polygonal meshes, subdivision surfaces, normals, UVs | UsdPreviewSurface PBR materials, packed textures | Skeletal animation, skinning, blend shapes | Full (D, E). `texturecraft-formats`. Native USDZ for iOS AR Quick Look. |
| **glTF 2.0 / GLB** | `.gltf`, `.glb` | Khronos Web 3D Standard (JSON + Binary Chunk) | Vertex buffers, indices, morph targets, vertex colors | Metallic-Roughness PBR, KHR material extensions | Node hierarchies, skeletal rigging, TRS animations | Full (D, E). `texturecraft-formats` & `vectorcraft-three-d`. Web 3D standard. |
| **Autodesk FBX** | `.fbx` | Binary & ASCII interchange | Polygonal geometry, smoothing groups | Lambert, Blinn, Phong, PBR maps | Complex skeletal hierarchies, skinning weights | Full (D, E). `texturecraft-formats`. Game engine exchange. |
| **Wavefront OBJ** | `.obj`, `.mtl` | Classic ASCII polygonal format | Points, lines, faces (`f v/vt/vn`) | Basic material definitions in accompanying `.mtl` | Static geometry only | Full (D, E). `texturecraft-formats`. 100% universal compatibility. |
| **Alembic** | `.abc` | VFX high-performance geometry cache | High-density animated meshes, point clouds | Attribute references | Baked vertex animation sequences | Full (D, E). `texturecraft-formats`. Cinema & VFX standard. |
| **Collada** | `.dae` | ISO/PAS 17506 XML schema | Geometry meshes, visual scenes | Common profile materials | Scene hierarchies, node transforms | Full (D, E). `texturecraft-formats`. Digital asset exchange. |
| **Stereolithography** | `.stl` | 3D Printing standard (ASCII & Binary) | Triangular facets with normal vectors | No color or material | Static geometry only | Full (D, E). `texturecraft-formats`. Additive manufacturing standard. |
| **Stanford Polygon** | `.ply` | 3D scanner format (ASCII & Binary) | Vertices, polygons, arbitrary properties | Per-vertex RGB color, confidence values | Static geometry only | Full (D, E). `texturecraft-formats`. Point cloud & scan data. |

---

## 7. Dimension 6: Documents, Layouts & Office

| Format | Extensions | Primary Use Case | Conversion Capabilities | Source / Engine |
|---|---|---|---|---|
| **PDF Optimization & Linearization** | `.pdf` | Commercial document distribution | Downsamples embedded images, strips orphaned metadata, linearizes for Fast Web View. | `pdfcraft-optimize` |
| **PDF to Editable Office / Web** | `.docx`, `.html`, `.md`, `.rtf` | Document reflow and conversion | Extracts layout, headings, paragraphs, and tables from PDF into Word, HTML5, or Markdown. | `pdfcraft-export` |
| **InDesign Markup (IDML)** | `.idml` | Page layout exchange | Converts multi-page spreads, stories, swatches, and typography between Craft and InDesign. | `designcraft-idml` |
| **EPUB 3** | `.epub` | Digital book publishing | Reflowable and fixed-layout EPUB packages with embedded fonts and CSS. | `designcraft-epub` |
| **OpenXML Word** | `.docx` | Word processing documents | Generates formatted Microsoft Word files with styles and tables. | `wordcraft-docx` |
| **OpenXML Presentation** | `.pptx` | Slideshows and pitch decks | Generates PowerPoint slides, vector graphics, and master layouts. | `deckcraft-pptx` |
| **OpenXML Spreadsheet** | `.xlsx`, `.csv` | Tabular data and calculation | Generates Excel workbooks and comma/tab-separated values. | `gridcraft-xlsx` |
| **Plotter HPGL** | `.plt`, `.hpgl` | Architectural blueprint plotters | Converts vector CAD paths to HPGL pen-plotter instruction streams. | `cadcraft-io` |

---

## 8. Summary of Unique Transcoding Capabilities

Because `encoderCraft` bridges all six dimensions, users can execute transcode operations that previously required four separate commercial applications:

1. **3D to Web Showcase:** Ingest an Autodesk FBX or Alembic VFX cache → generate an optimized `.glb` for web, a `.usdz` for iOS AR, a 4K ProRes turnaround video, and an animated WebP preview in one click.
2. **Video Sequence to Multi-Screen Delivery:** Ingest a 4K ProRes master → output an MXF OP-1a broadcast master (EBU R128 -23 LUFS), a YouTube 4K MP4 (-14 LUFS), a TikTok 9:16 vertical MP4 (-14 LUFS), and a 1080p animated GIF teaser.
3. **Print Publication to Multi-Channel:** Ingest a DesignCraft book → produce a print-ready PDF/X-4 with bleed, a reflowable EPUB 3, an optimized web PDF, and 300 DPI high-resolution CMYK TIFF proofs.
4. **Game Texture Baking:** Ingest high-res 32-bit floating point EXR textures → generate packed BC7 DDS textures for Unreal/DirectX, compressed WebP previews for web asset stores, and 16-bit PNG heightmaps.

## 9. Video Codec Capability Table (generated)

Generated from `crates/video-codecs/src/capabilities.rs`; `encodercraft-cli --formats` prints the same data. "Decode verified" means a conformance test decodes a stream made by an independent encoder (`crates/source/tests/decoders.rs`) and checks size, frame count and PSNR. AV1 has a pure-Rust decoder (`filmcraft-av1`).

| Codec | Decode | Encode | Decode verified | Notes |
|---|---|---|---|---|
| H.264 | yes | yes | yes | 8-bit 4:2:0 progressive decode (filmcraft-h264) |
| HEVC | yes | yes | yes | Main / Main 10 decode (filmcraft-hevc) |
| VP8 | yes | no | yes | decode only (filmcraft-vp8) |
| VP9 | yes | yes | yes | filmcraft-vp9 |
| AV1 | yes | yes | yes | pure-Rust decoder (filmcraft-av1) |
| MPEG-2 | yes | no | yes | MPEG-1/2 in PS and TS, decode only (filmcraft-mpeg2v) |
| ProRes | yes | yes | yes | filmcraft-prores |
| DNxHD/DNxHR | yes | yes | no | filmcraft-dnx; round-trips in video-codecs tests only |
| Motion JPEG | no | yes | - | encode only |
| GIF | no | yes | - | animated GIF89a encode only |

## 10. Container x Codec Matrix and Encoder Parameters

Generated by `encodercraft-cli --containers`. The pipeline rejects any other pairing with `UnsupportedCodec` before writing a file, and the UI only offers presets whose codec the container accepts. Every pairing is proven by an encode-decode test in `crates/pipeline/tests/presets_conformance.rs`.

| Container | H.264 | HEVC | AV1 | VP9 | ProRes | DNxHR | GIF |
|---|---|---|---|---|---|---|---|
| MP4 | yes | yes | yes | yes | - | - | - |
| MOV | yes | yes | - | - | yes | yes | - |
| MKV | yes | yes | yes | yes | - | - | - |
| WebM | - | - | yes | yes | - | - | - |
| MXF OP1a | yes | - | - | - | yes | yes | - |
| MXF OP-Atom | yes | - | - | - | yes | yes | - |
| GIF | - | - | - | - | - | - | yes |

Preset parameters (`VideoEncodeSettings`: codec, keyframe interval, profile, B-frames, rate mode, colour tag) are applied, or rejected with an error, never silently ignored:

| Codec | Rate mode | Keyframe interval | Profile | B-frames | HDR colour tags |
|---|---|---|---|---|---|
| H.264 | CBR, VBR, constant quality | yes | baseline, main, high | 0-3 | BT.709, PQ, HLG (VUI) |
| HEVC | bitrate, constant quality | yes | no | no | no |
| AV1 | bitrate, constant quality | yes | no | no | no |
| VP9 | bitrate, quality | yes | no | no | no |
| ProRes | intra | n/a | proxy, lt, 422, hq (4:2:2 only) | n/a | no |
| DNxHR | intra | n/a | lb, sq, hq, hqx | n/a | no |

Known upstream gaps: HEVC, AV1 and VP9 encoders expose no B-frames, profiles or HDR signalling; H.264 rate control is single-pass; ProRes 4444 is not encodable. MP4 output is written faststart (`moov` before `mdat`); MOV is not. MXF stores H.264 as Annex B.

## 11. Streaming, memory limits & measurements

Video encoding uses bounded decode and convert queues, a reusable output-frame buffer, and an ordered encode/mux stage. The default frame-queue budget is 256 MiB, with at most eight queued frames per boundary. The budget accounts for stage-local buffers, the recycled output frame, and the temporary linear-float conversion buffer used for high-bit-depth source frames; a source whose minimum working set exceeds the configured budget is rejected before streaming. Cancellation and stage failures discard the staged output and preserve any existing destination.

Source audio is decoded and encoded in 10-second windows. Loudness normalization still requires a whole-track pass; its default 1 GiB limit includes the source-channel and normalized stereo buffers. The CLI exposes both limits as `--frame-memory-mib` and `--normalization-memory-mib`. These settings bound pipeline-managed buffers, not the source decoder's cache: the upstream FilmCraft GOP cache currently uses a process-wide 1 GiB frame budget that encoderCraft cannot configure.

Release-mode measurements on the same 1920x1080 H.264/AAC source family using `YouTube 1080p Full HD`:

| Input duration | Input size | Wall time | Peak RSS |
|---:|---:|---:|---:|
| 20.07 s | 76 MiB | 14.70 s | 876 MB |
| 60.00 s | 217 MiB | 47.69 s | 940 MB |

Measured with `/usr/bin/time -l target/release/encodercraft-cli encode ...` on macOS. The non-linear RSS increase reflects the decoder's GOP cache and fixed stage working set; the complete source is not accumulated in memory. These numbers are measurements, not resource guarantees, and decoder-cache behavior remains upstream-controlled.
