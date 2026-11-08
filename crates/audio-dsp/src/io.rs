//! Audio decoding and encoding wrappers backed by `craft-audio-io`.
//!
//! Provides decoding for compressed formats (MP3, FLAC, Ogg Vorbis, AAC, CAF, WAV, AIFF)
//! and encoding for FLAC, AIFF, and WAV.

use craft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

/// Decodes any supported audio container and format into a planar `AudioSampleBuffer`.
pub fn decode_audio(bytes: &[u8]) -> Result<AudioSampleBuffer, EncoderError> {
    // The third-party demuxers can panic on hostile headers; that must be an error, never a crash.
    let decoded = std::panic::catch_unwind(|| craft_audio_io::decode(bytes, None))
        .map_err(|_| EncoderError::InvalidContainer("audio decoding failed: the file is malformed".into()))?;
    let (_info, buf) = decoded.map_err(|e| EncoderError::Internal(format!("audio decoding failed: {e}")))?;

    let layout = match buf.channels.len() {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        6 => ChannelLayout::Surround51,
        8 => ChannelLayout::Surround71,
        _ => ChannelLayout::Stereo,
    };

    AudioSampleBuffer::from_planar_channels(buf.sample_rate, layout, 0, buf.channels)
}

/// Encodes an `AudioSampleBuffer` to lossless FLAC format.
pub fn encode_audio_flac(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions { format: FileFormat::Flac, bit_depth: BitDepth::Int24, dither: true, bwf: None };

    craft_audio_io::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("FLAC encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Apple AIFF format.
pub fn encode_audio_aiff(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions { format: FileFormat::Aiff, bit_depth: BitDepth::Int24, dither: true, bwf: None };

    craft_audio_io::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("AIFF encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to standard MPEG-1 Layer III (MP3) format with optional ID3v2 tags.
pub fn encode_audio_mp3(
    buffer: &AudioSampleBuffer,
    bitrate_kbps: u32,
    metadata: Option<&encodercraft_core::XmpMetadata>,
) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to MP3".into()));
    }

    let channels = match buffer.channel_count() {
        1 => 1u8,
        _ => 2u8, // Downmix / use first 2 channels for stereo
    };

    let sample_rate = match buffer.sample_rate {
        32_000 => 32_000,
        48_000 => 48_000,
        _ => 44_100,
    };

    let bitrate = match bitrate_kbps {
        0..=64 => audio2mp3_core::Bitrate::Kbps64,
        65..=96 => audio2mp3_core::Bitrate::Kbps96,
        97..=128 => audio2mp3_core::Bitrate::Kbps128,
        129..=160 => audio2mp3_core::Bitrate::Kbps160,
        161..=192 => audio2mp3_core::Bitrate::Kbps192,
        193..=224 => audio2mp3_core::Bitrate::Kbps224,
        225..=256 => audio2mp3_core::Bitrate::Kbps256,
        _ => audio2mp3_core::Bitrate::Kbps320,
    };

    let config = audio2mp3_core::EncoderConfig { sample_rate, channels, bitrate };
    let mut encoder = audio2mp3_core::Mp3Encoder::new(config).map_err(|e| EncoderError::Internal(format!("failed to init MP3 encoder: {e:?}")))?;

    // Convert f32 planar samples to interleaved i16 PCM
    let len = buffer.samples_per_channel();
    let mut interleaved = Vec::with_capacity(len.saturating_mul(channels as usize));

    let ch0 = buffer.channel(0).map_err(|e| EncoderError::Internal(e.to_string()))?;
    let ch1 = if channels == 2 { buffer.channel(1).unwrap_or(ch0) } else { ch0 };

    for i in 0..len {
        let s0 = (ch0.get(i).copied().unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0) as i16;
        interleaved.push(s0);
        if channels == 2 {
            let s1 = (ch1.get(i).copied().unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0) as i16;
            interleaved.push(s1);
        }
    }

    let mut out = Vec::new();

    // Inject ID3v2 tag at the start if metadata is present
    if let Some(meta) = metadata {
        let id3 = build_id3v2_tag(meta);
        out.extend_from_slice(&id3);
    }

    encoder.encode(&interleaved, &mut out).map_err(|e| EncoderError::Internal(format!("MP3 encode failed: {e:?}")))?;
    encoder.flush(&mut out).map_err(|e| EncoderError::Internal(format!("MP3 flush failed: {e:?}")))?;

    Ok(out)
}

/// Encodes an `AudioSampleBuffer` to standard Ogg Vorbis format.
pub fn encode_audio_ogg_vorbis(buffer: &AudioSampleBuffer, _quality: f32) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to Ogg Vorbis".into()));
    }

    let channels = (buffer.channel_count().min(2)) as u8;
    let mut pcm: Vec<Vec<f32>> = Vec::with_capacity(channels as usize);
    for ch in 0..channels {
        let samples = buffer.channel(ch as usize).map_err(|e| EncoderError::Internal(e.to_string()))?.to_vec();
        pcm.push(samples);
    }

    let config = oxideav_vorbis::oggfile::StreamEncoderConfig::new(buffer.sample_rate, channels);
    oxideav_vorbis::oggfile::encode_pcm_to_ogg(&pcm, &config).map_err(|e| EncoderError::Internal(format!("Ogg Vorbis encoding failed: {e:?}")))
}

