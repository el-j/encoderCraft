//! Clean-room parser for RIFF WAVE and RF64 (Broadcast Wave >4GB) audio files.
//!
//! Enforces bounded allocations, safe slice indexing, and checked arithmetic.

use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

/// Audio format tag from the `fmt ` chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavAudioFormat {
    PcmInteger = 1,
    IeeeFloat = 3,
    Extensible = 0xFFFE,
}

/// Broadcast Audio Extension (`bext` chunk) metadata from Broadcast Wave files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastWaveMetadata {
    pub description: String,
    pub originator: String,
    pub originator_reference: String,
    pub origination_date: String,
    pub origination_time: String,
    pub time_reference_samples: u64,
}

/// Parsed metadata and chunk layout from a WAVE file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavHeaderInfo {
    pub format: WavAudioFormat,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub byte_rate: u32,
    pub block_align: u16,
    pub data_offset: usize,
    pub data_bytes: u64,
    pub total_samples_per_channel: u64,
    pub bext: Option<BroadcastWaveMetadata>,
    pub is_rf64: bool,
}

/// Parses the header of a WAV / RF64 buffer.
pub fn parse_wav_header(bytes: &[u8]) -> Result<WavHeaderInfo, EncoderError> {
    if bytes.len() < 12 {
        return Err(EncoderError::InvalidContainer("WAV file too short for RIFF header".into()));
    }

    let is_rf64 = bytes.starts_with(b"RF64");
    if !bytes.starts_with(b"RIFF") && !is_rf64 {
        return Err(EncoderError::InvalidContainer("not a RIFF or RF64 file".into()));
    }

    let form_type = bytes.get(8..12).ok_or_else(|| EncoderError::InvalidContainer("unexpected end of file reading RIFF form type".into()))?;
    if form_type != b"WAVE" {
        return Err(EncoderError::InvalidContainer(format!("expected WAVE form type, got `{}`", String::from_utf8_lossy(form_type))));
    }

    let mut cursor: usize = 12;
    let mut fmt_info: Option<(WavAudioFormat, u16, u32, u32, u16, u16)> = None;
    let mut data_info: Option<(usize, u64)> = None;
    let mut bext_info: Option<BroadcastWaveMetadata> = None;
    let mut rf64_data_size: Option<u64> = None;

    while cursor.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let chunk_id = bytes.get(cursor..cursor + 4).ok_or_else(|| EncoderError::InvalidContainer("error reading chunk id".into()))?;
        let chunk_size_u32 = u32::from_le_bytes([
            bytes.get(cursor + 4).copied().unwrap_or(0),
            bytes.get(cursor + 5).copied().unwrap_or(0),
            bytes.get(cursor + 6).copied().unwrap_or(0),
            bytes.get(cursor + 7).copied().unwrap_or(0),
        ]);

        let chunk_data_offset = cursor + 8;
        let mut chunk_size = chunk_size_u32 as u64;

        if is_rf64 && chunk_id == b"ds64" {
            // RF64 64-bit size chunk:
            // riffSize (8B), dataSize (8B), sampleCount (8B), tableLength (4B)
            if chunk_data_offset.checked_add(24).is_some_and(|end| end <= bytes.len()) {
                let data_size_bytes = [
                    bytes.get(chunk_data_offset + 8).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 9).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 10).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 11).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 12).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 13).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 14).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 15).copied().unwrap_or(0),
                ];
                rf64_data_size = Some(u64::from_le_bytes(data_size_bytes));
            }
        } else if chunk_id == b"fmt " {
            if chunk_data_offset.checked_add(16).is_some_and(|end| end <= bytes.len()) {
                let format_tag =
                    u16::from_le_bytes([bytes.get(chunk_data_offset).copied().unwrap_or(0), bytes.get(chunk_data_offset + 1).copied().unwrap_or(0)]);
                let channels = u16::from_le_bytes([
                    bytes.get(chunk_data_offset + 2).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 3).copied().unwrap_or(0),
                ]);
                let sample_rate = u32::from_le_bytes([
                    bytes.get(chunk_data_offset + 4).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 5).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 6).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 7).copied().unwrap_or(0),
                ]);
                let byte_rate = u32::from_le_bytes([
                    bytes.get(chunk_data_offset + 8).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 9).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 10).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 11).copied().unwrap_or(0),
                ]);
                let block_align = u16::from_le_bytes([
                    bytes.get(chunk_data_offset + 12).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 13).copied().unwrap_or(0),
                ]);
                let bits_per_sample = u16::from_le_bytes([
                    bytes.get(chunk_data_offset + 14).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 15).copied().unwrap_or(0),
                ]);

                let audio_fmt = match format_tag {
                    1 => WavAudioFormat::PcmInteger,
                    3 => WavAudioFormat::IeeeFloat,
                    0xFFFE => WavAudioFormat::Extensible,
                    other => {
                        return Err(EncoderError::UnsupportedCodec(format!("unsupported WAVE compression format tag 0x{other:04X}")));
                    }
                };

                fmt_info = Some((audio_fmt, channels, sample_rate, byte_rate, block_align, bits_per_sample));
            }
        } else if chunk_id == b"data" {
            if is_rf64 && chunk_size == 0xFFFF_FFFF {
                chunk_size = rf64_data_size.unwrap_or(0);
            }
            data_info = Some((chunk_data_offset, chunk_size));
            // When we find data, we can stop or continue
            break;
        } else if chunk_id == b"bext" && chunk_data_offset.checked_add(412).is_some_and(|end| end <= bytes.len()) {
            let parse_str = |start: usize, len: usize| -> String {
                let s_bytes = bytes.get(start..start + len).unwrap_or(&[]);
                let null_end = s_bytes.iter().position(|&b| b == 0).unwrap_or(s_bytes.len());
                String::from_utf8_lossy(&s_bytes[..null_end]).trim().to_string()
            };

            let desc = parse_str(chunk_data_offset, 256);
            let originator = parse_str(chunk_data_offset + 256, 32);
            let orig_ref = parse_str(chunk_data_offset + 288, 32);
            let date = parse_str(chunk_data_offset + 320, 10);
            let time = parse_str(chunk_data_offset + 330, 8);

            let time_ref_low = u32::from_le_bytes([
                bytes.get(chunk_data_offset + 338).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 339).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 340).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 341).copied().unwrap_or(0),
            ]) as u64;
            let time_ref_high = u32::from_le_bytes([
                bytes.get(chunk_data_offset + 342).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 343).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 344).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 345).copied().unwrap_or(0),
            ]) as u64;
            let time_reference_samples = (time_ref_high << 32) | time_ref_low;

            bext_info = Some(BroadcastWaveMetadata {
                description: desc,
                originator,
                originator_reference: orig_ref,
                origination_date: date,
                origination_time: time,
                time_reference_samples,
            });
        }

        // Chunks are padded to word boundaries (2 bytes)
        let padded_size = (chunk_size + 1) & !1;
        let next_cursor = cursor
            .checked_add(8)
            .and_then(|c| c.checked_add(padded_size as usize))
            .ok_or_else(|| EncoderError::InvalidContainer("overflow calculating next chunk offset".into()))?;

        cursor = next_cursor;
    }

    let (audio_fmt, channels, sample_rate, byte_rate, block_align, bits_per_sample) =
        fmt_info.ok_or_else(|| EncoderError::InvalidContainer("WAV file missing required `fmt ` chunk".into()))?;

    let (data_offset, data_bytes) = data_info.ok_or_else(|| EncoderError::InvalidContainer("WAV file missing required `data` chunk".into()))?;

    let bytes_per_frame = block_align.max(1) as u64;
    let total_samples = data_bytes / bytes_per_frame;

    Ok(WavHeaderInfo {
        format: audio_fmt,
        channels,
        sample_rate,
        bits_per_sample,
        byte_rate,
        block_align,
        data_offset,
        data_bytes,
        total_samples_per_channel: total_samples,
        bext: bext_info,
        is_rf64,
    })
}

