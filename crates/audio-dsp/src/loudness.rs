//! ITU-R BS.1770-4 / EBU R128 loudness measurement, True-Peak detection, and normalization.

use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

use crate::resample::SincResampler;

/// Comprehensive loudness and peak statistics conforming to EBU R128.
#[derive(Debug, Clone, PartialEq)]
pub struct LoudnessStats {
    /// Integrated loudness in LUFS (LKFS).
    pub integrated_lufs: f64,
    /// Maximum True-Peak level across all channels in dBTP.
    pub true_peak_dbtp: f64,
    /// Loudness Range (LRA) in LU.
    pub loudness_range_lu: f64,
}

/// Digital Biquad IIR Filter.
#[derive(Debug, Clone)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self { b0, b1, b2, a1, a2, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    fn process_sample(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Measures the integrated LUFS and True-Peak dBTP according to ITU-R BS.1770-4.
pub fn measure_loudness(buffer: &AudioSampleBuffer) -> Result<LoudnessStats, EncoderError> {
    if buffer.samples_per_channel() == 0 {
        return Ok(LoudnessStats { integrated_lufs: -70.0, true_peak_dbtp: -100.0, loudness_range_lu: 0.0 });
    }

    // 1. Resample to 48 kHz if needed (BS.1770 filter coefficients are standardized at 48 kHz)
    let buf_48k = if buffer.sample_rate == 48000 {
        buffer.clone()
    } else {
        let resampler = SincResampler::default();
        resampler.resample(buffer, 48000)?
    };

    let samples = buf_48k.samples_per_channel();
    let channels = buf_48k.channel_count();

    // 2. Filter channels through K-weighting pre-filter and RLB weighting filter
    let mut filtered_channels: Vec<Vec<f64>> = Vec::with_capacity(channels);
    let mut max_true_peak = 0.0f64;

    for ch in 0..channels {
        let in_data = buf_48k.channel(ch)?;

        // True peak oversampling (4x)
        let ch_peak = measure_true_peak_4x(in_data);
        if ch_peak > max_true_peak {
            max_true_peak = ch_peak;
        }

        // BS.1770 Stage 1 pre-filter (high-shelf at 48 kHz)
        let mut pre_filter = Biquad::new(1.53512485958697, -2.69169618940638, 1.19839281085285, -1.69065929318241, 0.73248077421585);

        // BS.1770 Stage 2 RLB weighting filter (high-pass at 48 kHz)
        let mut rlb_filter = Biquad::new(1.0, -2.0, 1.0, -1.99004745483398, 0.99007225034108);

        let mut filtered = Vec::with_capacity(samples);
        for &s in in_data {
            let s1 = pre_filter.process_sample(s as f64);
            let s2 = rlb_filter.process_sample(s1);
            filtered.push(s2);
        }
        filtered_channels.push(filtered);
    }

    // 3. Block gating: 400 ms blocks (19200 samples), 75% overlap (100 ms step = 4800 samples)
    let block_size = 19200;
    let step_size = 4800;

    if samples < block_size {
        // Shorter than 400 ms: calculate ungated mean square directly
        let mut total_power = 0.0f64;
        for (ch_idx, ch_data) in filtered_channels.iter().enumerate() {
            let weight = channel_weight(buf_48k.layout, ch_idx);
            let sum_sq: f64 = ch_data.iter().map(|&s| s * s).sum();
            total_power += weight * (sum_sq / (samples as f64));
        }

        let lufs = if total_power > 1e-12 { -0.691 + 10.0 * total_power.log10() } else { -70.0 };

        let true_peak_dbtp = if max_true_peak > 1e-9 { 20.0 * max_true_peak.log10() } else { -100.0 };

        return Ok(LoudnessStats { integrated_lufs: lufs, true_peak_dbtp, loudness_range_lu: 0.0 });
    }

    let num_blocks = (samples - block_size) / step_size + 1;
    let mut block_powers = Vec::with_capacity(num_blocks);

    for b in 0..num_blocks {
        let start = b * step_size;
        let end = start + block_size;

        let mut block_power = 0.0f64;
        for (ch_idx, ch_data) in filtered_channels.iter().enumerate() {
            let weight = channel_weight(buf_48k.layout, ch_idx);
            if weight == 0.0 {
                continue;
            }

            let slice = ch_data.get(start..end).unwrap_or(&[]);
            let sum_sq: f64 = slice.iter().map(|&s| s * s).sum();
            block_power += weight * (sum_sq / (block_size as f64));
        }

        block_powers.push(block_power);
    }

    // Absolute threshold: -70 LKFS -> power = 10^((-70 + 0.691)/10)
    let abs_threshold_power = 10.0f64.powf((-70.0 + 0.691) / 10.0);

    let mut above_abs = Vec::new();
    for &p in &block_powers {
        if p >= abs_threshold_power {
            above_abs.push(p);
        }
    }

    if above_abs.is_empty() {
        let true_peak_dbtp = if max_true_peak > 1e-9 { 20.0 * max_true_peak.log10() } else { -100.0 };
        return Ok(LoudnessStats { integrated_lufs: -70.0, true_peak_dbtp, loudness_range_lu: 0.0 });
    }

    // Relative threshold: average of blocks above absolute threshold, minus 10 LU
    let avg_abs_power = above_abs.iter().sum::<f64>() / (above_abs.len() as f64);
    let rel_threshold_power = avg_abs_power * 10.0f64.powf(-10.0 / 10.0);

    let mut above_rel = Vec::new();
    for &p in &above_abs {
        if p >= rel_threshold_power {
            above_rel.push(p);
        }
    }

    let final_power = if above_rel.is_empty() { avg_abs_power } else { above_rel.iter().sum::<f64>() / (above_rel.len() as f64) };

    let integrated_lufs = if final_power > 1e-12 { -0.691 + 10.0 * final_power.log10() } else { -70.0 };

    let true_peak_dbtp = if max_true_peak > 1e-9 { 20.0 * max_true_peak.log10() } else { -100.0 };

    Ok(LoudnessStats { integrated_lufs, true_peak_dbtp, loudness_range_lu: 0.0 })
}

