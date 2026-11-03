//! Unified media decoding for encoderCraft.
//!
//! This crate is a thin adapter over the ArtCraft `filmcraft-media` / `filmcraft-codecs` crates, which
//! already implement the container demuxers (MP4/MOV, Matroska/WebM, MXF, AVI, MPEG-TS/PS, Ogg, Y4M, APV)
//! and the decoders (H.264, HEVC, VP8/VP9, AV1, MPEG-2, ProRes, DNx, AAC, Opus, MP3, FLAC, PCM) as well as
//! still-image formats (PNG, JPEG, TIFF, WebP, GIF, BMP, TGA, DPX, EXR, HDR, DNG, PSD). It converts their
//! frame and audio types into the encoderCraft pipeline types and reports failures as typed errors:
//! an undecodable file is an error, never silently replaced by placeholder content.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError, Rational64, VideoFrame};
use encodercraft_geom::Dimensions;
use filmcraft_media::reader::FileReader;
use filmcraft_media::sequence::{FrameLoader, ImageSequenceSource, Numbered, sequence_frames};
use filmcraft_media::{FrameRequest, MediaError, MediaKind, SharedReader, SharedSource};
use filmcraft_time::{FrameRate, Tick};

/// Largest still image file that is read whole when no streaming reader exists for its format.
const MAX_WHOLE_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Upper bound for audio that is decoded into a single in-memory buffer.
pub const MAX_AUDIO_DECODE_BYTES: usize = 2 * 1024 * 1024 * 1024;
/// Audio is decoded in windows of this many seconds.
const AUDIO_WINDOW_SECONDS: usize = 10;

/// What kind of media a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Movie,
    AudioOnly,
    Still,
    ImageSequence,
    Synthetic,
}

/// Description of the video stream of a source.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoProbe {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational64,
    pub codec: String,
    pub pixel_format: String,
    pub has_alpha: bool,
    pub bitrate: Option<u64>,
}

/// Description of the first audio stream of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioProbe {
    pub sample_rate: u32,
    pub channels: u32,
    pub codec: String,
    pub bits_per_sample: Option<u32>,
}

/// Facts about a source file, available without decoding it.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbe {
    pub name: String,
    pub container: String,
    pub kind: SourceKind,
    pub duration_seconds: f64,
    pub video: Option<VideoProbe>,
    pub audio: Vec<AudioProbe>,
    pub start_timecode_frames: Option<i64>,
    pub file_size: Option<u64>,
}

/// A fully decoded still image, upright and as straight-alpha RGBA8.
struct StillPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Extensions decoded by the shared photocraft still-image codecs (EXIF orientation applied, 16-bit,
/// float and CMYK sources converted to RGBA8).
const STILL_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp", "tga", "exr", "hdr"];

/// An opened media file that can deliver video frames by index and audio by sample position.
pub struct MediaReader {
    source: Option<SharedSource>,
    still: Option<StillPixels>,
    probe: MediaProbe,
}

fn map_media_error(context: &str, err: MediaError) -> EncoderError {
    match err {
        MediaError::Cancelled => EncoderError::Cancelled,
        MediaError::Io(msg) => EncoderError::Io(format!("{context}: {msg}")),
        MediaError::Unsupported(msg) => EncoderError::UnsupportedCodec(format!("{context}: {msg}")),
        MediaError::Decode(msg) => EncoderError::InvalidContainer(format!("{context}: {msg}")),
        MediaError::NoStream(kind) => EncoderError::InvalidContainer(format!("{context}: source has no {kind} stream")),
        MediaError::Offline(msg) => EncoderError::Io(format!("{context}: media offline: {msg}")),
    }
}

fn kind_of(kind: MediaKind) -> SourceKind {
    match kind {
        MediaKind::Movie => SourceKind::Movie,
        MediaKind::AudioOnly => SourceKind::AudioOnly,
        MediaKind::Still => SourceKind::Still,
        MediaKind::ImageSequence => SourceKind::ImageSequence,
        MediaKind::Synthetic => SourceKind::Synthetic,
    }
}

/// Maps a decoded channel count onto the nearest supported layout, padding with silence in the
/// standard channel order (L R C LFE Ls Rs Lrs Rrs) or truncating beyond eight channels.
fn layout_for(channels: usize) -> ChannelLayout {
    match channels {
        0 | 1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        3..=6 => ChannelLayout::Surround51,
        _ => ChannelLayout::Surround71,
    }
}

/// Converts a panic escaping third-party decoder code into a typed error so a hostile file can
/// never terminate the application.
fn guard<T>(what: &str, f: impl FnOnce() -> Result<T, EncoderError>) -> Result<T, EncoderError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(EncoderError::InvalidContainer(format!("media {what} failed: the file is malformed"))))
}

