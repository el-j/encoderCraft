//! High-level media probing engine extracting comprehensive stream metadata.
//!
//! Integrates container sniffing, WAV/AIFF parsing, image probers, and craft-media.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ChannelLayout, EncoderError, Rational64};
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
        _ => {
            // For other formats (MP4, MKV, glTF, USD), record the detected container
            metadata.insert("container".into(), container.display_name().into());
        }
    }

    Ok(MediaInfo { container, file_size_bytes: file_size, duration_seconds, video_streams, audio_streams, metadata })
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
    }
}
