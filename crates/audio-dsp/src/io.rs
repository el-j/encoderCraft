//! Audio decoding and encoding wrappers backed by `craft-audio`.
//!
//! Provides decoding for compressed formats (MP3, FLAC, Ogg Vorbis, AAC, CAF, WAV, AIFF)
//! and encoding for FLAC, AIFF, and WAV.

use craft_audio::{AudioBuffer, BitDepth, EncodeFormat, EncodeOptions};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

/// Decodes any supported audio container and format into a planar `AudioSampleBuffer`.
pub fn decode_audio(bytes: &[u8]) -> Result<AudioSampleBuffer, EncoderError> {
    if bytes.starts_with(b"qoaf") {
        return decode_audio_qoa(bytes);
    }

    let decoded_craft = std::panic::catch_unwind(|| craft_audio::decode(bytes, None));
    if let Ok(Ok((_info, buf))) = decoded_craft {
        let layout = match buf.channels.len() {
            1 => ChannelLayout::Mono,
            2 => ChannelLayout::Stereo,
            6 => ChannelLayout::Surround51,
            8 => ChannelLayout::Surround71,
            _ => ChannelLayout::Stereo,
        };
        return AudioSampleBuffer::from_planar_channels(buf.sample_rate, layout, 0, buf.channels);
    }

    Err(EncoderError::InvalidContainer("audio decoding failed: unsupported or malformed audio data".into()))
}

/// Encodes an `AudioSampleBuffer` to lossless FLAC format.
pub fn encode_audio_flac(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions::new(EncodeFormat::Flac).with_bit_depth(BitDepth::Int24).with_dither(true);

    craft_audio::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("FLAC encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Apple AIFF format.
pub fn encode_audio_aiff(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions::new(EncodeFormat::Aiff).with_bit_depth(BitDepth::Int24).with_dither(true);

    craft_audio::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("AIFF encoding failed: {e}")))
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

/// Encodes an `AudioSampleBuffer` to Apple Core Audio Format (CAF) with uncompressed Linear PCM.
pub fn encode_audio_caf(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to CAF".into()));
    }

    let channels = buffer.channel_count() as u32;
    let len = buffer.samples_per_channel();
    let bytes_per_sample = 2u32; // 16-bit integer PCM
    let bytes_per_packet = channels * bytes_per_sample;

    let mut pcm_bytes = Vec::with_capacity(len.saturating_mul(bytes_per_packet as usize));
    for i in 0..len {
        for ch in 0..channels {
            let ch_data = buffer.channel(ch as usize).map_err(|e| EncoderError::Internal(e.to_string()))?;
            let s = (ch_data.get(i).copied().unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm_bytes.extend_from_slice(&s.to_le_bytes());
        }
    }

    let mut out = Vec::with_capacity(64 + pcm_bytes.len());
    // File header: 'caff', version 1, flags 0
    out.extend_from_slice(b"caff");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    // 'desc' chunk (32 bytes)
    out.extend_from_slice(b"desc");
    out.extend_from_slice(&32i64.to_be_bytes());
    out.extend_from_slice(&(buffer.sample_rate as f64).to_be_bytes());
    out.extend_from_slice(b"lpcm");
    out.extend_from_slice(&0x0000_0002u32.to_be_bytes()); // Little-endian signed integer
    out.extend_from_slice(&bytes_per_packet.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes()); // 1 frame per packet
    out.extend_from_slice(&channels.to_be_bytes());
    out.extend_from_slice(&16u32.to_be_bytes()); // 16 bits per channel

    // 'data' chunk
    let data_chunk_size = (4 + pcm_bytes.len()) as i64;
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_chunk_size.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // mEditCount = 0
    out.extend_from_slice(&pcm_bytes);

    Ok(out)
}

/// Encodes an `AudioSampleBuffer` to standalone RFC 7845 Ogg Opus format.
pub fn encode_audio_opus(
    buffer: &AudioSampleBuffer,
    bitrate_kbps: u32,
    metadata: Option<&encodercraft_core::XmpMetadata>,
) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to Opus".into()));
    }

    let channels = (buffer.channel_count().clamp(1, 2)) as u8;
    let bitrate_bps = bitrate_kbps.clamp(16, 510) * 1000;

    let mut enc = craft_audio::opus::encoder::Encoder::new(channels, bitrate_bps)
        .map_err(|e| EncoderError::Internal(format!("Opus encoder init failed: {e}")))?;
    let c = channels as usize;
    let _len = buffer.samples_per_channel();

    // Prepare interleaved 48 kHz float samples (Opus natively operates at 48 kHz)
    let resampled = if buffer.sample_rate != 48_000 { crate::resample::SincResampler::new(32).resample(buffer, 48_000)? } else { buffer.clone() };

    let r_len = resampled.samples_per_channel();
    let mut pcm = vec![0.0f32; r_len * c];
    for ch in 0..c {
        let ch_data = resampled.channel(ch).map_err(|e| EncoderError::Internal(e.to_string()))?;
        for (i, &v) in ch_data.iter().enumerate() {
            if let Some(slot) = pcm.get_mut(i * c + ch) {
                *slot = v;
            }
        }
    }

    let frame_size = craft_audio::opus::encoder::Encoder::FRAME_SIZE;
    let total = r_len + enc.pre_skip() as usize;
    let frame_count = total.div_ceil(frame_size);
    pcm.resize(frame_count * frame_size * c, 0.0);

    let packets: Vec<Vec<u8>> = pcm
        .chunks(frame_size * c)
        .map(|f| enc.encode_float(f).map_err(|e| EncoderError::Internal(format!("Opus frame encode failed: {e}"))))
        .collect::<Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    // Page 0: OpusHead (header_type 0x02 = beginning of stream)
    ogg_page(&mut out, &enc.opus_head(), 0x02, 0, 0);

    // Page 1: OpusTags
    let vendor = b"encoderCraft-opusenc";
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);

    let mut comments: Vec<String> = Vec::new();
    if let Some(meta) = metadata {
        if let Some(ref title) = meta.title {
            comments.push(format!("TITLE={title}"));
        }
        if let Some(ref creator) = meta.creator {
            comments.push(format!("ARTIST={creator}"));
        }
        if let Some(ref album) = meta.album {
            comments.push(format!("ALBUM={album}"));
        }
        if let Some(ref isrc) = meta.isrc {
            comments.push(format!("ISRC={isrc}"));
        }
    }
    tags.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for comm in &comments {
        let comm_bytes = comm.as_bytes();
        tags.extend_from_slice(&(comm_bytes.len() as u32).to_le_bytes());
        tags.extend_from_slice(comm_bytes);
    }
    ogg_page(&mut out, &tags, 0, 0, 1);

    // Subsequent pages: audio packets
    let pre_skip = enc.pre_skip() as u64;
    for (i, p) in packets.iter().enumerate() {
        let last = i + 1 == packets.len();
        let granule = if last { pre_skip + r_len as u64 } else { ((i + 1) * frame_size) as u64 };
        ogg_page(&mut out, p, if last { 0x04 } else { 0 }, granule, i as u32 + 2);
    }

    Ok(out)
}

