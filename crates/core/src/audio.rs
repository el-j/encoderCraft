//! Planar 32-bit float audio sample buffers and channel configurations.
//!
//! Enforces bounded allocations, safe channel access, and clean sample conversions.

use serde::{Deserialize, Serialize};

use crate::error::EncoderError;

/// Standard audio channel layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChannelLayout {
    /// Single channel (center/mono).
    Mono = 1,
    /// 2 channels (Left, Right).
    #[default]
    Stereo = 2,
    /// 6 channels (Left, Right, Center, LFE, Left Surround, Right Surround).
    Surround51 = 6,
    /// 8 channels (Left, Right, Center, LFE, Left Surround, Right Surround, Left Rear, Right Rear).
    Surround71 = 8,
}

/// A buffer of audio samples stored in 32-bit floating point planar format.
#[derive(Debug, Clone)]
pub struct AudioSampleBuffer {
    pub sample_rate: u32,
    pub layout: ChannelLayout,
    pub start_sample_index: i64,
    channels: Vec<Vec<f32>>,
}

impl AudioSampleBuffer {
    /// Creates an empty buffer with pre-allocated zero samples for all channels.
    pub fn new_silent(sample_rate: u32, layout: ChannelLayout, samples_per_channel: usize, start_sample_index: i64) -> Result<Self, EncoderError> {
        if sample_rate == 0 {
            return Err(EncoderError::InvalidJobSpec("audio sample rate cannot be zero".into()));
        }

        let channel_count = layout as usize;
        let mut channels = Vec::with_capacity(channel_count);
        for _ in 0..channel_count {
            channels.push(vec![0.0f32; samples_per_channel]);
        }

        Ok(Self { sample_rate, layout, start_sample_index, channels })
    }

    /// Creates an audio buffer from existing planar channel vectors.
    pub fn from_planar_channels(
        sample_rate: u32,
        layout: ChannelLayout,
        start_sample_index: i64,
        channels: Vec<Vec<f32>>,
    ) -> Result<Self, EncoderError> {
        if sample_rate == 0 {
            return Err(EncoderError::InvalidJobSpec("audio sample rate cannot be zero".into()));
        }

        let expected_channels = layout as usize;
        if channels.len() != expected_channels {
            return Err(EncoderError::InvalidJobSpec(format!("channel count mismatch (expected {}, got {})", expected_channels, channels.len())));
        }

        let first_len = channels.first().map(Vec::len).unwrap_or(0);
        for (i, ch) in channels.iter().enumerate() {
            if ch.len() != first_len {
                return Err(EncoderError::InvalidJobSpec(format!(
                    "channel {} sample length {} differs from channel 0 length {}",
                    i,
                    ch.len(),
                    first_len
                )));
            }
        }

        Ok(Self { sample_rate, layout, start_sample_index, channels })
    }

    /// Number of audio channels in this buffer.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Number of samples per channel.
    pub fn samples_per_channel(&self) -> usize {
        self.channels.first().map(Vec::len).unwrap_or(0)
    }

    /// Read-only access to a specific channel's planar sample slice.
    pub fn channel(&self, index: usize) -> Result<&[f32], EncoderError> {
        self.channels.get(index).map(Vec::as_slice).ok_or_else(|| EncoderError::InvalidJobSpec(format!("channel index {index} out of range")))
    }

    /// Mutable access to a specific channel's planar sample slice.
    pub fn channel_mut(&mut self, index: usize) -> Result<&mut [f32], EncoderError> {
        self.channels.get_mut(index).map(Vec::as_mut_slice).ok_or_else(|| EncoderError::InvalidJobSpec(format!("channel index {index} out of range")))
    }

    /// Converts the planar float samples into interleaved 16-bit signed integer PCM bytes.
    pub fn to_interleaved_s16le(&self) -> Result<Vec<u8>, EncoderError> {
        let channels = self.channel_count();
        let samples = self.samples_per_channel();
        let total_samples =
            channels.checked_mul(samples).ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating total audio samples".into()))?;
        let byte_count =
            total_samples.checked_mul(2).ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating audio byte count".into()))?;

        let mut bytes = Vec::with_capacity(byte_count);
        for s in 0..samples {
            for c in 0..channels {
                let sample_f32 = self.channels.get(c).and_then(|ch| ch.get(s)).copied().unwrap_or(0.0);
                let clamped = sample_f32.clamp(-1.0, 1.0);
                let sample_s16 = (clamped * 32767.0).round() as i16;
                bytes.extend_from_slice(&sample_s16.to_le_bytes());
            }
        }

        Ok(bytes)
    }

    /// Converts the planar float samples into interleaved 32-bit float PCM bytes.
    pub fn to_interleaved_f32le(&self) -> Result<Vec<u8>, EncoderError> {
        let channels = self.channel_count();
        let samples = self.samples_per_channel();
        let total_samples =
            channels.checked_mul(samples).ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating total audio samples".into()))?;
        let byte_count =
            total_samples.checked_mul(4).ok_or_else(|| EncoderError::Internal("arithmetic overflow calculating audio byte count".into()))?;

        let mut bytes = Vec::with_capacity(byte_count);
        for s in 0..samples {
            for c in 0..channels {
                let sample_f32 = self.channels.get(c).and_then(|ch| ch.get(s)).copied().unwrap_or(0.0);
                bytes.extend_from_slice(&sample_f32.to_le_bytes());
            }
        }

        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_buffer_creation() {
        let buffer = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 1024, 0).unwrap();
        assert_eq!(buffer.channel_count(), 2);
        assert_eq!(buffer.samples_per_channel(), 1024);
        assert_eq!(buffer.channel(0).unwrap().len(), 1024);
        assert_eq!(buffer.channel(1).unwrap().len(), 1024);
    }

    #[test]
    fn test_interleaving_s16le() {
        let left = vec![0.0f32, 1.0f32, -1.0f32];
        let right = vec![0.5f32, -0.5f32, 0.0f32];
        let buffer = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Stereo, 0, vec![left, right]).unwrap();

        let s16_bytes = buffer.to_interleaved_s16le().unwrap();
        // 3 samples * 2 channels * 2 bytes = 12 bytes
        assert_eq!(s16_bytes.len(), 12);
    }
}
