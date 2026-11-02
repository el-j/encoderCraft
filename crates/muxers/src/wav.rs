//! Broadcast Wave Format (BWF) RIFF / RF64 audio container multiplexer.
//!
//! Complies with EBU Tech 3285 for embedding `bext` broadcast audio metadata and timecode reference.

use std::io::{Seek, SeekFrom, Write};

use encodercraft_core::{AudioSampleBuffer, EncoderError};

use crate::muxer::BextMetadata;

/// Format of the audio samples inside the WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormatTag {
    Pcm16,
    Pcm24,
    Pcm32,
    IeeeFloat32,
}

/// Broadcast Wave Format (BWF) writer.
pub struct BwfWavWriter<W: Write + Seek> {
    writer: W,
    sample_rate: u32,
    channels: u16,
    format: WavFormatTag,
    data_bytes_written: u64,
    data_chunk_pos: u64,
}

impl<W: Write + Seek> BwfWavWriter<W> {
    /// Initializes a new WAV container and writes the RIFF header, optional `bext` chunk, and `fmt ` chunk.
    pub fn new(writer: W, sample_rate: u32, channels: u16, format: WavFormatTag, bext: Option<&BextMetadata>) -> Result<Self, EncoderError> {
        Self::new_with_xmp(writer, sample_rate, channels, format, bext, None)
    }

