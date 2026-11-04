//! Job specifications, output targets, and queue status data structures.
//!
//! Matches the inter-app dispatch protocol specification (docs/inter-app-dispatch-protocol.md).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::EncoderError;
use crate::timecode::Rational64;

/// Execution priority of a queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JobPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

/// Execution start policy upon submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartPolicy {
    /// Begins encoding as soon as a worker becomes available.
    #[default]
    AutoStart,
    /// Adds to queue in a paused state until explicitly started by the user.
    Paused,
}

/// Current status of a job or output target in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JobStatus {
    #[default]
    Queued,
    Processing,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

/// Identifying metadata about the submitting client application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub app_name: String,
    pub app_version: String,
    pub pid: u32,
    #[serde(default)]
    pub user_notes: Option<String>,
}

/// Single clip descriptor inside a multi-clip stitched sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StitchedClipSource {
    pub path: PathBuf,
    #[serde(default)]
    pub in_point_ticks: Option<u64>,
    #[serde(default)]
    pub out_point_ticks: Option<u64>,
}

/// Caption export format for sidecar generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionExportFormat {
    Srt,
    WebVtt,
    Scc,
    Mcc,
    Ttml,
}

impl CaptionExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::WebVtt => "vtt",
            Self::Scc => "scc",
            Self::Mcc => "mcc",
            Self::Ttml => "ttml",
        }
    }
}

/// Audio stem export mode for multichannel workflows (AME parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AudioStemMode {
    /// No stem splitting (default single master output).
    #[default]
    None,
    /// Export each channel in the mix as a separate discrete mono BWF WAV file.
    DiscreteMono,
    /// Export channel pairs (e.g. 1-2, 3-4) as stereo BWF WAV files.
    StereoPairs,
}

/// Classification of media payload in a container or stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MediaStreamKind {
    /// Audio-only media (e.g. MP3, WAV, FLAC, AIFF, Ogg, AAC, M4A).
    AudioOnly,
    /// Video containing one or more audio tracks.
    VideoWithAudio,
    /// Video-only media (no audio tracks present).
    VideoOnly,
    /// Single still image (PNG, TIFF, JPEG, EXR, WebP).
    StillImage,
    /// Sequence of still images.
    ImageSequence,
    /// 2D vector graphic (SVG, AI, EPS).
    Vector2D,
    /// Digital document or page layout (PDF, DOCX, IDML, INDD, EPUB, PPTX, XLSX).
    Document,
    /// 3D spatial asset (glTF, GLB, OBJ, STL, PLY).
    Spatial3D,
    /// 2D CAD blueprint or technical drawing (AutoCAD DXF, DWG, HPGL).
    Cad2D,
    /// Multi-layer composite image with masks, layers, and adjustment channels (PSD, PSB, PhotoCraft).
    LayeredComposite,
    /// Procedural motion graphics, composition DAG, keyframed curves (After Effects AEP, EffectCraft).
    MotionGraph,
    /// Non-linear editing video timeline with cuts, tracks, and transitions (Premiere Pro PRPROJ, FilmCraft).
    ProjectTimeline,
    /// Unknown or unprobed format.
    #[default]
    Unknown,
}

impl MediaStreamKind {
    /// Returns true if this media kind contains a video stream.
    pub fn has_video(&self) -> bool {
        matches!(self, Self::VideoWithAudio | Self::VideoOnly | Self::StillImage | Self::ImageSequence | Self::MotionGraph | Self::ProjectTimeline)
    }

    /// Returns true if this media kind is strictly audio.
    pub fn is_audio_only(&self) -> bool {
        matches!(self, Self::AudioOnly)
    }

    /// Returns true if this media kind is a 2D vector asset.
    pub fn is_vector(&self) -> bool {
        matches!(self, Self::Vector2D)
    }

    /// Returns true if this media kind is a digital document or page layout.
    pub fn is_document(&self) -> bool {
        matches!(self, Self::Document)
    }

    /// Returns true if this media kind is a 3D asset.
    pub fn is_spatial_3d(&self) -> bool {
        matches!(self, Self::Spatial3D)
    }

    /// Returns true if this media kind is a 2D CAD technical drawing.
    pub fn is_cad(&self) -> bool {
        matches!(self, Self::Cad2D)
    }

    /// Returns true if this media kind is a layered image composite.
    pub fn is_layered_composite(&self) -> bool {
        matches!(self, Self::LayeredComposite)
    }

    /// Returns true if this media kind is a motion graphics composition.
    pub fn is_motion_graph(&self) -> bool {
        matches!(self, Self::MotionGraph)
    }

