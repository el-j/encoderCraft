//! Audio channel delay / lip-sync compensation filter.

use encodercraft_core::{AudioSampleBuffer, EncoderError};

/// Applies delay or advance to an audio sample buffer in milliseconds.
///
/// - `delay_ms > 0`: Delays audio (pads with silent zeroes at the beginning).
/// - `delay_ms < 0`: Advances audio (trims samples from the beginning).
/// - `delay_ms == 0`: No-op.
pub fn apply_audio_delay(buffer: &mut AudioSampleBuffer, delay_ms: i32) -> Result<(), EncoderError> {
    if delay_ms == 0 || buffer.sample_rate == 0 {
        return Ok(());
    }

    let sample_rate = buffer.sample_rate as f64;
    let offset_samples = ((delay_ms.abs() as f64 / 1000.0) * sample_rate).round() as usize;
    if offset_samples == 0 {
        return Ok(());
    }

    if delay_ms > 0 {
        for ch in buffer.channels_mut() {
            let mut new_ch = vec![0.0f32; offset_samples];
            new_ch.extend_from_slice(ch);
            *ch = new_ch;
        }
    } else {
        for ch in buffer.channels_mut() {
            if ch.len() <= offset_samples {
                ch.fill(0.0);
            } else {
                ch.drain(0..offset_samples);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_positive_audio_delay() {
        let mut buf = AudioSampleBuffer::from_planar_channels(48_000, ChannelLayout::Stereo, 0, vec![vec![1.0; 480], vec![1.0; 480]]).unwrap();

        // 10 ms at 48 kHz = 480 samples of silence
        apply_audio_delay(&mut buf, 10).unwrap();
        assert_eq!(buf.samples_per_channel(), 960);
        assert_eq!(buf.channel(0).unwrap()[0], 0.0);
        assert_eq!(buf.channel(0).unwrap()[479], 0.0);
        assert_eq!(buf.channel(0).unwrap()[480], 1.0);
    }

    #[test]
    fn test_negative_audio_advance() {
        let mut samples = Vec::new();
        for i in 0..480 {
            samples.push(i as f32);
        }
        let mut buf = AudioSampleBuffer::from_planar_channels(48_000, ChannelLayout::Mono, 0, vec![samples]).unwrap();

        // -5 ms at 48 kHz = 240 samples trimmed from start
        apply_audio_delay(&mut buf, -5).unwrap();
        assert_eq!(buf.samples_per_channel(), 240);
        assert_eq!(buf.channel(0).unwrap()[0], 240.0);
    }

    #[test]
    fn test_zero_delay_is_noop() {
        let mut buf = AudioSampleBuffer::from_planar_channels(48_000, ChannelLayout::Mono, 0, vec![vec![0.5; 100]]).unwrap();
        apply_audio_delay(&mut buf, 0).unwrap();
        assert_eq!(buf.samples_per_channel(), 100);
        assert_eq!(buf.channel(0).unwrap()[0], 0.5);
    }
}
