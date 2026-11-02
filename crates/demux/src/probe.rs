//! High-level media probing engine extracting comprehensive stream metadata.
//!
//! Integrates container sniffing, WAV/AIFF parsing, image probers, and craft-media.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ChannelLayout, EncoderError, MediaStreamKind, Rational64};
use encodercraft_geom::Dimensions;
use serde::{Deserialize, Serialize};

use crate::aiff::parse_aiff_header;
use crate::image_probe::{probe_jpeg, probe_png, probe_webp};
use crate::sniff::{ContainerKind, sniff_container};
use crate::wav::parse_wav_header;

/// Detailed technical specifications of an elementary video stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoStreamInfo {
    pub stream_index: usize,
    pub codec_name: String,
    pub dimensions: Dimensions,
    pub pixel_format: PixelFormat,
    pub color_space: ColorSpaceTag,
    pub frame_rate: Rational64,
    pub bit_depth: u8,
    pub duration_seconds: Option<f64>,
}

/// Detailed technical specifications of an elementary audio stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub stream_index: usize,
    pub codec_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub layout: ChannelLayout,
    pub bits_per_sample: u16,
    pub duration_seconds: Option<f64>,
}

/// Comprehensive media container metadata and stream manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: ContainerKind,
    pub file_size_bytes: u64,
    pub duration_seconds: Option<f64>,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub metadata: HashMap<String, String>,
}

impl MediaInfo {
    /// Classifies the overall stream kind based on discovered elementary streams and container kind.
    pub fn stream_kind(&self) -> MediaStreamKind {
        match self.container {
            ContainerKind::Wav | ContainerKind::Rf64Wav | ContainerKind::Aiff | ContainerKind::Flac | ContainerKind::Mp3 | ContainerKind::Ogg => {
                MediaStreamKind::AudioOnly
            }
            ContainerKind::GltfBinary
            | ContainerKind::GltfJson
            | ContainerKind::UsdBinary
            | ContainerKind::UsdAscii
            | ContainerKind::Obj
            | ContainerKind::StlBinary
            | ContainerKind::StlAscii
            | ContainerKind::Ply => MediaStreamKind::Spatial3D,
            ContainerKind::Png
            | ContainerKind::Jpeg
            | ContainerKind::WebP
            | ContainerKind::Tiff
            | ContainerKind::OpenExr
            | ContainerKind::RadianceHdr
            | ContainerKind::Dds
            | ContainerKind::Psd
            | ContainerKind::Bmp => MediaStreamKind::StillImage,
            ContainerKind::Svg | ContainerKind::IllustratorAi | ContainerKind::Eps => MediaStreamKind::Vector2D,
            ContainerKind::Pdf
            | ContainerKind::Docx
            | ContainerKind::Pptx
            | ContainerKind::Xlsx
            | ContainerKind::Idml
            | ContainerKind::Indd
            | ContainerKind::Epub => MediaStreamKind::Document,
            _ => {
                let has_video = !self.video_streams.is_empty();
                let has_audio = !self.audio_streams.is_empty();
                if has_video && has_audio {
                    MediaStreamKind::VideoWithAudio
                } else if has_video {
                    MediaStreamKind::VideoOnly
                } else if has_audio {
                    MediaStreamKind::AudioOnly
                } else {
                    MediaStreamKind::Unknown
                }
            }
        }
    }
}

/// Probes or classifies the file at `path`, returning the detected `MediaStreamKind`.
///
/// Combines full stream probing, container header sniffing, and file extension fallback.
pub fn classify_stream_kind(path: &Path) -> MediaStreamKind {
    if let Ok(info) = probe_file(path) {
        let kind = info.stream_kind();
        if kind != MediaStreamKind::Unknown {
            return kind;
        }
    }

    // Fallback based on extension if probe failed or file is headless/mock
    if let Some(ext) = path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()) {
        match ext.as_str() {
            "mp3" | "wav" | "flac" | "aiff" | "aif" | "ogg" | "oga" | "m4a" | "aac" | "opus" | "alac" | "wma" => MediaStreamKind::AudioOnly,
            "glb" | "gltf" | "obj" | "stl" | "ply" | "usda" | "usdc" | "usdz" => MediaStreamKind::Spatial3D,
            "svg" | "ai" | "eps" => MediaStreamKind::Vector2D,
            "pdf" | "docx" | "idml" | "indd" | "epub" | "pptx" | "xlsx" | "rtf" => MediaStreamKind::Document,
            "png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff" | "exr" | "hdr" | "dds" | "bmp" | "psd" => MediaStreamKind::StillImage,
            "mp4" | "mov" | "mkv" | "webm" | "mxf" | "avi" | "m4v" | "mpg" | "mpeg" | "ts" => MediaStreamKind::VideoWithAudio,
            _ => MediaStreamKind::Unknown,
        }
    } else {
        MediaStreamKind::Unknown
    }
}