    /// Returns true if this media kind is an NLE project timeline.
    pub fn is_project_timeline(&self) -> bool {
        matches!(self, Self::ProjectTimeline)
    }
}

/// Mode of media source ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SourceDescriptor {
    /// File source already present on disk.
    File {
        path: PathBuf,
        #[serde(default)]
        start_tick: Option<i64>,
        #[serde(default)]
        duration_ticks: Option<i64>,
        #[serde(default)]
        timebase: Option<Rational64>,
    },
    /// Multiple sequential clips stitched together into a single continuous sequence.
    StitchedClips { clips: Vec<StitchedClipSource> },
    /// Live frame streaming over local IPC domain socket or named pipe.
    Pipe { endpoint: String, width: u32, height: u32, frame_rate: Rational64, sample_rate: u32, channels: u32 },
    /// Headless project delegation (renders via origin app's CLI).
    ProjectReference { project_path: PathBuf, composition_id: String, host_cli_path: Option<PathBuf> },
}

/// High-level output media container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerType {
    IsoBmffMp4,
    QuickTimeMov,
    MatroskaMkv,
    WebM,
    MxfOp1a,
    MxfOpAtom,
    Wave,
    Aiff,
    Flac,
    OggAudio,
    Mp3,
    StillSequence,
    AnimatedGif,
    Png,
    Jpeg,
    WebP,
    Tiff,
    OpenExr,
    Bmp,
    Svg,
    PostScriptEps,
    Gltf,
    Usdz,
    Obj,
    Stl,
    Ply,
    Pdf,
    Docx,
    Idml,
    // Adobe proprietary formats
    AdobeInDesignIndd,
    AdobePhotoshopPsd,
    AdobePhotoshopPsb,
    AdobeIllustratorAi,
    AdobeAfterEffectsAep,
    AdobePremierePrproj,
    AdobeAuditionSesx,
    // CAD & Technical drafting formats
    AutocadDxf,
    AutocadDwg,
    HpglPlotter,
    // Native ArtCraft creative formats
    ArtcraftDesigncraft,
    ArtcraftPhotocraft,
    ArtcraftVectorcraft,
    ArtcraftEffectcraft,
    ArtcraftFilmcraft,
    ArtcraftSoundcraft,
    ArtcraftCadcraft,
    ArtcraftAnicraft,
}