/// Frame rate given to an image sequence when the job does not choose one.
pub const SEQUENCE_DEFAULT_FPS: Rational64 = Rational64::FPS_24;

/// A numbered-frame path such as `shot_%04d.png`: the file name holds one printf-style `%d` field.
struct SequencePattern {
    dir: PathBuf,
    prefix: String,
    suffix: String,
    width: usize,
}

impl SequencePattern {
    fn parse(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let at = name.find('%')?;
        let rest = name.get(at + 1..)?;
        let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        let digits = rest.get(..digits_len)?;
        let after = rest.get(digits_len..)?;
        let suffix = after.strip_prefix('d')?;
        if suffix.contains('%') {
            return None;
        }
        let width = if digits.is_empty() { 0 } else { digits.parse::<usize>().ok()?.min(12) };
        let dir = path.parent().map(Path::to_path_buf).filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."));
        Some(Self { dir, prefix: name.get(..at)?.to_string(), suffix: suffix.to_string(), width })
    }

    fn number_of(&self, file_name: &str) -> Option<u64> {
        let middle = file_name.strip_prefix(self.prefix.as_str())?.strip_suffix(self.suffix.as_str())?;
        if middle.is_empty() || middle.len() > 12 || !middle.bytes().all(|b| b.is_ascii_digit()) || middle.len() < self.width {
            return None;
        }
        middle.parse().ok()
    }
}

/// True when `path` is a numbered-frame pattern like `shot_%04d.png` rather than a real file.
pub fn is_sequence_pattern(path: &Path) -> bool {
    SequencePattern::parse(path).is_some()
}

impl MediaReader {
    /// Opens a media file, probing its container and streams. A `%04d`-style path opens the
    /// numbered image sequence it describes at [`SEQUENCE_DEFAULT_FPS`].
    pub fn open(path: &Path) -> Result<Self, EncoderError> {
        Self::open_with_rate(path, None)
    }

    /// Like [`MediaReader::open`], with the frame rate image sequences play at; other media ignore it.
    pub fn open_with_rate(path: &Path, sequence_rate: Option<Rational64>) -> Result<Self, EncoderError> {
        if let Some(pattern) = SequencePattern::parse(path) {
            return guard("open", || Self::open_sequence(&pattern, path, sequence_rate.unwrap_or(SEQUENCE_DEFAULT_FPS)));
        }
        guard("open", || Self::open_unguarded(path))
    }