/// Probes an in-memory byte slice and extracts full media stream information.
pub fn probe_bytes(bytes: &[u8]) -> Result<MediaInfo, EncoderError> {
    let container =
        sniff_container(bytes).ok_or_else(|| EncoderError::InvalidContainer("unrecognized file signature / unsupported format".into()))?;

    let file_size = bytes.len() as u64;
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut metadata = HashMap::new();
    let mut duration_seconds = None;

    match container {
        ContainerKind::Wav | ContainerKind::Rf64Wav => {
            let wav = parse_wav_header(bytes)?;
            let dur = if wav.sample_rate > 0 { Some((wav.total_samples_per_channel as f64) / (wav.sample_rate as f64)) } else { None };
            duration_seconds = dur;

            let layout = match wav.channels {
                1 => ChannelLayout::Mono,
                2 => ChannelLayout::Stereo,
                6 => ChannelLayout::Surround51,
                8 => ChannelLayout::Surround71,
                _ => ChannelLayout::Stereo,
            };

            let codec_name = match wav.format {
                crate::wav::WavAudioFormat::PcmInteger => "PCM Linear Integer",
                crate::wav::WavAudioFormat::IeeeFloat => "IEEE 754 Float",
                crate::wav::WavAudioFormat::Extensible => "WAVE Extensible",
            };

            audio_streams.push(AudioStreamInfo {
                stream_index: 0,
                codec_name: codec_name.to_string(),
                sample_rate: wav.sample_rate,
                channels: wav.channels,
                layout,
                bits_per_sample: wav.bits_per_sample,
                duration_seconds: dur,
            });

            if let Some(bext) = wav.bext {
                if !bext.description.is_empty() {
                    metadata.insert("description".into(), bext.description);
                }
                if !bext.originator.is_empty() {
                    metadata.insert("originator".into(), bext.originator);
                }
                if !bext.origination_date.is_empty() {
                    metadata.insert("date".into(), bext.origination_date);
                }
                if !bext.origination_time.is_empty() {
                    metadata.insert("time".into(), bext.origination_time);
                }
            }
        }
        ContainerKind::Aiff => {
            let aiff = parse_aiff_header(bytes)?;
            let dur = if aiff.sample_rate > 0 { Some((aiff.num_sample_frames as f64) / (aiff.sample_rate as f64)) } else { None };
            duration_seconds = dur;

            let layout = match aiff.channels {
                1 => ChannelLayout::Mono,
                2 => ChannelLayout::Stereo,
                6 => ChannelLayout::Surround51,
                8 => ChannelLayout::Surround71,
                _ => ChannelLayout::Stereo,
            };

            audio_streams.push(AudioStreamInfo {
                stream_index: 0,
                codec_name: "PCM Big-Endian".to_string(),
                sample_rate: aiff.sample_rate,
                channels: aiff.channels,
                layout,
                bits_per_sample: aiff.bits_per_sample,
                duration_seconds: dur,
            });
        }
        ContainerKind::Png => {
            let img = probe_png(bytes)?;
            video_streams.push(VideoStreamInfo {
                stream_index: 0,
                codec_name: "PNG Still".into(),
                dimensions: img.dimensions,
                pixel_format: img.format,
                color_space: ColorSpaceTag::SRGB,
                frame_rate: Rational64::FPS_24,
                bit_depth: img.bit_depth,
                duration_seconds: None,
            });
        }
        ContainerKind::Jpeg => {
            let img = probe_jpeg(bytes)?;
            video_streams.push(VideoStreamInfo {
                stream_index: 0,
                codec_name: "JPEG Still".into(),
                dimensions: img.dimensions,
                pixel_format: img.format,
                color_space: ColorSpaceTag::SRGB,
                frame_rate: Rational64::FPS_24,
                bit_depth: img.bit_depth,
                duration_seconds: None,
            });
        }
        ContainerKind::WebP => {
            let img = probe_webp(bytes)?;
            video_streams.push(VideoStreamInfo {
                stream_index: 0,
                codec_name: "WebP Still".into(),
                dimensions: img.dimensions,
                pixel_format: img.format,
                color_space: ColorSpaceTag::SRGB,
                frame_rate: Rational64::FPS_24,
                bit_depth: img.bit_depth,
                duration_seconds: None,
            });
        }
        ContainerKind::IsoBmffMp4 | ContainerKind::QuickTimeMov => {
            if let Ok(mp4) = craft_media::isobmff::open(bytes) {
                populate_isobmff_streams(&mp4, &mut duration_seconds, &mut video_streams, &mut audio_streams);
            } else {
                metadata.insert("container".into(), container.display_name().into());
            }
        }
        ContainerKind::Matroska | ContainerKind::WebM => {
            if let Ok(mkv) = craft_media::matroska::open(bytes) {
                populate_matroska_streams(&mkv, &mut duration_seconds, &mut video_streams, &mut audio_streams);
            } else {
                metadata.insert("container".into(), container.display_name().into());
            }
        }
        ContainerKind::Mxf => {
            if let Ok(mxf) = filmcraft_mxf::open(bytes) {
                populate_mxf_streams(&mxf, &mut duration_seconds, &mut video_streams, &mut audio_streams, &mut metadata);
            } else {
                metadata.insert("container".into(), container.display_name().into());
            }
        }
        _ => {
            // For other formats (glTF, USD, etc.), record the detected container
            metadata.insert("container".into(), container.display_name().into());
        }
    }

    Ok(MediaInfo { container, file_size_bytes: file_size, duration_seconds, video_streams, audio_streams, metadata })
}