fn build_id3v2_tag(meta: &encodercraft_core::XmpMetadata) -> Vec<u8> {
    let mut frames = Vec::new();

    let mut add_text_frame = |id: &[u8; 4], text: &str| {
        let mut frame_data = Vec::with_capacity(text.len() + 1);
        frame_data.push(3); // UTF-8 encoding
        frame_data.extend_from_slice(text.as_bytes());

        let frame_len = frame_data.len() as u32;
        frames.extend_from_slice(id);
        frames.extend_from_slice(&frame_len.to_be_bytes());
        frames.extend_from_slice(&[0, 0]); // Flags
        frames.extend_from_slice(&frame_data);
    };

    if let Some(ref title) = meta.title {
        add_text_frame(b"TIT2", title);
    }
    if let Some(ref creator) = meta.creator {
        add_text_frame(b"TPE1", creator);
    }
    if let Some(ref album) = meta.album {
        add_text_frame(b"TALB", album);
    }
    if let Some(track) = meta.track_number {
        let s = track.to_string();
        add_text_frame(b"TRCK", &s);
    }
    if let Some(year) = meta.year {
        let s = year.to_string();
        add_text_frame(b"TYER", &s);
    }
    if let Some(ref genre) = meta.genre {
        add_text_frame(b"TCON", genre);
    }
    if let Some(ref copyright) = meta.copyright {
        add_text_frame(b"TCOP", copyright);
    }
    if let Some(ref composer) = meta.composer {
        add_text_frame(b"TCOM", composer);
    }
    if let Some(ref isrc) = meta.isrc {
        add_text_frame(b"TSRC", isrc);
    }

    if frames.is_empty() {
        return Vec::new();
    }

    let size = frames.len() as u32;
    let synchsafe_size = [((size >> 21) & 0x7F) as u8, ((size >> 14) & 0x7F) as u8, ((size >> 7) & 0x7F) as u8, (size & 0x7F) as u8];

    let mut tag = Vec::with_capacity(10 + frames.len());
    tag.extend_from_slice(b"ID3");
    tag.push(3); // Version 2.3
    tag.push(0); // Revision
    tag.push(0); // Flags
    tag.extend_from_slice(&synchsafe_size);
    tag.extend_from_slice(&frames);
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_flac_and_aiff_encode_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 1024, 0).unwrap();
        let flac_bytes = encode_audio_flac(&buf).unwrap();
        assert!(!flac_bytes.is_empty());

        let decoded = decode_audio(&flac_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channel_count(), 2);

        let aiff_bytes = encode_audio_aiff(&buf).unwrap();
        assert!(!aiff_bytes.is_empty());
    }

    #[test]
    fn test_mp3_encode_with_id3v2_and_decode() {
        let buf = AudioSampleBuffer::new_silent(44100, ChannelLayout::Stereo, 2304, 0).unwrap();
        let meta = encodercraft_core::XmpMetadata {
            title: Some("Song Title".into()),
            creator: Some("Band Name".into()),
            album: Some("Album Name".into()),
            isrc: Some("US-ABC-26-00001".into()),
            ..Default::default()
        };

        let mp3_bytes = encode_audio_mp3(&buf, 192, Some(&meta)).unwrap();
        assert!(!mp3_bytes.is_empty());
        assert_eq!(&mp3_bytes[0..3], b"ID3");

        let decoded = decode_audio(&mp3_bytes).unwrap();
        assert_eq!(decoded.channel_count(), 2);
    }

    #[test]
    fn test_ogg_vorbis_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(44100, ChannelLayout::Stereo, 2048, 0).unwrap();
        let ogg_bytes = encode_audio_ogg_vorbis(&buf, 0.7).unwrap();
        assert!(!ogg_bytes.is_empty());
        assert_eq!(&ogg_bytes[0..4], b"OggS");

        let decoded = decode_audio(&ogg_bytes).unwrap();
        assert_eq!(decoded.channel_count(), 2);
    }
}