    /// Initializes a new WAV container with optional `bext` broadcast chunk and `axml` XMP metadata chunk.
    pub fn new_with_xmp(
        mut writer: W,
        sample_rate: u32,
        channels: u16,
        format: WavFormatTag,
        bext: Option<&BextMetadata>,
        xmp_xml: Option<&str>,
    ) -> Result<Self, EncoderError> {
        if sample_rate == 0 || channels == 0 {
            return Err(EncoderError::InvalidJobSpec("sample rate and channels must be positive".into()));
        }

        // 1. Placeholder RIFF header
        writer.write_all(b"RIFF\0\0\0\0WAVE").map_err(|e| EncoderError::Io(format!("failed to write RIFF header: {e}")))?;

        // 2. Optional `bext` broadcast chunk (EBU Tech 3285)
        if let Some(meta) = bext {
            writer.write_all(b"bext").map_err(|e| EncoderError::Io(format!("failed to write bext chunk id: {e}")))?;

            let bext_len: u32 = 602; // Standard size of EBU Tech 3285 v1/v2 bext header
            writer.write_all(&bext_len.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write bext length: {e}")))?;

            let mut bext_data = vec![0u8; bext_len as usize];

            // Description: 256 bytes
            let desc_bytes = meta.description.as_bytes();
            let desc_len = desc_bytes.len().min(256);
            bext_data[0..desc_len].copy_from_slice(&desc_bytes[..desc_len]);

            // Originator: 32 bytes (offset 256)
            let orig_bytes = meta.originator.as_bytes();
            let orig_len = orig_bytes.len().min(32);
            bext_data[256..256 + orig_len].copy_from_slice(&orig_bytes[..orig_len]);

            // OriginatorReference: 32 bytes (offset 288)
            let ref_bytes = meta.originator_reference.as_bytes();
            let ref_len = ref_bytes.len().min(32);
            bext_data[288..288 + ref_len].copy_from_slice(&ref_bytes[..ref_len]);

            // OriginationDate: 10 bytes YYYY-MM-DD (offset 320)
            let date_bytes = meta.origination_date.as_bytes();
            let date_len = date_bytes.len().min(10);
            bext_data[320..320 + date_len].copy_from_slice(&date_bytes[..date_len]);

            // OriginationTime: 8 bytes HH:MM:SS (offset 330)
            let time_bytes = meta.origination_time.as_bytes();
            let time_len = time_bytes.len().min(8);
            bext_data[330..330 + time_len].copy_from_slice(&time_bytes[..time_len]);

            // TimeReference: 8 bytes (offset 338: low 32 bits, offset 342: high 32 bits)
            let time_low = (meta.time_reference_samples & 0xFFFF_FFFF) as u32;
            let time_high = ((meta.time_reference_samples >> 32) & 0xFFFF_FFFF) as u32;
            bext_data[338..342].copy_from_slice(&time_low.to_le_bytes());
            bext_data[342..346].copy_from_slice(&time_high.to_le_bytes());

            // Version: 1 (offset 346)
            bext_data[346..348].copy_from_slice(&1u16.to_le_bytes());

            // UMID: 64 bytes (offset 348)
            bext_data[348..348 + 64].copy_from_slice(&meta.umid);

            writer.write_all(&bext_data).map_err(|e| EncoderError::Io(format!("failed to write bext payload: {e}")))?;
        }

        // 3. Optional `axml` chunk for XMP metadata (EBU Tech 3285 Supplement 1 / Adobe XMP)
        if let Some(xml) = xmp_xml {
            writer.write_all(b"axml").map_err(|e| EncoderError::Io(format!("failed to write axml chunk id: {e}")))?;
            let xml_bytes = xml.as_bytes();
            let axml_len = xml_bytes.len() as u32;
            writer.write_all(&axml_len.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write axml length: {e}")))?;
            writer.write_all(xml_bytes).map_err(|e| EncoderError::Io(format!("failed to write axml body: {e}")))?;
            if !axml_len.is_multiple_of(2) {
                writer.write_all(&[0u8]).map_err(|e| EncoderError::Io(format!("failed to write axml pad: {e}")))?;
            }
        }

        // 3. `fmt ` chunk
        writer.write_all(b"fmt ").map_err(|e| EncoderError::Io(format!("failed to write fmt chunk id: {e}")))?;
        let fmt_len: u32 = 16;
        writer.write_all(&fmt_len.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write fmt length: {e}")))?;

        let (tag, bits_per_sample) = match format {
            WavFormatTag::Pcm16 => (1u16, 16u16),
            WavFormatTag::Pcm24 => (1u16, 24u16),
            WavFormatTag::Pcm32 => (1u16, 32u16),
            WavFormatTag::IeeeFloat32 => (3u16, 32u16),
        };

        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate * (block_align as u32);

        writer.write_all(&tag.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write audio tag: {e}")))?;
        writer.write_all(&channels.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write channel count: {e}")))?;
        writer.write_all(&sample_rate.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write sample rate: {e}")))?;
        writer.write_all(&byte_rate.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write byte rate: {e}")))?;
        writer.write_all(&block_align.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write block align: {e}")))?;
        writer.write_all(&bits_per_sample.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write bits per sample: {e}")))?;

        // 4. `data` chunk header placeholder
        writer.write_all(b"data").map_err(|e| EncoderError::Io(format!("failed to write data chunk id: {e}")))?;
        let data_chunk_pos = writer.stream_position().map_err(|e| EncoderError::Io(format!("failed to get stream position: {e}")))?;
        writer.write_all(&0u32.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to write data length placeholder: {e}")))?;

        Ok(Self { writer, sample_rate, channels, format, data_bytes_written: 0, data_chunk_pos })
    }

    /// Sample rate of the audio file in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of interleaved audio channels.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Number of audio PCM bytes written to the data chunk so far.
    pub fn data_bytes_written(&self) -> u64 {
        self.data_bytes_written
    }

    /// Appends audio samples from an `AudioSampleBuffer`.
    pub fn write_buffer(&mut self, buffer: &AudioSampleBuffer) -> Result<(), EncoderError> {
        if buffer.channel_count() != (self.channels as usize) {
            return Err(EncoderError::InvalidJobSpec(format!("channel count mismatch (expected {}, got {})", self.channels, buffer.channel_count())));
        }

        let samples = buffer.samples_per_channel();
        if samples == 0 {
            return Ok(());
        }

        match self.format {
            WavFormatTag::Pcm16 => {
                let bytes = buffer.to_interleaved_s16le()?;
                self.writer.write_all(&bytes).map_err(|e| EncoderError::Io(format!("failed to write pcm16 samples: {e}")))?;
                self.data_bytes_written += bytes.len() as u64;
            }
            WavFormatTag::IeeeFloat32 => {
                let bytes = buffer.to_interleaved_f32le()?;
                self.writer.write_all(&bytes).map_err(|e| EncoderError::Io(format!("failed to write f32 samples: {e}")))?;
                self.data_bytes_written += bytes.len() as u64;
            }
            WavFormatTag::Pcm24 => {
                let mut bytes = Vec::with_capacity(samples * (self.channels as usize) * 3);
                for s in 0..samples {
                    for c in 0..(self.channels as usize) {
                        let sample_f32 = buffer.channel(c)?.get(s).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
                        let s24 = (sample_f32 * 8_388_607.0).round() as i32;
                        let le = s24.to_le_bytes();
                        bytes.extend_from_slice(&le[0..3]);
                    }
                }
                self.writer.write_all(&bytes).map_err(|e| EncoderError::Io(format!("failed to write pcm24 samples: {e}")))?;
                self.data_bytes_written += bytes.len() as u64;
            }
            WavFormatTag::Pcm32 => {
                let mut bytes = Vec::with_capacity(samples * (self.channels as usize) * 4);
                for s in 0..samples {
                    for c in 0..(self.channels as usize) {
                        let sample_f32 = buffer.channel(c)?.get(s).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
                        let s32 = (sample_f32 * 2_147_483_647.0).round() as i32;
                        bytes.extend_from_slice(&s32.to_le_bytes());
                    }
                }
                self.writer.write_all(&bytes).map_err(|e| EncoderError::Io(format!("failed to write pcm32 samples: {e}")))?;
                self.data_bytes_written += bytes.len() as u64;
            }
        }

        Ok(())
    }

    /// Back-patches RIFF and data chunk lengths and finalizes the file.
    pub fn finish(mut self) -> Result<W, EncoderError> {
        let total_file_len = self.writer.stream_position().map_err(|e| EncoderError::Io(format!("failed to get final position: {e}")))?;

        // Back-patch data chunk size
        let data_len_u32 = (self.data_bytes_written & 0xFFFF_FFFF) as u32;
        self.writer.seek(SeekFrom::Start(self.data_chunk_pos)).map_err(|e| EncoderError::Io(format!("failed to seek to data size: {e}")))?;
        self.writer.write_all(&data_len_u32.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to patch data size: {e}")))?;

        // Back-patch RIFF size (total file size minus 8)
        let riff_len = ((total_file_len.saturating_sub(8)) & 0xFFFF_FFFF) as u32;
        self.writer.seek(SeekFrom::Start(4)).map_err(|e| EncoderError::Io(format!("failed to seek to riff size: {e}")))?;
        self.writer.write_all(&riff_len.to_le_bytes()).map_err(|e| EncoderError::Io(format!("failed to patch riff size: {e}")))?;

        // Seek back to end
        self.writer.seek(SeekFrom::Start(total_file_len)).map_err(|e| EncoderError::Io(format!("failed to seek back to end: {e}")))?;

        Ok(self.writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;
    use std::io::Cursor;

    #[test]
    fn test_bwf_wav_writer() {
        let cursor = Cursor::new(Vec::new());
        let bext = BextMetadata {
            description: "ArtCraft Test Audio".into(),
            originator: "encoderCraft".into(),
            origination_date: "2026-11-01".into(),
            origination_time: "23:00:00".into(),
            time_reference_samples: 48000,
            ..Default::default()
        };

        let mut writer = BwfWavWriter::new(cursor, 48000, 2, WavFormatTag::Pcm16, Some(&bext)).unwrap();

        let left = vec![0.0f32, 0.5f32, -0.5f32];
        let right = vec![0.1f32, -0.1f32, 0.0f32];
        let buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Stereo, 0, vec![left, right]).unwrap();

        writer.write_buffer(&buf).unwrap();
        let finished = writer.finish().unwrap();
        let bytes = finished.into_inner();

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // verify bext chunk exists
        assert!(&bytes[12..16] == b"bext");
    }
}