impl ContainerType {
    /// Every container variant, for lookups and enumeration.
    pub const ALL: &'static [ContainerType] = &[
        Self::IsoBmffMp4,
        Self::QuickTimeMov,
        Self::MatroskaMkv,
        Self::WebM,
        Self::MxfOp1a,
        Self::MxfOpAtom,
        Self::Wave,
        Self::Aiff,
        Self::Flac,
        Self::OggAudio,
        Self::Mp3,
        Self::StillSequence,
        Self::AnimatedGif,
        Self::Png,
        Self::Jpeg,
        Self::WebP,
        Self::Tiff,
        Self::OpenExr,
        Self::Bmp,
        Self::Svg,
        Self::PostScriptEps,
        Self::Gltf,
        Self::Usdz,
        Self::Obj,
        Self::Stl,
        Self::Ply,
        Self::Pdf,
        Self::Docx,
        Self::Idml,
        Self::AdobeInDesignIndd,
        Self::AdobePhotoshopPsd,
        Self::AdobePhotoshopPsb,
        Self::AdobeIllustratorAi,
        Self::AdobeAfterEffectsAep,
        Self::AdobePremierePrproj,
        Self::AdobeAuditionSesx,
        Self::AutocadDxf,
        Self::AutocadDwg,
        Self::HpglPlotter,
        Self::ArtcraftDesigncraft,
        Self::ArtcraftPhotocraft,
        Self::ArtcraftVectorcraft,
        Self::ArtcraftEffectcraft,
        Self::ArtcraftFilmcraft,
        Self::ArtcraftSoundcraft,
        Self::ArtcraftCadcraft,
        Self::ArtcraftAnicraft,
    ];

    /// Canonical file extension without leading dot.
    pub fn file_extension(&self) -> &'static str {
        match self {
            Self::IsoBmffMp4 => "mp4",
            Self::QuickTimeMov => "mov",
            Self::MatroskaMkv => "mkv",
            Self::WebM => "webm",
            Self::MxfOp1a | Self::MxfOpAtom => "mxf",
            Self::Wave => "wav",
            Self::Aiff => "aif",
            Self::Flac => "flac",
            Self::OggAudio => "ogg",
            Self::Mp3 => "mp3",
            Self::StillSequence => "png",
            Self::AnimatedGif => "gif",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Tiff => "tif",
            Self::OpenExr => "exr",
            Self::Bmp => "bmp",
            Self::Svg => "svg",
            Self::PostScriptEps => "eps",
            Self::Gltf => "glb",
            Self::Usdz => "usdz",
            Self::Obj => "obj",
            Self::Stl => "stl",
            Self::Ply => "ply",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Idml => "idml",
            Self::AdobeInDesignIndd => "indd",
            Self::AdobePhotoshopPsd => "psd",
            Self::AdobePhotoshopPsb => "psb",
            Self::AdobeIllustratorAi => "ai",
            Self::AdobeAfterEffectsAep => "aep",
            Self::AdobePremierePrproj => "prproj",
            Self::AdobeAuditionSesx => "sesx",
            Self::AutocadDxf => "dxf",
            Self::AutocadDwg => "dwg",
            Self::HpglPlotter => "plt",
            Self::ArtcraftDesigncraft => "designcraft",
            Self::ArtcraftPhotocraft => "photocraft",
            Self::ArtcraftVectorcraft => "vectorcraft",
            Self::ArtcraftEffectcraft => "effectcraft",
            Self::ArtcraftFilmcraft => "filmcraft",
            Self::ArtcraftSoundcraft => "soundcraft",
            Self::ArtcraftCadcraft => "cadcraft",
            Self::ArtcraftAnicraft => "anicraft",
        }
    }

    /// Resolves a container from a file extension (case-insensitive, with or without a leading dot) or a common alias.
    pub fn from_extension(ext: &str) -> Option<ContainerType> {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        let alias = match ext.as_str() {
            "m4v" => Some(Self::IsoBmffMp4),
            "aiff" => Some(Self::Aiff),
            "jpeg" => Some(Self::Jpeg),
            "tiff" => Some(Self::Tiff),
            "gltf" => Some(Self::Gltf),
            "oga" => Some(Self::OggAudio),
            "ps" => Some(Self::PostScriptEps),
            _ => None,
        };
        if alias.is_some() {
            return alias;
        }
        Self::ALL.iter().copied().find(|c| c.file_extension() == ext && !matches!(c, Self::MxfOpAtom | Self::StillSequence))
    }

    /// Whether converting a `self` source into `target` is a supported conversion.
    pub fn can_convert_to(&self, target: ContainerType) -> bool {
        self.valid_conversion_targets().contains(&target)
    }

    /// User-friendly display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::IsoBmffMp4 => "MP4 (ISO BMFF)",
            Self::QuickTimeMov => "QuickTime Movie (MOV)",
            Self::MatroskaMkv => "Matroska Video (MKV)",
            Self::WebM => "WebM",
            Self::MxfOp1a => "MXF OP1a Broadcast",
            Self::MxfOpAtom => "MXF OP-Atom",
            Self::Wave => "Waveform Audio (WAV / BWF)",
            Self::Aiff => "Audio Interchange (AIFF)",
            Self::Flac => "Free Lossless Audio Codec (FLAC)",
            Self::OggAudio => "Ogg Vorbis Audio",
            Self::Mp3 => "MPEG Audio Layer 3 (MP3)",
            Self::StillSequence => "Numbered Image Sequence",
            Self::AnimatedGif => "Animated GIF",
            Self::Png => "Portable Network Graphics (PNG)",
            Self::Jpeg => "JPEG Image",
            Self::WebP => "WebP Image",
            Self::Tiff => "Tagged Image File Format (TIFF)",
            Self::OpenExr => "ILM OpenEXR (Float)",
            Self::Bmp => "Windows Bitmap (BMP)",
            Self::Svg => "Scalable Vector Graphics (SVG)",
            Self::PostScriptEps => "Encapsulated PostScript (EPS)",
            Self::Gltf => "glTF 2.0 Binary (GLB)",
            Self::Usdz => "Apple AR Quick Look (USDZ)",
            Self::Obj => "Wavefront 3D (OBJ)",
            Self::Stl => "Stereolithography 3D (STL)",
            Self::Ply => "Stanford Polygon 3D (PLY)",
            Self::Pdf => "Portable Document Format (PDF)",
            Self::Docx => "Microsoft Word Document (DOCX)",
            Self::Idml => "InDesign Markup Language (IDML)",
            Self::AdobeInDesignIndd => "Adobe InDesign Document (INDD)",
            Self::AdobePhotoshopPsd => "Adobe Photoshop Document (PSD)",
            Self::AdobePhotoshopPsb => "Adobe Photoshop Large Document (PSB)",
            Self::AdobeIllustratorAi => "Adobe Illustrator Artwork (AI)",
            Self::AdobeAfterEffectsAep => "Adobe After Effects Project (AEP)",
            Self::AdobePremierePrproj => "Adobe Premiere Pro Project (PRPROJ)",
            Self::AdobeAuditionSesx => "Adobe Audition Session (SESX)",
            Self::AutocadDxf => "AutoCAD Drawing Exchange (DXF)",
            Self::AutocadDwg => "AutoCAD Drawing (DWG)",
            Self::HpglPlotter => "HPGL / PLT Plotter Stream",
            Self::ArtcraftDesigncraft => "DesignCraft Publication (.designcraft)",
            Self::ArtcraftPhotocraft => "PhotoCraft Composite (.photocraft)",
            Self::ArtcraftVectorcraft => "VectorCraft Artwork (.vectorcraft)",
            Self::ArtcraftEffectcraft => "EffectCraft Composition (.effectcraft)",
            Self::ArtcraftFilmcraft => "FilmCraft Timeline (.filmcraft)",
            Self::ArtcraftSoundcraft => "SoundCraft Session (.soundcraft)",
            Self::ArtcraftCadcraft => "CadCraft Blueprint (.cadcraft)",
            Self::ArtcraftAnicraft => "AniCraft Animation (.anicraft)",
        }
    }

    /// Primary media domain for this container.
    pub fn media_kind(&self) -> MediaStreamKind {
        match self {
            Self::IsoBmffMp4 | Self::QuickTimeMov | Self::MatroskaMkv | Self::WebM | Self::MxfOp1a | Self::MxfOpAtom | Self::AnimatedGif => {
                MediaStreamKind::VideoWithAudio
            }
            Self::Wave | Self::Aiff | Self::Flac | Self::OggAudio | Self::Mp3 | Self::AdobeAuditionSesx | Self::ArtcraftSoundcraft => {
                MediaStreamKind::AudioOnly
            }
            Self::Png | Self::Jpeg | Self::WebP | Self::Tiff | Self::OpenExr | Self::Bmp => MediaStreamKind::StillImage,
            Self::StillSequence | Self::ArtcraftAnicraft => MediaStreamKind::ImageSequence,
            Self::Svg | Self::AdobeIllustratorAi | Self::ArtcraftVectorcraft | Self::PostScriptEps => MediaStreamKind::Vector2D,
            Self::Pdf | Self::Docx | Self::Idml | Self::AdobeInDesignIndd | Self::ArtcraftDesigncraft => MediaStreamKind::Document,
            Self::Gltf | Self::Usdz | Self::Obj | Self::Stl | Self::Ply => MediaStreamKind::Spatial3D,
            Self::AutocadDxf | Self::AutocadDwg | Self::HpglPlotter | Self::ArtcraftCadcraft => MediaStreamKind::Cad2D,
            Self::AdobePhotoshopPsd | Self::AdobePhotoshopPsb | Self::ArtcraftPhotocraft => MediaStreamKind::LayeredComposite,
            Self::AdobeAfterEffectsAep | Self::ArtcraftEffectcraft => MediaStreamKind::MotionGraph,
            Self::AdobePremierePrproj | Self::ArtcraftFilmcraft => MediaStreamKind::ProjectTimeline,
        }
    }

    /// Returns valid, compatible target container types for transcode or export from this source container.
    pub fn valid_conversion_targets(&self) -> Vec<ContainerType> {
        match self.media_kind() {
            MediaStreamKind::AudioOnly if matches!(self, Self::AdobeAuditionSesx | Self::ArtcraftSoundcraft) => {
                vec![Self::Wave, Self::Aiff, Self::Flac, Self::AdobeAuditionSesx, Self::ArtcraftSoundcraft]
            }
            MediaStreamKind::AudioOnly => vec![Self::Wave, Self::Aiff, Self::Flac, Self::Mp3, Self::OggAudio, Self::ArtcraftSoundcraft],
            MediaStreamKind::VideoWithAudio | MediaStreamKind::VideoOnly => vec![
                Self::IsoBmffMp4,
                Self::QuickTimeMov,
                Self::WebM,
                Self::MatroskaMkv,
                Self::MxfOp1a,
                Self::MxfOpAtom,
                Self::AnimatedGif,
                Self::StillSequence,
                Self::Wave,
                Self::Aiff,
                Self::Flac,
                Self::Mp3,
            ],
            MediaStreamKind::StillImage => {
                vec![Self::Png, Self::Jpeg, Self::WebP, Self::Tiff, Self::OpenExr, Self::Bmp, Self::AdobePhotoshopPsd, Self::ArtcraftPhotocraft]
            }
            MediaStreamKind::LayeredComposite => vec![
                Self::ArtcraftPhotocraft,
                Self::AdobePhotoshopPsd,
                Self::AdobePhotoshopPsb,
                Self::OpenExr,
                Self::Tiff,
                Self::Png,
                Self::Jpeg,
                Self::WebP,
                Self::Bmp,
            ],
            MediaStreamKind::Vector2D => {
                vec![Self::ArtcraftVectorcraft, Self::Svg, Self::Pdf, Self::PostScriptEps, Self::AutocadDxf, Self::Png, Self::AdobeIllustratorAi]
            }
            MediaStreamKind::Document => vec![Self::ArtcraftDesigncraft, Self::Pdf, Self::Idml, Self::Docx, Self::Png, Self::Tiff],
            MediaStreamKind::Spatial3D => vec![Self::Gltf, Self::Usdz, Self::Obj, Self::Stl, Self::Ply],
            MediaStreamKind::Cad2D => {
                vec![Self::ArtcraftCadcraft, Self::AutocadDxf, Self::AutocadDwg, Self::Svg, Self::Pdf, Self::HpglPlotter, Self::Png]
            }
            MediaStreamKind::MotionGraph => vec![
                Self::ArtcraftEffectcraft,
                Self::IsoBmffMp4,
                Self::QuickTimeMov,
                Self::WebM,
                Self::MatroskaMkv,
                Self::AnimatedGif,
                Self::StillSequence,
                Self::Svg,
            ],
            MediaStreamKind::ProjectTimeline => {
                vec![Self::ArtcraftFilmcraft, Self::IsoBmffMp4, Self::QuickTimeMov, Self::MxfOp1a, Self::MatroskaMkv, Self::Wave]
            }
            MediaStreamKind::ImageSequence => vec![
                Self::IsoBmffMp4,
                Self::QuickTimeMov,
                Self::WebM,
                Self::MatroskaMkv,
                Self::AnimatedGif,
                Self::Png,
                Self::Tiff,
                Self::OpenExr,
                Self::ArtcraftAnicraft,
            ],
            MediaStreamKind::Unknown => vec![Self::IsoBmffMp4, Self::QuickTimeMov, Self::Wave, Self::Png],
        }
    }
}