/// Decodes interleaved PCM WAV data bytes into an `AudioSampleBuffer`.
pub fn decode_wav_samples(header: &WavHeaderInfo, bytes: &[u8], max_samples: usize) -> Result<AudioSampleBuffer, EncoderError> {
    let layout = match header.channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        6 => ChannelLayout::Surround51,
        8 => ChannelLayout::Surround71,
        other => {
            return Err(EncoderError::UnsupportedCodec(format!("unsupported channel count {other}")));
        }
    };

    let sample_data = bytes.get(header.data_offset..).ok_or_else(|| EncoderError::InvalidContainer("data chunk offset out of range".into()))?;

    let bytes_per_sample = (header.bits_per_sample / 8) as usize;
    if bytes_per_sample == 0 {
        return Err(EncoderError::InvalidContainer(format!("unsupported WAV sample size of {} bits", header.bits_per_sample)));
    }
    let frame_bytes =
        (header.channels as usize).checked_mul(bytes_per_sample).ok_or(EncoderError::Internal("arithmetic overflow in frame bytes".into()))?;
    if frame_bytes == 0 {
        return Err(EncoderError::InvalidContainer("WAV declares no channels".into()));
    }

    let available_frames = sample_data.len() / frame_bytes.max(1);
    let frames_to_read = available_frames.min(max_samples);

    let channel_count = header.channels as usize;
    let mut planar = vec![vec![0.0f32; frames_to_read]; channel_count];

    for (frame_idx, chunk) in sample_data.chunks_exact(frame_bytes).take(frames_to_read).enumerate() {
        for (ch_idx, ch_bytes) in chunk.chunks_exact(bytes_per_sample).take(channel_count).enumerate() {
            let sample_f32 = match (header.format, header.bits_per_sample) {
                (WavAudioFormat::PcmInteger, 16) => {
                    let s = i16::from_le_bytes([ch_bytes.first().copied().unwrap_or(0), ch_bytes.get(1).copied().unwrap_or(0)]);
                    (s as f32) / 32768.0
                }
                (WavAudioFormat::PcmInteger, 24) => {
                    let b0 = ch_bytes.first().copied().unwrap_or(0) as i32;
                    let b1 = ch_bytes.get(1).copied().unwrap_or(0) as i32;
                    let b2 = ch_bytes.get(2).copied().unwrap_or(0) as i32;
                    let mut s = (b2 << 24) | (b1 << 16) | (b0 << 8);
                    s >>= 8; // sign extend
                    (s as f32) / 8388608.0
                }
                (WavAudioFormat::IeeeFloat, 32) => f32::from_le_bytes([
                    ch_bytes.first().copied().unwrap_or(0),
                    ch_bytes.get(1).copied().unwrap_or(0),
                    ch_bytes.get(2).copied().unwrap_or(0),
                    ch_bytes.get(3).copied().unwrap_or(0),
                ]),
                _ => 0.0,
            };

            if let Some(slot) = planar.get_mut(ch_idx).and_then(|ch| ch.get_mut(frame_idx)) {
                *slot = sample_f32;
            }
        }
    }

    AudioSampleBuffer::from_planar_channels(header.sample_rate, layout, 0, planar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wav_header() {
        // Build a synthetic 16-bit 44.1kHz Stereo WAV header
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 8u32).to_le_bytes()); // size
        wav.extend_from_slice(b"WAVE");
        // fmt chunk
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // fmt size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // 2 channels
        wav.extend_from_slice(&44100u32.to_le_bytes()); // 44.1 kHz
        wav.extend_from_slice(&(44100u32 * 4).to_le_bytes()); // byte rate
        wav.extend_from_slice(&4u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // 16 bits
        // data chunk
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&8u32.to_le_bytes()); // 8 bytes (2 stereo samples)
        wav.extend_from_slice(&[0u8; 8]); // 2 zero samples

        let info = parse_wav_header(&wav).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.data_bytes, 8);
        assert_eq!(info.total_samples_per_channel, 2);
    }
}
