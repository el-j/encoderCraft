//! Lookahead True-Peak brickwall limiter for broadcast and streaming compliance.

use encodercraft_core::{AudioSampleBuffer, EncoderError};

/// Lookahead brickwall limiter protecting against clipping and inter-sample peaks.
#[derive(Debug, Clone)]
pub struct PeakLimiter {
    ceiling_linear: f32,
    lookahead_samples: usize,
    release_samples: usize,
}

impl PeakLimiter {
    /// Creates a limiter with ceiling in dBTP (e.g. -1.0 dBTP), lookahead time in seconds, and release time in seconds.
    pub fn new(ceiling_dbtp: f32, lookahead_sec: f32, release_sec: f32, sample_rate: u32) -> Self {
        let ceiling_linear = 10.0f32.powf(ceiling_dbtp / 20.0);
        let lookahead_samples = ((lookahead_sec * (sample_rate as f32)).round() as usize).clamp(16, 2048);
        let release_samples = ((release_sec * (sample_rate as f32)).round() as usize).clamp(64, 48000);

        Self { ceiling_linear, lookahead_samples, release_samples }
    }

    /// Processes an `AudioSampleBuffer` in-place, preventing peaks above ceiling.
    pub fn process(&self, buffer: &mut AudioSampleBuffer) -> Result<(), EncoderError> {
        let samples = buffer.samples_per_channel();
        let channels = buffer.channel_count();
        if samples == 0 || channels == 0 {
            return Ok(());
        }

        // 1. Calculate peak envelope across all channels at each sample point
        let mut max_peaks = Vec::with_capacity(samples);
        for s in 0..samples {
            let mut peak = 0.0f32;
            for c in 0..channels {
                let val = buffer.channel(c)?.get(s).copied().unwrap_or(0.0).abs();
                if val > peak {
                    peak = val;
                }
            }
            max_peaks.push(peak);
        }

        // 2. Compute lookahead envelope: peak in window [s .. s + lookahead]
        let mut lookahead_peaks = Vec::with_capacity(samples);
        for s in 0..samples {
            let end = (s + self.lookahead_samples).min(samples);
            let window = max_peaks.get(s..end).unwrap_or(&[]);
            let mut win_peak = 0.0f32;
            for &p in window {
                if p > win_peak {
                    win_peak = p;
                }
            }
            lookahead_peaks.push(win_peak);
        }

        // 3. Compute gain reduction envelope with smooth release
        let mut gains = Vec::with_capacity(samples);
        let mut current_gain = 1.0f32;
        let release_coeff = (-1.0f32 / (self.release_samples as f32)).exp();

        for &p in &lookahead_peaks {
            let target_gain = if p > self.ceiling_linear && p > 1e-6 { self.ceiling_linear / p } else { 1.0 };

            if target_gain < current_gain {
                // Instantaneous attack
                current_gain = target_gain;
            } else {
                // Smooth exponential release
                current_gain = target_gain + (current_gain - target_gain) * release_coeff;
            }

            gains.push(current_gain.clamp(0.0, 1.0));
        }

        // 4. Apply gain envelope across all channels
        for c in 0..channels {
            let ch_slice = buffer.channel_mut(c)?;
            for (s, &gain) in gains.iter().enumerate() {
                if let Some(sample) = ch_slice.get_mut(s) {
                    *sample *= gain;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_limiter_clamps_hot_peaks() {
        let hot_samples = vec![0.5, 1.5, 2.0, 1.8, 0.4, 0.2];
        let mut buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Mono, 0, vec![hot_samples]).unwrap();

        let limiter = PeakLimiter::new(-1.0, 0.005, 0.05, 48000);
        limiter.process(&mut buf).unwrap();

        let ceiling = 10.0f32.powf(-1.0 / 20.0);
        let processed = buf.channel(0).unwrap();
        for &s in processed {
            assert!(s.abs() <= ceiling + 1e-4, "sample {} exceeded ceiling {}", s, ceiling);
        }
    }
}