/// Which part of a multi-item source a target exports. All-`None` exports the whole source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSelection {
    /// Sequence or composition of a project, by exact name or zero-based index.
    #[serde(default)]
    pub sequence: Option<String>,
    /// Inclusive 1-based page range of a document.
    #[serde(default)]
    pub pages: Option<(u32, u32)>,
    /// Names of the layers to keep (CAD and layered raster sources).
    #[serde(default)]
    pub layers: Option<Vec<String>>,
}

impl SourceSelection {
    pub fn is_empty(&self) -> bool {
        self.sequence.is_none() && self.pages.is_none() && self.layers.is_none()
    }

    /// Rejects selections that can never be satisfied.
    pub fn validate(&self) -> Result<(), EncoderError> {
        if let Some((first, last)) = self.pages
            && (first == 0 || last < first)
        {
            return Err(EncoderError::InvalidJobSpec(format!(
                "invalid page range {first}-{last}: pages are 1-based and the range must not be reversed"
            )));
        }
        if let Some(layers) = &self.layers
            && layers.is_empty()
        {
            return Err(EncoderError::InvalidJobSpec("layer selection must name at least one layer".into()));
        }
        if let Some(seq) = &self.sequence
            && seq.trim().is_empty()
        {
            return Err(EncoderError::InvalidJobSpec("sequence selection cannot be empty".into()));
        }
        Ok(())
    }