fn populate_isobmff_streams(
    mp4: &craft_media::isobmff::Mp4File,
    duration_seconds: &mut Option<f64>,
    video_streams: &mut Vec<VideoStreamInfo>,
    audio_streams: &mut Vec<AudioStreamInfo>,
) {
    let dur = if mp4.timescale > 0 { Some(mp4.duration as f64 / mp4.timescale as f64) } else { None };
    *duration_seconds = dur;

    for (idx, track) in mp4.tracks.iter().enumerate() {
        match track.kind {
            craft_media::isobmff::TrackKind::Video => {
                if let Some(v) = track.video()
                    && let Ok(dims) = Dimensions::new(v.width as u32, v.height as u32)
                {
                    let codec_str = track.codec().map(|c| c.name().to_string()).unwrap_or_else(|| "video".to_string());
                    let bit_depth = if v.depth > 0 { (v.depth & 0xFF) as u8 } else { 8 };
                    video_streams.push(VideoStreamInfo {
                        stream_index: idx,
                        codec_name: codec_str,
                        dimensions: dims,
                        pixel_format: PixelFormat::Yuv420p,
                        color_space: ColorSpaceTag::REC709,
                        frame_rate: Rational64::new(track.timescale as i64, 1).unwrap_or(Rational64::FPS_24),
                        bit_depth,
                        duration_seconds: dur,
                    });
                }
            }
            craft_media::isobmff::TrackKind::Audio => {
                if let Some(a) = track.audio() {
                    let layout = match a.channels {
                        1 => ChannelLayout::Mono,
                        2 => ChannelLayout::Stereo,
                        6 => ChannelLayout::Surround51,
                        8 => ChannelLayout::Surround71,
                        _ => ChannelLayout::Stereo,
                    };
                    let codec_str = track.codec().map(|c| c.name().to_string()).unwrap_or_else(|| "audio".to_string());
                    audio_streams.push(AudioStreamInfo {
                        stream_index: idx,
                        codec_name: codec_str,
                        sample_rate: a.sample_rate as u32,
                        channels: a.channels as u16,
                        layout,
                        bits_per_sample: a.sample_size,
                        duration_seconds: dur,
                    });
                }
            }
            _ => {}
        }
    }
}