    fn open_sequence(pattern: &SequencePattern, path: &Path, rate: Rational64) -> Result<Self, EncoderError> {
        let context = path.display().to_string();
        if rate.num <= 0 || rate.den <= 0 {
            return Err(EncoderError::InvalidJobSpec(format!("{context}: invalid sequence frame rate {}/{}", rate.num, rate.den)));
        }
        let names: Vec<String> = std::fs::read_dir(&pattern.dir)
            .map_err(|e| EncoderError::Io(format!("{context}: {e}")))?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| pattern.number_of(name).is_some())
            .collect();
        let first_name = names
            .iter()
            .filter_map(|name| pattern.number_of(name).map(|n| (n, name)))
            .min()
            .map(|(_, name)| name.clone())
            .ok_or_else(|| EncoderError::Io(format!("{context}: no files match the sequence pattern")))?;
        let first_path = pattern.dir.join(&first_name);
        let first = Numbered::parse(&first_path.to_string_lossy())
            .ok_or_else(|| EncoderError::InvalidContainer(format!("{context}: `{first_name}` is not a numbered still image")))?;
        let frames = sequence_frames(&first, &names.iter().map(|n| n.to_string()).collect::<Vec<_>>());
        let loader: FrameLoader = Arc::new(|p: &str| std::fs::read(p));
        let name = format!("{}[sequence]", pattern.prefix.trim_end_matches(['_', '.', '-', ' ']));
        let source = ImageSequenceSource::new(&name, frames, FrameRate::new(rate.num, rate.den), loader).map_err(|e| map_media_error(&context, e))?;
        Ok(Self::from_source(Arc::new(source)))
    }

    fn open_unguarded(path: &Path) -> Result<Self, EncoderError> {
        if let Some(reader) = Self::open_still(path)? {
            return Ok(reader);
        }
        let context = path.display().to_string();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| context.clone());
        let file = FileReader::open(path).map_err(|e| EncoderError::Io(format!("{context}: {e}")))?;
        let reader: SharedReader = Arc::new(file);
        let source = filmcraft_media::reader::open_reader_within(
            &name,
            reader,
            &filmcraft_codecs::reader_openers(),
            &filmcraft_codecs::openers(),
            MAX_WHOLE_FILE_BYTES,
        )
        .map_err(|e| map_media_error(&context, e))?;
        Ok(Self::from_source(source))
    }

    /// Decodes a still image with the photocraft codecs; `None` when the file is not one of them
    /// (or is an animation, which the streaming decoder handles).
    fn open_still(path: &Path) -> Result<Option<Self>, EncoderError> {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        if !STILL_EXTENSIONS.contains(&ext.as_str()) {
            return Ok(None);
        }
        let context = path.display().to_string();
        let meta = std::fs::metadata(path).map_err(|e| EncoderError::Io(format!("{context}: {e}")))?;
        if meta.len() > MAX_WHOLE_FILE_BYTES {
            return Err(EncoderError::InvalidJobSpec(format!("{context}: image file is larger than the {} MiB limit", MAX_WHOLE_FILE_BYTES >> 20)));
        }
        let bytes = std::fs::read(path).map_err(|e| EncoderError::Io(format!("{context}: {e}")))?;
        if ext == "webp" && photocraft_codecs::webp_is_animated(&bytes) {
            return Ok(None);
        }
        let image = photocraft_codecs::decode_with(&bytes, &photocraft_codecs::DecodeOptions::default())
            .map_err(|e| EncoderError::InvalidContainer(format!("{context}: {e}")))?;
        let (width, height) = image.dimensions();
        let rgba = image.to_rgba8();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| context.clone());
        let probe = MediaProbe {
            name,
            container: ext.to_ascii_uppercase(),
            kind: SourceKind::Still,
            duration_seconds: 0.0,
            video: Some(VideoProbe {
                width,
                height,
                frame_rate: Rational64 { num: 1, den: 1 },
                codec: ext.to_ascii_uppercase(),
                pixel_format: format!("{:?}", image.layout()),
                has_alpha: matches!(image.layout(), photocraft_codecs::ChannelLayout::Rgba | photocraft_codecs::ChannelLayout::GrayA),
                bitrate: None,
            }),
            audio: Vec::new(),
            start_timecode_frames: None,
            file_size: Some(meta.len()),
        };
        Ok(Some(Self { source: None, still: Some(StillPixels { width, height, rgba }), probe }))
    }

    /// Wraps an already opened filmcraft media source.
    pub fn from_source(source: SharedSource) -> Self {
        let info = source.info();
        let fps_of = |num: i64, den: i64| Rational64 { num: num.max(1), den: den.max(1) };
        let probe = MediaProbe {
            name: info.name.clone(),
            container: info.container.clone(),
            kind: kind_of(info.kind),
            duration_seconds: info.duration.seconds().max(0.0),
            video: info.video.as_ref().map(|v| VideoProbe {
                width: v.width,
                height: v.height,
                frame_rate: fps_of(v.frame_rate.num, v.frame_rate.den),
                codec: v.codec.clone(),
                pixel_format: v.pixel_format.clone(),
                has_alpha: v.has_alpha,
                bitrate: v.bitrate,
            }),
            audio: info
                .audio_streams
                .iter()
                .map(|a| AudioProbe { sample_rate: a.sample_rate, channels: a.channels, codec: a.codec.clone(), bits_per_sample: a.bits_per_sample })
                .collect(),
            start_timecode_frames: info.start_timecode,
            file_size: info.file_size,
        };
        Self { source: Some(source), still: None, probe }
    }

    /// Probed stream and container facts.
    pub fn probe(&self) -> &MediaProbe {
        &self.probe
    }

    /// Number of video frames; stills have exactly one and audio-only sources none.
    pub fn frame_count(&self) -> u64 {
        let Some(video) = &self.probe.video else {
            return 0;
        };
        if matches!(self.probe.kind, SourceKind::Still) {
            return 1;
        }
        let fps = video.frame_rate.num as f64 / video.frame_rate.den as f64;
        let frames = (self.probe.duration_seconds * fps).round();
        if frames.is_finite() && frames >= 1.0 { frames as u64 } else { 1 }
    }

    /// Decodes the frame with the given index as straight-alpha RGBA8.
    pub fn frame_at_index(&self, index: u64) -> Result<VideoFrame, EncoderError> {
        guard("decode", || self.frame_at_index_unguarded(index))
    }

    fn frame_at_index_unguarded(&self, index: u64) -> Result<VideoFrame, EncoderError> {
        let video =
            self.probe.video.as_ref().ok_or_else(|| EncoderError::InvalidContainer(format!("{}: source has no video stream", self.probe.name)))?;
        let count = self.frame_count();
        if index >= count {
            return Err(EncoderError::InvalidJobSpec(format!("frame index {index} is outside the {count} frames of {}", self.probe.name)));
        }
        let idx = i64::try_from(index).map_err(|_| EncoderError::Internal("frame index overflow".into()))?;
        if let Some(still) = &self.still {
            let dims = Dimensions { width: still.width, height: still.height };
            return VideoFrame::from_raw_bytes(
                dims,
                PixelFormat::Rgba8,
                ColorSpaceTag::SRGB,
                idx,
                1,
                Rational64 { num: 1, den: 1 },
                still.rgba.clone(),
            );
        }
        let source = self.source.as_ref().ok_or_else(|| EncoderError::Internal("reader has no source".into()))?;
        let time = if matches!(self.probe.kind, SourceKind::Still) {
            Tick::ZERO
        } else {
            Tick::from_rational(idx, video.frame_rate.den, video.frame_rate.num)
        };
        let decoded = source.video_frame(FrameRequest::full(time)).map_err(|e| map_media_error(&self.probe.name, e))?;
        let rgba = decoded.to_rgba8();
        let dims = Dimensions { width: decoded.width, height: decoded.height };
        let color = if matches!(self.probe.kind, SourceKind::Still) { ColorSpaceTag::SRGB } else { ColorSpaceTag::REC709 };
        let timebase = Rational64 { num: video.frame_rate.den, den: video.frame_rate.num };
        VideoFrame::from_raw_bytes(dims, PixelFormat::Rgba8, color, idx, 1, timebase, rgba)
    }

    /// Decodes `frames` samples per channel starting at `start` (in samples at `sample_rate`).
    pub fn audio(&self, start: i64, frames: usize, sample_rate: u32) -> Result<AudioSampleBuffer, EncoderError> {
        if self.probe.audio.is_empty() {
            return Err(EncoderError::InvalidContainer(format!("{}: source has no audio stream", self.probe.name)));
        }
        let source =
            self.source.as_ref().ok_or_else(|| EncoderError::InvalidContainer(format!("{}: source has no audio stream", self.probe.name)))?;
        let buf = source.audio(start, frames, sample_rate).map_err(|e| map_media_error(&self.probe.name, e))?;
        let layout = layout_for(buf.channels.len());
        let wanted = layout as usize;
        let len = buf.frames();
        let mut planes = buf.channels;
        planes.truncate(wanted);
        while planes.len() < wanted {
            planes.push(vec![0.0; len]);
        }
        AudioSampleBuffer::from_planar_channels(buf.sample_rate, layout, start, planes)
    }

    /// Decodes the complete audio track at `sample_rate`, failing instead of exhausting memory.
    pub fn read_all_audio(&self, sample_rate: u32) -> Result<AudioSampleBuffer, EncoderError> {
        let total = (self.probe.duration_seconds * f64::from(sample_rate)).ceil();
        if !total.is_finite() || total < 0.0 {
            return Err(EncoderError::InvalidContainer(format!("{}: invalid audio duration", self.probe.name)));
        }
        self.read_audio_range(0, total as usize, sample_rate)
    }

    /// Decodes `total` samples per channel starting at sample `first` in bounded windows, failing
    /// instead of exhausting memory.
    pub fn read_audio_range(&self, first: i64, total: usize, sample_rate: u32) -> Result<AudioSampleBuffer, EncoderError> {
        let Some(stream) = self.probe.audio.first() else {
            return Err(EncoderError::InvalidContainer(format!("{}: source has no audio stream", self.probe.name)));
        };
        let channels = (stream.channels as usize).clamp(1, 8);
        let bytes =
            total.checked_mul(channels).and_then(|n| n.checked_mul(4)).ok_or_else(|| EncoderError::InvalidContainer("audio size overflow".into()))?;
        if bytes > MAX_AUDIO_DECODE_BYTES {
            return Err(EncoderError::InvalidJobSpec(format!(
                "audio of {} would need {} MiB in memory, over the {} MiB limit",
                self.probe.name,
                bytes >> 20,
                MAX_AUDIO_DECODE_BYTES >> 20
            )));
        }
        let window = usize::try_from(sample_rate).unwrap_or(48_000).saturating_mul(AUDIO_WINDOW_SECONDS).max(1);
        let mut planes: Vec<Vec<f32>> = Vec::new();
        let mut layout = ChannelLayout::Stereo;
        let mut done = 0usize;
        while done < total {
            let take = window.min(total - done);
            let start = i64::try_from(done)
                .ok()
                .and_then(|d| d.checked_add(first))
                .ok_or_else(|| EncoderError::Internal("audio position overflow".into()))?;
            let chunk = self.audio(start, take, sample_rate)?;
            layout = chunk.layout;
            let chunk_planes = chunk.into_planar_channels();
            if planes.is_empty() {
                planes = chunk_planes;
            } else {
                for (dst, src) in planes.iter_mut().zip(chunk_planes) {
                    dst.extend(src);
                }
            }
            done += take;
        }
        if planes.is_empty() {
            return AudioSampleBuffer::new_silent(sample_rate, layout, 0, 0);
        }
        AudioSampleBuffer::from_planar_channels(sample_rate, layout, first, planes)
    }
}