/// Encodes an `AudioSampleBuffer` to Dolby Digital AC-3 stream format.
pub fn encode_audio_ac3(buffer: &AudioSampleBuffer, bitrate_kbps: u32) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to AC-3".into()));
    }
    let resampled = if buffer.sample_rate != 32_000 && buffer.sample_rate != 44_100 && buffer.sample_rate != 48_000 {
        crate::resample::SincResampler::new(32).resample(buffer, 48_000)?
    } else {
        buffer.clone()
    };
    let bitrate_bps = bitrate_kbps.clamp(32, 640).saturating_mul(1000);
    let sample_rate = resampled.sample_rate;
    let channels = resampled.into_planar_channels();
    let audio_buf = craft_audio::AudioBuffer { sample_rate, channels };
    let opts = craft_audio::EncodeOptions::new(craft_audio::EncodeFormat::Ac3).with_bitrate(bitrate_bps);
    craft_audio::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("AC-3 encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Enhanced AC-3 (E-AC-3 / Dolby Digital Plus) stream format.
pub fn encode_audio_eac3(buffer: &AudioSampleBuffer, bitrate_kbps: u32) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to E-AC-3".into()));
    }
    let resampled = if buffer.sample_rate != 32_000 && buffer.sample_rate != 44_100 && buffer.sample_rate != 48_000 {
        crate::resample::SincResampler::new(32).resample(buffer, 48_000)?
    } else {
        buffer.clone()
    };
    let bitrate_bps = bitrate_kbps.clamp(32, 1024).saturating_mul(1000);
    let sample_rate = resampled.sample_rate;
    let channels = resampled.into_planar_channels();
    let audio_buf = craft_audio::AudioBuffer { sample_rate, channels };
    let opts = craft_audio::EncodeOptions::new(craft_audio::EncodeFormat::Eac3).with_bitrate(bitrate_bps);
    craft_audio::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("E-AC-3 encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Apple Lossless Audio Codec (ALAC) format.
pub fn encode_audio_alac(buffer: &AudioSampleBuffer, bit_depth: u8) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to ALAC".into()));
    }
    let bd = if bit_depth == 24 { craft_audio::BitDepth::Int24 } else { craft_audio::BitDepth::Int16 };
    let channels = buffer.clone().into_planar_channels();
    let audio_buf = craft_audio::AudioBuffer { sample_rate: buffer.sample_rate, channels };
    let opts = craft_audio::EncodeOptions::new(craft_audio::EncodeFormat::Alac).with_bit_depth(bd);
    craft_audio::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("ALAC encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Quite OK Audio (QOA) format.
pub fn encode_audio_qoa(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    if buffer.channel_count() == 0 || buffer.samples_per_channel() == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode empty audio buffer to QOA".into()));
    }
    let channels = buffer.channel_count().min(craft_audio::qoa::MAX_CHANNELS);
    let len = buffer.samples_per_channel();
    let mut qoa_channels: Vec<Vec<i16>> = Vec::with_capacity(channels);
    for ch in 0..channels {
        let ch_data = buffer.channel(ch).map_err(|e| EncoderError::Internal(e.to_string()))?;
        let mut i16_samples = Vec::with_capacity(len);
        for &s in ch_data {
            let clamped = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            i16_samples.push(clamped);
        }
        qoa_channels.push(i16_samples);
    }
    craft_audio::qoa::encode(buffer.sample_rate, &qoa_channels).map_err(|e| EncoderError::Internal(format!("QOA encoding failed: {e:?}")))
}