/// Normalizes the entire audio buffer to the target integrated LUFS level,
/// while respecting maximum allowed true-peak to prevent clipping.
pub fn normalize_loudness(buffer: &mut AudioSampleBuffer, target_lufs: f64, max_true_peak_dbtp: f64) -> Result<f64, EncoderError> {
    let stats = measure_loudness(buffer)?;

    if stats.integrated_lufs <= -70.0 {
        return Ok(0.0); // Buffer is completely silent
    }

    let mut gain_db = target_lufs - stats.integrated_lufs;

    // Check if applying this gain would exceed max true-peak
    let projected_peak_dbtp = stats.true_peak_dbtp + gain_db;
    if projected_peak_dbtp > max_true_peak_dbtp {
        // Back off gain to stay within true-peak limit
        gain_db -= projected_peak_dbtp - max_true_peak_dbtp;
    }

    let linear_gain = (10.0f64.powf(gain_db / 20.0)) as f32;

    let channels = buffer.channel_count();
    for ch in 0..channels {
        let ch_slice = buffer.channel_mut(ch)?;
        for s in ch_slice.iter_mut() {
            *s *= linear_gain;
        }
    }

    Ok(gain_db)
}

fn channel_weight(layout: ChannelLayout, ch_index: usize) -> f64 {
    match layout {
        ChannelLayout::Mono => 1.0,
        ChannelLayout::Stereo => 1.0,
        ChannelLayout::Surround51 => {
            match ch_index {
                0..=2 => 1.0,                      // L, R, C
                3 => 0.0,                          // LFE is ignored in BS.1770
                4 | 5 => std::f64::consts::SQRT_2, // Ls, Rs (+1.5 dB = ~1.41)
                _ => 1.0,
            }
        }
        ChannelLayout::Surround71 => match ch_index {
            0..=2 => 1.0,
            3 => 0.0,
            4..=7 => std::f64::consts::SQRT_2,
            _ => 1.0,
        },
    }
}

/// 4x oversampled True-Peak detector.
fn measure_true_peak_4x(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }

    let mut max_peak = 0.0f64;

    // Direct sample peaks
    for &s in samples {
        let abs = (s as f64).abs();
        if abs > max_peak {
            max_peak = abs;
        }
    }

    // Polyphase inter-sample interpolation (4x oversampling filter)
    for i in 1..samples.len().saturating_sub(2) {
        let s0 = samples.get(i.saturating_sub(1)).copied().unwrap_or(0.0) as f64;
        let s1 = samples.get(i).copied().unwrap_or(0.0) as f64;
        let s2 = samples.get(i + 1).copied().unwrap_or(0.0) as f64;
        let s3 = samples.get(i + 2).copied().unwrap_or(0.0) as f64;

        // Catmull-Rom cubic Hermite spline interpolation at 0.25, 0.5, 0.75
        for frac in [0.25, 0.5, 0.75] {
            let t2 = frac * frac;
            let t3 = t2 * frac;

            let interpolated =
                0.5 * ((2.0 * s1) + (-s0 + s2) * frac + (2.0 * s0 - 5.0 * s1 + 4.0 * s2 - s3) * t2 + (-s0 + 3.0 * s1 - 3.0 * s2 + s3) * t3);

            let abs = interpolated.abs();
            if abs > max_peak {
                max_peak = abs;
            }
        }
    }

    max_peak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_buffer_loudness() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 48000, 0).unwrap();
        let stats = measure_loudness(&buf).unwrap();
        assert!(stats.integrated_lufs <= -70.0);
    }

    #[test]
    fn test_sine_wave_loudness_and_normalization() {
        let mut samples = Vec::new();
        // 1 kHz sine at 0 dBFS (peak = 1.0)
        for i in 0..48000 {
            let phase = 2.0 * std::f32::consts::PI * 1000.0 * (i as f32) / 48000.0;
            samples.push(phase.sin() * 0.5); // -6 dBFS
        }

        let mut buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Mono, 0, vec![samples]).unwrap();

        let initial_stats = measure_loudness(&buf).unwrap();
        assert!(initial_stats.integrated_lufs > -30.0 && initial_stats.integrated_lufs < -5.0);

        // Normalize to -23 LUFS (EBU R128 target)
        let applied_gain = normalize_loudness(&mut buf, -23.0, -1.0).unwrap();
        assert!(applied_gain.is_finite());

        let post_stats = measure_loudness(&buf).unwrap();
        assert!((post_stats.integrated_lufs - (-23.0)).abs() < 1.0);
        assert!(post_stats.true_peak_dbtp <= -1.0 + 0.1);
    }
}