/// Probes `path` for preset evaluation. The lightweight header probe classifies the file, and the
/// real decoder's view of a moving picture (frame rate, size, length, audio) takes precedence,
/// because container timescales are not frame rates. Containers the header probe does not know
/// are accepted when the decoder can open them.
pub fn probe_media_info(path: &Path) -> Result<encodercraft_demux::MediaInfo, EncoderError> {
    use encodercraft_demux::{AudioStreamInfo, ContainerKind, MediaInfo, VideoStreamInfo};

    let header = encodercraft_demux::probe_file(path);
    let moving = match &header {
        Ok(info) => info.container.is_video() || !info.video_streams.is_empty(),
        Err(_) => true,
    };
    let reader = if moving { MediaReader::open(path).ok().filter(|r| !matches!(r.probe().kind, SourceKind::Still)) } else { None };
    let (mut info, reader) = match (header, reader) {
        (Ok(info), reader) => (info, reader),
        (Err(_), Some(reader)) => (
            MediaInfo {
                container: ContainerKind::MpegTs,
                file_size_bytes: reader.probe().file_size.unwrap_or(0),
                duration_seconds: None,
                video_streams: Vec::new(),
                audio_streams: Vec::new(),
                metadata: Default::default(),
            },
            Some(reader),
        ),
        (Err(e), None) => return Err(e),
    };
    let Some(reader) = reader else {
        return Ok(info);
    };
    let probe = reader.probe();
    info.duration_seconds = Some(probe.duration_seconds);
    if let Some(v) = &probe.video {
        let dimensions = Dimensions::new(v.width, v.height)?;
        match info.video_streams.first_mut() {
            Some(stream) => {
                stream.dimensions = dimensions;
                stream.frame_rate = v.frame_rate;
                stream.duration_seconds = Some(probe.duration_seconds);
            }
            None => info.video_streams.push(VideoStreamInfo {
                stream_index: 0,
                codec_name: v.codec.clone(),
                dimensions,
                pixel_format: PixelFormat::Yuv420p,
                color_space: ColorSpaceTag::REC709,
                frame_rate: v.frame_rate,
                bit_depth: 8,
                duration_seconds: Some(probe.duration_seconds),
            }),
        }
    }
    if info.audio_streams.is_empty() {
        for (i, a) in probe.audio.iter().enumerate() {
            info.audio_streams.push(AudioStreamInfo {
                stream_index: i + 1,
                codec_name: a.codec.clone(),
                sample_rate: a.sample_rate,
                channels: u16::try_from(a.channels).unwrap_or(u16::MAX),
                layout: layout_for(a.channels as usize),
                bits_per_sample: a.bits_per_sample.and_then(|b| u16::try_from(b).ok()).unwrap_or(16),
                duration_seconds: Some(probe.duration_seconds),
            });
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests;