    /// Parses `N` or `A-B` (1-based, inclusive).
    pub fn parse_pages(text: &str) -> Result<(u32, u32), EncoderError> {
        let bad = || EncoderError::InvalidJobSpec(format!("invalid page range '{text}' (use N or A-B, 1-based)"));
        let (a, b) = match text.split_once('-') {
            Some((a, b)) => (a.trim().parse::<u32>().map_err(|_| bad())?, b.trim().parse::<u32>().map_err(|_| bad())?),
            None => {
                let n = text.trim().parse::<u32>().map_err(|_| bad())?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return Err(bad());
        }
        Ok((a, b))
    }
}

/// Rate-control strategy requested for a video encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RateMode {
    /// Constant bitrate when a bitrate is given, constant quality otherwise.
    #[default]
    Auto,
    Cbr,
    Vbr,
    /// Constant quality; the bitrate is ignored.
    ConstantQuality,
}

/// Colour description written into the video bitstream (ITU-T H.273 code points).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorTag {
    /// BT.709 primaries, transfer and matrix, limited range.
    #[default]
    Bt709,
    /// BT.2020 with the PQ (SMPTE ST 2084) transfer.
    Pq,
    /// BT.2020 with the HLG (ARIB STD-B67) transfer.
    Hlg,
}

