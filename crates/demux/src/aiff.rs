//! Clean-room parser for Apple AIFF / AIFC audio files.
//!
//! Enforces bounded allocations, big-endian conversions, and IEEE 754 extended float decoding.

use encodercraft_core::EncoderError;

/// Parsed metadata from an AIFF header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiffHeaderInfo {
    pub channels: u16,
    pub num_sample_frames: u32,
    pub bits_per_sample: u16,
    pub sample_rate: u32,
    pub data_offset: usize,
    pub data_bytes: u64,
}

/// Parses an IEEE 754 80-bit extended precision float (big-endian) into integer sample rate.
fn parse_ieee_extended_sample_rate(bytes: &[u8; 10]) -> u32 {
    let exponent = (((bytes[0] as u16) & 0x7F) << 8) | (bytes[1] as u16);
    let mantissa = u64::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9]]);

    if exponent == 0 && mantissa == 0 {
        return 0;
    }

    // Exponent bias is 16383
    let shift = (exponent as i32) - 16383 - 63;
    if shift >= 0 {
        (mantissa as u32).checked_shl(shift as u32).unwrap_or(0)
    } else {
        let shift_right = (-shift).min(63) as u32;
        (mantissa >> shift_right) as u32
    }
}

/// Parses an AIFF or AIFC file buffer.
pub fn parse_aiff_header(bytes: &[u8]) -> Result<AiffHeaderInfo, EncoderError> {
    if bytes.len() < 12 {
        return Err(EncoderError::InvalidContainer("AIFF file too short for FORM header".into()));
    }

    if !bytes.starts_with(b"FORM") {
        return Err(EncoderError::InvalidContainer("not an AIFF FORM container".into()));
    }

    let form_type = bytes.get(8..12).ok_or_else(|| EncoderError::InvalidContainer("error reading AIFF form type".into()))?;
    if form_type != b"AIFF" && form_type != b"AIFC" {
        return Err(EncoderError::InvalidContainer(format!("expected AIFF or AIFC form type, got `{}`", String::from_utf8_lossy(form_type))));
    }

    let mut cursor: usize = 12;
    let mut comm_info: Option<(u16, u32, u16, u32)> = None;
    let mut ssnd_info: Option<(usize, u64)> = None;

    while cursor.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let chunk_id = bytes.get(cursor..cursor + 4).ok_or_else(|| EncoderError::InvalidContainer("error reading chunk id".into()))?;
        let chunk_size = u32::from_be_bytes([
            bytes.get(cursor + 4).copied().unwrap_or(0),
            bytes.get(cursor + 5).copied().unwrap_or(0),
            bytes.get(cursor + 6).copied().unwrap_or(0),
            bytes.get(cursor + 7).copied().unwrap_or(0),
        ]) as usize;

        let chunk_data_offset = cursor + 8;

        if chunk_id == b"COMM" {
            if chunk_data_offset.checked_add(18).is_some_and(|end| end <= bytes.len()) {
                let channels =
                    u16::from_be_bytes([bytes.get(chunk_data_offset).copied().unwrap_or(0), bytes.get(chunk_data_offset + 1).copied().unwrap_or(0)]);
                let num_frames = u32::from_be_bytes([
                    bytes.get(chunk_data_offset + 2).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 3).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 4).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 5).copied().unwrap_or(0),
                ]);
                let sample_size = u16::from_be_bytes([
                    bytes.get(chunk_data_offset + 6).copied().unwrap_or(0),
                    bytes.get(chunk_data_offset + 7).copied().unwrap_or(0),
                ]);

                let mut rate_bytes = [0u8; 10];
                if let Some(src) = bytes.get(chunk_data_offset + 8..chunk_data_offset + 18) {
                    rate_bytes.copy_from_slice(src);
                }
                let sample_rate = parse_ieee_extended_sample_rate(&rate_bytes);

                comm_info = Some((channels, num_frames, sample_size, sample_rate));
            }
        } else if chunk_id == b"SSND" && chunk_data_offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
            let offset = u32::from_be_bytes([
                bytes.get(chunk_data_offset).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 1).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 2).copied().unwrap_or(0),
                bytes.get(chunk_data_offset + 3).copied().unwrap_or(0),
            ]) as usize;

            let sound_data_start = chunk_data_offset + 8 + offset;
            let sound_bytes = (chunk_size as u64).saturating_sub(8 + offset as u64);
            ssnd_info = Some((sound_data_start, sound_bytes));
            break;
        }

        // Even-byte chunk padding
        let padded_size = (chunk_size + 1) & !1;
        cursor = cursor
            .checked_add(8)
            .and_then(|c| c.checked_add(padded_size))
            .ok_or_else(|| EncoderError::InvalidContainer("overflow calculating chunk offset".into()))?;
    }

    let (channels, num_frames, sample_size, sample_rate) =
        comm_info.ok_or_else(|| EncoderError::InvalidContainer("AIFF missing required `COMM` chunk".into()))?;

    let (data_offset, data_bytes) = ssnd_info.ok_or_else(|| EncoderError::InvalidContainer("AIFF missing required `SSND` chunk".into()))?;

    Ok(AiffHeaderInfo { channels, num_sample_frames: num_frames, bits_per_sample: sample_size, sample_rate, data_offset, data_bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ieee_extended_rate() {
        // 44100 Hz in 80-bit IEEE extended:
        // Exponent = 0x400E (16398), Mantissa = 0xAC44000000000000
        let bytes: [u8; 10] = [0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(parse_ieee_extended_sample_rate(&bytes), 44100);

        // 48000 Hz: Exponent = 0x400E, Mantissa = 0xBB80000000000000
        let bytes_48: [u8; 10] = [0x40, 0x0E, 0xBB, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(parse_ieee_extended_sample_rate(&bytes_48), 48000);
    }
}
