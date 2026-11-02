//! AAC-LC audio encoder wrapper backed by `craft-aac`.

use craft_aac::{Encoder, EncoderConfig};
use encodercraft_core::{AudioSampleBuffer, EncoderError};

/// AAC-LC (MPEG-4 Audio Low Complexity) audio encoder.
pub struct AacEncoderWrapper {
    inner: Encoder,
    channels: usize,
}

impl AacEncoderWrapper {
    /// Creates a new CBR AAC-LC encoder instance.
    pub fn new(sample_rate: u32, channels: u8, bitrate_bps: u32) -> Result<Self, EncoderError> {
        let ch = channels.clamp(1, 8) as usize;
        let cfg = EncoderConfig::cbr(sample_rate, ch, bitrate_bps);
        let inner = Encoder::new(cfg).map_err(|e| EncoderError::Internal(format!("failed to initialize AAC encoder: {e}")))?;

        Ok(Self { inner, channels: ch })
    }

    /// AudioSpecificConfig payload for MP4 `esds` or ADTS stream headers.
    pub fn audio_specific_config(&self) -> Vec<u8> {
        self.inner.audio_specific_config()
    }

    /// Priming / pre-roll samples (typically 1024).
    pub fn priming_samples(&self) -> u32 {
        self.inner.priming_samples()
    }

    /// Encodes an `AudioSampleBuffer` into a sequence of AAC access units (raw packets).
    pub fn encode_buffer(&mut self, buffer: &AudioSampleBuffer) -> Result<Vec<Vec<u8>>, EncoderError> {
        let num_ch = buffer.channel_count().min(self.channels);
        let mut channel_slices: Vec<&[f32]> = Vec::with_capacity(num_ch);

        for c in 0..num_ch {
            let slice = buffer.channel(c)?;
            channel_slices.push(slice);
        }

        let access_units = self.inner.encode(&channel_slices);
        Ok(access_units)
    }

    /// Flushes any pending buffered audio samples.
    pub fn flush(&mut self) -> Result<Vec<Vec<u8>>, EncoderError> {
        let flushed = self.inner.flush();
        Ok(flushed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_aac_encoder() {
        let mut enc = AacEncoderWrapper::new(48000, 2, 128000).unwrap();
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 4800, 0).unwrap();
        let packets = enc.encode_buffer(&buf).unwrap();
        let flushed = enc.flush().unwrap();
        assert!(!packets.is_empty() || !flushed.is_empty());
        assert!(!enc.audio_specific_config().is_empty());
    }
}