impl ColorTag {
    /// `(primaries, transfer, matrix)` per ITU-T H.273.
    pub const fn code_points(self) -> (u8, u8, u8) {
        match self {
            Self::Bt709 => (1, 1, 1),
            Self::Pq => (9, 16, 9),
            Self::Hlg => (9, 18, 9),
        }
    }
}

/// Video encoder settings of a target; every field falls back to a codec default when `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoEncodeSettings {
    /// Codec identifier (`h264`, `hevc`, `av1`, `vp9`, `prores`, `dnxhr`, ...). `None` infers it from the preset name.
    pub codec: Option<String>,
    /// Maximum distance between keyframes in frames.
    pub keyframe_interval: Option<u32>,
    /// Codec profile: ProRes `proxy|lt|422|hq|4444|4444xq`, DNx `lb|sq|hq|hqx|444`, H.264 `baseline|main|high`.
    pub profile: Option<String>,
    /// Consecutive B-frames, for codecs that support them.
    pub bframes: Option<u8>,
    pub rate_mode: RateMode,
    pub color: Option<ColorTag>,
}

/// Output target specifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTarget {
    pub target_id: String,
    pub output_path: PathBuf,
    pub preset_name: String,
    pub container: ContainerType,
    #[serde(default)]
    pub custom_width: Option<u32>,
    #[serde(default)]
    pub custom_height: Option<u32>,
    #[serde(default)]
    pub custom_frame_rate: Option<Rational64>,
    #[serde(default)]
    pub target_bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub target_lufs: Option<f32>,
    #[serde(default)]
    pub lut_path: Option<PathBuf>,
    #[serde(default)]
    pub burn_timecode: bool,
    #[serde(default)]
    pub caption_path: Option<PathBuf>,
    #[serde(default)]
    pub caption_format: Option<CaptionExportFormat>,
    #[serde(default)]
    pub burn_captions: bool,
    #[serde(default)]
    pub caption_offset_ms: Option<i64>,
    #[serde(default)]
    pub audio_stems: Option<AudioStemMode>,
    #[serde(default)]
    pub metadata: Option<crate::metadata::XmpMetadata>,
    #[serde(default)]
    pub hardware_acceleration: bool,
    /// Still-image quality 1..=100 (JPEG and lossy WebP); `None` uses the encoder default.
    #[serde(default)]
    pub image_quality: Option<u8>,
    /// Sequence, page range or layers of the source to export.
    #[serde(default)]
    pub selection: SourceSelection,
    /// Codec, GOP, B-frame, rate-control and colour settings of the video stream.
    #[serde(default)]
    pub video: VideoEncodeSettings,
}

impl Default for OutputTarget {
    fn default() -> Self {
        Self {
            target_id: String::new(),
            output_path: PathBuf::new(),
            preset_name: String::new(),
            container: ContainerType::IsoBmffMp4,
            custom_width: None,
            custom_height: None,
            custom_frame_rate: None,
            target_bitrate_kbps: None,
            target_lufs: None,
            lut_path: None,
            burn_timecode: false,
            caption_path: None,
            caption_format: None,
            burn_captions: false,
            caption_offset_ms: None,
            audio_stems: None,
            metadata: None,
            hardware_acceleration: false,
            image_quality: None,
            selection: SourceSelection::default(),
            video: VideoEncodeSettings::default(),
        }
    }
}

/// Complete, self-contained transcode job specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftJobSpec {
    pub version: String,
    pub job_id: String,
    pub client_info: ClientInfo,
    #[serde(default)]
    pub priority: JobPriority,
    #[serde(default)]
    pub start_policy: StartPolicy,
    pub source: SourceDescriptor,
    pub targets: Vec<OutputTarget>,
}

impl CraftJobSpec {
    /// Validates the internal consistency of the job specification.
    pub fn validate(&self) -> Result<(), EncoderError> {
        if self.job_id.trim().is_empty() {
            return Err(EncoderError::InvalidJobSpec("jobId cannot be empty".into()));
        }
        for target in &self.targets {
            target.selection.validate()?;
        }

        if self.targets.is_empty() {
            return Err(EncoderError::InvalidJobSpec("job must define at least one output target".into()));
        }

        for (idx, target) in self.targets.iter().enumerate() {
            if target.target_id.trim().is_empty() {
                return Err(EncoderError::InvalidJobSpec(format!("target at index {idx} has empty targetId")));
            }
            if target.output_path.as_os_str().is_empty() {
                return Err(EncoderError::InvalidJobSpec(format!("target `{}` has empty outputPath", target.target_id)));
            }
        }

        Ok(())
    }