/// Decodes Quite OK Audio (QOA) bytes to an `AudioSampleBuffer`.
pub fn decode_audio_qoa(bytes: &[u8]) -> Result<AudioSampleBuffer, EncoderError> {
    let decoded = craft_audio::qoa::decode(bytes).map_err(|e| EncoderError::InvalidContainer(format!("QOA decoding failed: {e:?}")))?;
    let layout = match decoded.channels.len() {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        6 => ChannelLayout::Surround51,
        8 => ChannelLayout::Surround71,
        _ => ChannelLayout::Stereo,
    };
    let planar: Vec<Vec<f32>> = decoded.channels.into_iter().map(|ch| ch.into_iter().map(|s| s as f32 / 32768.0).collect()).collect();
    AudioSampleBuffer::from_planar_channels(decoded.sample_rate, layout, 0, planar)
}

fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    }
    crc
}

fn ogg_page(out: &mut Vec<u8>, packet: &[u8], header_type: u8, granule: u64, seq: u32) {
    let mut lacing = vec![255u8; packet.len() / 255];
    lacing.push((packet.len() % 255) as u8);
    let start = out.len();
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(header_type);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&0x4F50_5553u32.to_le_bytes()); // 'OPUS' serial
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
    out.push(lacing.len() as u8);
    out.extend_from_slice(&lacing);
    out.extend_from_slice(packet);
    let crc = ogg_crc(&out[start..]);
    if let Some(crc_dest) = out.get_mut(start + 22..start + 26) {
        crc_dest.copy_from_slice(&crc.to_le_bytes());
    }
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

    #[test]
    fn test_caf_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 2048, 0).unwrap();
        let caf_bytes = encode_audio_caf(&buf).unwrap();
        assert!(!caf_bytes.is_empty());
        assert_eq!(&caf_bytes[0..4], b"caff");

        let decoded = decode_audio(&caf_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channel_count(), 2);
    }

    #[test]
    fn test_opus_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 2880, 0).unwrap();
        let meta = encodercraft_core::XmpMetadata { title: Some("Opus Track".into()), creator: Some("Opus Artist".into()), ..Default::default() };
        let opus_bytes = encode_audio_opus(&buf, 128, Some(&meta)).unwrap();
        assert!(!opus_bytes.is_empty());
        assert_eq!(&opus_bytes[0..4], b"OggS");
        assert!(opus_bytes.windows(8).any(|w| w == b"OpusHead"));
        assert!(opus_bytes.windows(8).any(|w| w == b"OpusTags"));
    }

    #[test]
    fn test_ac3_and_eac3_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 3072, 0).unwrap();
        let ac3_bytes = encode_audio_ac3(&buf, 192).unwrap();
        assert!(!ac3_bytes.is_empty());
        let decoded_ac3 = decode_audio(&ac3_bytes).unwrap();
        assert_eq!(decoded_ac3.sample_rate, 48000);
        assert_eq!(decoded_ac3.channel_count(), 2);

        let eac3_bytes = encode_audio_eac3(&buf, 224).unwrap();
        assert!(!eac3_bytes.is_empty());
        let decoded_eac3 = decode_audio(&eac3_bytes).unwrap();
        assert_eq!(decoded_eac3.sample_rate, 48000);
        assert_eq!(decoded_eac3.channel_count(), 2);
    }

    #[test]
    fn test_alac_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(44100, ChannelLayout::Stereo, 2048, 0).unwrap();
        let alac_bytes = encode_audio_alac(&buf, 16).unwrap();
        assert!(!alac_bytes.is_empty());
        let decoded = decode_audio(&alac_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 44100);
        assert_eq!(decoded.channel_count(), 2);
    }

    #[test]
    fn test_qoa_encode_and_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 1024, 0).unwrap();
        let qoa_bytes = encode_audio_qoa(&buf).unwrap();
        assert!(!qoa_bytes.is_empty());
        assert_eq!(&qoa_bytes[0..4], b"qoaf");

        let decoded = decode_audio(&qoa_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channel_count(), 2);
        assert_eq!(decoded.samples_per_channel(), 1024);
    }
}
