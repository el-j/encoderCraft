//! High-fidelity bandlimited sinc audio sample rate conversion.
//!
//! Provides clean, click-free resampling across arbitrary sample rates (e.g. 44.1 kHz ↔ 48 kHz / 96 kHz)
//! using windowed sinc interpolation with anti-aliasing low-pass filtering.

use encodercraft_core::{AudioSampleBuffer, EncoderError};

/// Bandlimited sinc resampler with anti-aliasing protection.
#[derive(Debug, Clone)]
pub struct SincResampler {
    filter_half_length: usize,
}

impl Default for SincResampler {
    fn default() -> Self {
        Self::new(16)
    }
}

impl SincResampler {
    /// Creates a new resampler with the specified filter half-width (number of zero crossings).
    /// Typically 16 gives studio-quality results (~96 dB stopband attenuation).
    pub fn new(filter_half_length: usize) -> Self {
        let half = filter_half_length.clamp(4, 64);
        Self { filter_half_length: half }
    }

    /// Resamples an entire `AudioSampleBuffer` to a target sample rate.
    pub fn resample(&self, source: &AudioSampleBuffer, target_sample_rate: u32) -> Result<AudioSampleBuffer, EncoderError> {
        if target_sample_rate == 0 {
            return Err(EncoderError::InvalidJobSpec("target sample rate cannot be zero".into()));
        }

        let in_rate = source.sample_rate;
        if in_rate == target_sample_rate {
            return Ok(source.clone());
        }

        let in_samples = source.samples_per_channel();
        if in_samples == 0 {
            return AudioSampleBuffer::new_silent(target_sample_rate, source.layout, 0, source.start_sample_index);
        }

        // Calculate output sample count: (in_samples * target_sample_rate) / in_rate
        let out_samples_u128 = (in_samples as u128)
            .checked_mul(target_sample_rate as u128)
            .ok_or_else(|| EncoderError::Internal("overflow calculating resampled buffer size".into()))?;
        let out_samples = (out_samples_u128 / (in_rate as u128)) as usize;

        let channel_count = source.channel_count();
        let mut out_channels = Vec::with_capacity(channel_count);

        for ch in 0..channel_count {
            let in_data = source.channel(ch)?;
            let mut out_data = Vec::with_capacity(out_samples);
            self.resample_channel(in_data, in_rate, &mut out_data, target_sample_rate, out_samples)?;
            out_channels.push(out_data);
        }

        AudioSampleBuffer::from_planar_channels(target_sample_rate, source.layout, source.start_sample_index, out_channels)
    }

    /// Resamples a single channel slice into a destination vector.
    pub fn resample_channel(&self, input: &[f32], in_rate: u32, output: &mut Vec<f32>, out_rate: u32, target_len: usize) -> Result<(), EncoderError> {
        if input.is_empty() || target_len == 0 {
            output.clear();
            return Ok(());
        }

        output.clear();
        output.reserve(target_len);

        let ratio = (in_rate as f64) / (out_rate as f64);
        let cutoff = if out_rate < in_rate {
            // Anti-aliasing downsampling filter
            0.95 * (out_rate as f64) / (in_rate as f64)
        } else {
            // Upsampling: standard Nyquist
            0.95
        };

        let half_len = self.filter_half_length as isize;

        for n in 0..target_len {
            let center_t = (n as f64) * ratio;
            let center_idx = center_t.floor() as isize;
            let frac = center_t - (center_idx as f64);

            let mut sum = 0.0f64;
            let mut weight_sum = 0.0f64;

            for offset in -half_len..=half_len {
                let sample_idx = center_idx + offset;
                let sample_val = if sample_idx < 0 {
                    input.first().copied().unwrap_or(0.0) as f64
                } else if (sample_idx as usize) >= input.len() {
                    input.last().copied().unwrap_or(0.0) as f64
                } else {
                    input.get(sample_idx as usize).copied().unwrap_or(0.0) as f64
                };

                let t = (offset as f64) - frac;
                let weight = self.sinc_windowed(t * cutoff, t / (self.filter_half_length as f64));
                sum += sample_val * weight;
                weight_sum += weight;
            }

            let normalized = if weight_sum.abs() > 1e-9 { sum / weight_sum } else { sum };

            output.push(normalized as f32);
        }

        Ok(())
    }

    #[inline]
    fn sinc_windowed(&self, x: f64, window_x: f64) -> f64 {
        if window_x.abs() >= 1.0 {
            return 0.0;
        }

        let sinc_val = if x.abs() < 1e-9 {
            1.0
        } else {
            let pix = std::f64::consts::PI * x;
            pix.sin() / pix
        };

        // Blackman window
        let w = 0.42 + 0.5 * (std::f64::consts::PI * (window_x + 1.0)).cos() + 0.08 * (2.0 * std::f64::consts::PI * (window_x + 1.0)).cos();
        sinc_val * w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_identical_rate_passthrough() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 512, 0).unwrap();
        let resampler = SincResampler::default();
        let out = resampler.resample(&buf, 48000).unwrap();
        assert_eq!(out.sample_rate, 48000);
        assert_eq!(out.samples_per_channel(), 512);
    }

    #[test]
    fn test_upsampling_44100_to_48000() {
        let mut left = Vec::new();
        for i in 0..441 {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * (i as f32) / 44100.0;
            left.push(phase.sin());
        }
        let buf = AudioSampleBuffer::from_planar_channels(44100, ChannelLayout::Mono, 0, vec![left]).unwrap();
        let resampler = SincResampler::default();
        let out = resampler.resample(&buf, 48000).unwrap();
        assert_eq!(out.sample_rate, 48000);
        assert_eq!(out.samples_per_channel(), 480);
    }
}