    /// Serializes this job specification to a JSON string.
    pub fn to_json(&self) -> Result<String, EncoderError> {
        serde_json::to_string_pretty(self).map_err(|e| EncoderError::InvalidJobSpec(format!("JSON serialization failed: {e}")))
    }

    /// Deserializes a job specification from a JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, EncoderError> {
        let spec: Self = serde_json::from_str(json_str).map_err(|e| EncoderError::InvalidJobSpec(format!("JSON deserialization failed: {e}")))?;
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_spec_json_roundtrip() {
        let spec = CraftJobSpec {
            version: "1.0.0".into(),
            job_id: "test_job_001".into(),
            client_info: ClientInfo { app_name: "FilmCraft".into(), app_version: "0.9.4".into(), pid: 1234, user_notes: Some("Test export".into()) },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("/media/test.mov"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "tgt_01".into(),
                output_path: PathBuf::from("/media/out.mp4"),
                preset_name: "YouTube 4K".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(3840),
                custom_height: Some(2160),
                custom_frame_rate: Some(Rational64::FPS_60),
                target_bitrate_kbps: Some(45000),
                target_lufs: Some(-14.0),
                lut_path: None,
                burn_timecode: false,
                ..Default::default()
            }],
        };

        let json = spec.to_json().unwrap();
        let deserialized = CraftJobSpec::from_json(&json).unwrap();
        assert_eq!(spec, deserialized);
    }

    #[test]
    fn test_container_type_extensions_and_media_kinds() {
        assert_eq!(ContainerType::AdobeInDesignIndd.file_extension(), "indd");
        assert_eq!(ContainerType::AdobePhotoshopPsd.file_extension(), "psd");
        assert_eq!(ContainerType::AdobePhotoshopPsb.file_extension(), "psb");
        assert_eq!(ContainerType::AdobeIllustratorAi.file_extension(), "ai");
        assert_eq!(ContainerType::AdobeAfterEffectsAep.file_extension(), "aep");
        assert_eq!(ContainerType::AdobePremierePrproj.file_extension(), "prproj");
        assert_eq!(ContainerType::AdobeAuditionSesx.file_extension(), "sesx");
        assert_eq!(ContainerType::AutocadDxf.file_extension(), "dxf");
        assert_eq!(ContainerType::AutocadDwg.file_extension(), "dwg");
        assert_eq!(ContainerType::HpglPlotter.file_extension(), "plt");
        assert_eq!(ContainerType::ArtcraftDesigncraft.file_extension(), "designcraft");
        assert_eq!(ContainerType::ArtcraftPhotocraft.file_extension(), "photocraft");
        assert_eq!(ContainerType::ArtcraftVectorcraft.file_extension(), "vectorcraft");
        assert_eq!(ContainerType::ArtcraftEffectcraft.file_extension(), "effectcraft");
        assert_eq!(ContainerType::ArtcraftFilmcraft.file_extension(), "filmcraft");
        assert_eq!(ContainerType::ArtcraftSoundcraft.file_extension(), "soundcraft");
        assert_eq!(ContainerType::ArtcraftCadcraft.file_extension(), "cadcraft");
        assert_eq!(ContainerType::ArtcraftAnicraft.file_extension(), "anicraft");
        assert_eq!(ContainerType::PostScriptEps.file_extension(), "eps");

        // Media kind mapping
        assert_eq!(ContainerType::AdobeInDesignIndd.media_kind(), MediaStreamKind::Document);
        assert_eq!(ContainerType::AdobePhotoshopPsd.media_kind(), MediaStreamKind::LayeredComposite);
        assert_eq!(ContainerType::AutocadDxf.media_kind(), MediaStreamKind::Cad2D);
        assert_eq!(ContainerType::AdobeAfterEffectsAep.media_kind(), MediaStreamKind::MotionGraph);
        assert_eq!(ContainerType::AdobePremierePrproj.media_kind(), MediaStreamKind::ProjectTimeline);
        assert_eq!(ContainerType::AdobeAuditionSesx.media_kind(), MediaStreamKind::AudioOnly);
        assert_eq!(ContainerType::PostScriptEps.media_kind(), MediaStreamKind::Vector2D);
    }

    #[test]
    fn test_container_type_valid_conversion_targets() {
        let vector_targets = ContainerType::AdobeIllustratorAi.valid_conversion_targets();
        assert!(vector_targets.contains(&ContainerType::ArtcraftVectorcraft));
        assert!(vector_targets.contains(&ContainerType::Svg));
        assert!(vector_targets.contains(&ContainerType::PostScriptEps));
        assert!(vector_targets.contains(&ContainerType::Pdf));
        assert!(vector_targets.contains(&ContainerType::AutocadDxf));
        let cad_targets = ContainerType::AutocadDxf.valid_conversion_targets();
        assert!(cad_targets.contains(&ContainerType::ArtcraftCadcraft));
        assert!(cad_targets.contains(&ContainerType::Svg));

        let doc_targets = ContainerType::AdobeInDesignIndd.valid_conversion_targets();
        assert!(doc_targets.contains(&ContainerType::ArtcraftDesigncraft));
        assert!(doc_targets.contains(&ContainerType::Pdf));
        assert!(doc_targets.contains(&ContainerType::Idml));

        let psd_targets = ContainerType::AdobePhotoshopPsd.valid_conversion_targets();
        assert!(psd_targets.contains(&ContainerType::ArtcraftPhotocraft));
        assert!(psd_targets.contains(&ContainerType::OpenExr));
    }

    #[test]
    fn test_container_from_extension_and_conversion_rules() {
        assert_eq!(ContainerType::from_extension(".PSD"), Some(ContainerType::AdobePhotoshopPsd));
        assert_eq!(ContainerType::from_extension("aiff"), Some(ContainerType::Aiff));
        assert_eq!(ContainerType::from_extension("mxf"), Some(ContainerType::MxfOp1a));
        assert_eq!(ContainerType::from_extension("png"), Some(ContainerType::Png));
        assert_eq!(ContainerType::from_extension("nope"), None);
        for c in ContainerType::ALL {
            if !matches!(c, ContainerType::MxfOpAtom | ContainerType::StillSequence) {
                assert_eq!(ContainerType::from_extension(c.file_extension()), Some(*c), "{c:?}");
            }
        }
        assert!(ContainerType::AdobeInDesignIndd.can_convert_to(ContainerType::Pdf));
        assert!(!ContainerType::AdobeInDesignIndd.can_convert_to(ContainerType::Wave));
        assert!(!ContainerType::AutocadDwg.can_convert_to(ContainerType::IsoBmffMp4));
        assert!(ContainerType::AdobeAuditionSesx.can_convert_to(ContainerType::ArtcraftSoundcraft));
        assert!(ContainerType::ArtcraftSoundcraft.can_convert_to(ContainerType::AdobeAuditionSesx));
    }

    #[test]
    fn test_source_selection_validation_and_page_parsing() {
        assert_eq!(SourceSelection::parse_pages("3").ok(), Some((3, 3)));
        assert_eq!(SourceSelection::parse_pages("2-5").ok(), Some((2, 5)));
        for bad in ["0", "5-2", "a-b", "", "1-", "-3"] {
            assert!(SourceSelection::parse_pages(bad).is_err(), "{bad}");
        }
        assert!(SourceSelection::default().is_empty());
        assert!(SourceSelection { pages: Some((0, 1)), ..Default::default() }.validate().is_err());
        assert!(SourceSelection { layers: Some(vec![]), ..Default::default() }.validate().is_err());
        assert!(SourceSelection { sequence: Some(" ".into()), ..Default::default() }.validate().is_err());
        assert!(SourceSelection { sequence: Some("Main".into()), pages: Some((1, 2)), layers: Some(vec!["a".into()]) }.validate().is_ok());
        let json = serde_json::to_string(&OutputTarget::default()).unwrap_or_default();
        let back: OutputTarget =
            serde_json::from_str(&json.replace(",\"selection\":{\"sequence\":null,\"pages\":null,\"layers\":null}", "")).unwrap();
        assert!(back.selection.is_empty(), "selection must default when absent from older job files");
    }

    #[test]
    fn test_media_stream_kind_classification() {
        assert!(MediaStreamKind::Cad2D.is_cad());
        assert!(!MediaStreamKind::Cad2D.has_video());

        assert!(MediaStreamKind::LayeredComposite.is_layered_composite());
        assert!(MediaStreamKind::MotionGraph.is_motion_graph());
        assert!(MediaStreamKind::MotionGraph.has_video());

        assert!(MediaStreamKind::ProjectTimeline.is_project_timeline());
        assert!(MediaStreamKind::ProjectTimeline.has_video());
    }
}