fn populate_matroska_streams(
    mkv: &craft_media::matroska::MkvFile,
    duration_seconds: &mut Option<f64>,
    video_streams: &mut Vec<VideoStreamInfo>,
    audio_streams: &mut Vec<AudioStreamInfo>,
) {
    let dur = mkv.duration_ns().map(|ns| ns as f64 / 1_000_000_000.0);
    *duration_seconds = dur;

    for (idx, track) in mkv.tracks.iter().enumerate() {
        match track.kind {
            craft_media::matroska::TrackKind::Video => {
                if let Some(v) = &track.video
                    && let Ok(dims) = Dimensions::new(v.pixel_width, v.pixel_height)
                {
                    video_streams.push(VideoStreamInfo {
                        stream_index: idx,
                        codec_name: format!("{:?}", track.codec),
                        dimensions: dims,
                        pixel_format: PixelFormat::Yuv420p,
                        color_space: ColorSpaceTag::REC709,
                        frame_rate: Rational64::FPS_24,
                        bit_depth: 8,
                        duration_seconds: dur,
                    });
                }
            }
            craft_media::matroska::TrackKind::Audio => {
                if let Some(a) = &track.audio {
                    let layout = match a.channels {
                        1 => ChannelLayout::Mono,
                        2 => ChannelLayout::Stereo,
                        6 => ChannelLayout::Surround51,
                        8 => ChannelLayout::Surround71,
                        _ => ChannelLayout::Stereo,
                    };
                    audio_streams.push(AudioStreamInfo {
                        stream_index: idx,
                        codec_name: format!("{:?}", track.codec),
                        sample_rate: a.sampling_frequency as u32,
                        channels: a.channels as u16,
                        layout,
                        bits_per_sample: a.bit_depth.unwrap_or(16) as u16,
                        duration_seconds: dur,
                    });
                }
            }
            _ => {}
        }
    }
}

fn populate_mxf_streams(
    mxf: &filmcraft_mxf::MxfFile,
    duration_seconds: &mut Option<f64>,
    video_streams: &mut Vec<VideoStreamInfo>,
    audio_streams: &mut Vec<AudioStreamInfo>,
    metadata: &mut HashMap<String, String>,
) {
    if let Some(ref pkg) = mxf.material_package_name {
        metadata.insert("package_name".into(), pkg.clone());
    }
    if let Some(tc) = mxf.timecode {
        metadata.insert("start_timecode".into(), tc.format());
    }
    for (idx, track) in mxf.tracks.iter().enumerate() {
        let fps = if track.edit_rate.den > 0 && track.edit_rate.num > 0 {
            Rational64::new(i64::from(track.edit_rate.num), i64::from(track.edit_rate.den)).unwrap_or(Rational64::FPS_24)
        } else {
            Rational64::FPS_24
        };
        let dur = track.duration.map(|d| (d as f64) / fps.as_f64());
        if duration_seconds.is_none() {
            *duration_seconds = dur;
        }
        match track.kind {
            filmcraft_mxf::TrackKind::Picture => {
                if let Some(pic) = &track.picture
                    && let Ok(dims) = Dimensions::new(pic.frame_width(), pic.frame_height())
                {
                    let bit_depth = u8::try_from(pic.component_depth).unwrap_or(8).max(8);
                    let pix_fmt = if bit_depth >= 10 { PixelFormat::Yuv422p10 } else { PixelFormat::Yuv422p };
                    video_streams.push(VideoStreamInfo {
                        stream_index: idx,
                        codec_name: track.codec.name().to_string(),
                        dimensions: dims,
                        pixel_format: pix_fmt,
                        color_space: ColorSpaceTag::REC709,
                        frame_rate: fps,
                        bit_depth,
                        duration_seconds: dur,
                    });
                }
            }
            filmcraft_mxf::TrackKind::Sound => {
                if let Some(snd) = &track.sound {
                    let ch = u16::try_from(snd.channels).unwrap_or(2).max(1);
                    let layout = match ch {
                        1 => ChannelLayout::Mono,
                        2 => ChannelLayout::Stereo,
                        6 => ChannelLayout::Surround51,
                        8 => ChannelLayout::Surround71,
                        _ => ChannelLayout::Stereo,
                    };
                    let sr = if snd.sample_rate.den > 0 {
                        u32::try_from((snd.sample_rate.num / snd.sample_rate.den).max(8000)).unwrap_or(48_000)
                    } else {
                        48_000
                    };
                    audio_streams.push(AudioStreamInfo {
                        stream_index: idx,
                        codec_name: track.codec.name().to_string(),
                        sample_rate: sr,
                        channels: ch,
                        layout,
                        bits_per_sample: u16::try_from(snd.bits).unwrap_or(16),
                        duration_seconds: dur,
                    });
                }
            }
            filmcraft_mxf::TrackKind::Data => {}
        }
    }
}

/// Probes a media file on disk, reading only the necessary header bytes.
pub fn probe_file(path: &Path) -> Result<MediaInfo, EncoderError> {
    let mut file = File::open(path).map_err(|e| EncoderError::Io(format!("failed to open media file `{}`: {e}", path.display())))?;

    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    // Read initial 64 KB header buffer for rapid probing
    let mut header_buf = vec![0u8; 64 * 1024];
    let bytes_read =
        file.read(&mut header_buf).map_err(|e| EncoderError::Io(format!("failed to read media header from `{}`: {e}", path.display())))?;
    header_buf.truncate(bytes_read);

    let mut info = probe_bytes(&header_buf)?;
    info.file_size_bytes = file_size;

    // If streams were not found in the initial header chunk (e.g. `moov` at file end), open full file source
    if info.video_streams.is_empty() && info.audio_streams.is_empty() {
        if matches!(info.container, ContainerKind::IsoBmffMp4 | ContainerKind::QuickTimeMov)
            && let Ok(file_handle) = File::open(path)
            && let Ok(mp4) = craft_media::isobmff::open(file_handle)
        {
            populate_isobmff_streams(&mp4, &mut info.duration_seconds, &mut info.video_streams, &mut info.audio_streams);
        } else if matches!(info.container, ContainerKind::Matroska | ContainerKind::WebM)
            && let Ok(file_handle) = File::open(path)
            && let Ok(mkv) = craft_media::matroska::open(file_handle)
        {
            populate_matroska_streams(&mkv, &mut info.duration_seconds, &mut info.video_streams, &mut info.audio_streams);
        } else if info.container == ContainerKind::Mxf
            && let Ok(full_bytes) = std::fs::read(path)
            && let Ok(mxf) = filmcraft_mxf::open(full_bytes.as_slice())
        {
            populate_mxf_streams(&mxf, &mut info.duration_seconds, &mut info.video_streams, &mut info.audio_streams, &mut info.metadata);
        }
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_wav_bytes() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 8u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // Stereo
        wav.extend_from_slice(&48000u32.to_le_bytes()); // 48 kHz
        wav.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&48000u32.to_le_bytes()); // 48000 bytes = 12000 samples = 0.25s
        wav.extend_from_slice(&vec![0u8; 48000]);

        let info = probe_bytes(&wav).unwrap();
        assert_eq!(info.container, ContainerKind::Wav);
        assert_eq!(info.audio_streams.len(), 1);
        let audio = &info.audio_streams[0];
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.channels, 2);
        assert!((info.duration_seconds.unwrap() - 0.25).abs() < 1e-4);
        assert_eq!(info.stream_kind(), MediaStreamKind::AudioOnly);
    }

    #[test]
    fn test_classify_stream_kind() {
        assert_eq!(classify_stream_kind(Path::new("song.mp3")), MediaStreamKind::AudioOnly);
        assert_eq!(classify_stream_kind(Path::new("voice.wav")), MediaStreamKind::AudioOnly);
        assert_eq!(classify_stream_kind(Path::new("track.flac")), MediaStreamKind::AudioOnly);
        assert_eq!(classify_stream_kind(Path::new("track.ogg")), MediaStreamKind::AudioOnly);
        assert_eq!(classify_stream_kind(Path::new("model.glb")), MediaStreamKind::Spatial3D);
        assert_eq!(classify_stream_kind(Path::new("scene.gltf")), MediaStreamKind::Spatial3D);
        assert_eq!(classify_stream_kind(Path::new("picture.png")), MediaStreamKind::StillImage);
        assert_eq!(classify_stream_kind(Path::new("photo.psd")), MediaStreamKind::StillImage);
        assert_eq!(classify_stream_kind(Path::new("icon.svg")), MediaStreamKind::Vector2D);
        assert_eq!(classify_stream_kind(Path::new("logo.ai")), MediaStreamKind::Vector2D);
        assert_eq!(classify_stream_kind(Path::new("drawing.eps")), MediaStreamKind::Vector2D);
        assert_eq!(classify_stream_kind(Path::new("document.pdf")), MediaStreamKind::Document);
        assert_eq!(classify_stream_kind(Path::new("proposal.docx")), MediaStreamKind::Document);
        assert_eq!(classify_stream_kind(Path::new("catalog.idml")), MediaStreamKind::Document);
        assert_eq!(classify_stream_kind(Path::new("book.epub")), MediaStreamKind::Document);
        assert_eq!(classify_stream_kind(Path::new("clip.mp4")), MediaStreamKind::VideoWithAudio);
        assert_eq!(classify_stream_kind(Path::new("movie.mov")), MediaStreamKind::VideoWithAudio);
    }
}
