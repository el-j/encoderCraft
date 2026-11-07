//! Multichannel audio matrix router with arbitrary input-to-output cross-routing and gain trimming.

use encodercraft_core::{AudioChannelMapping, AudioSampleBuffer, ChannelLayout, EncoderError};

/// Remixes an audio buffer through an arbitrary M x N matrix router.
pub fn apply_audio_matrix(mapping: &AudioChannelMapping, source: &AudioSampleBuffer) -> Result<AudioSampleBuffer, EncoderError> {
    let out_channels = mapping.output_channels;
    if out_channels == 0 {
        return Err(EncoderError::InvalidJobSpec("audio matrix router output channels cannot be zero".into()));
    }

    let samples = source.samples_per_channel();
    let sample_rate = source.sample_rate;
    let start_idx = source.start_sample_index;

    let target_layout = match out_channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        6 => ChannelLayout::Surround51,
        8 => ChannelLayout::Surround71,
        _ => {
            if out_channels <= 2 {
                ChannelLayout::Stereo
            } else if out_channels <= 6 {
                ChannelLayout::Surround51
            } else {
                ChannelLayout::Surround71
            }
        }
    };

    let mut planar_out = Vec::with_capacity(target_layout.channel_count());
    let in_channels = source.channels();

    for out_idx in 0..target_layout.channel_count() {
        let mut channel_data = vec![0.0f32; samples];

        for (in_idx, in_ch) in in_channels.iter().enumerate() {
            let gain = mapping.get_gain(out_idx, in_idx);
            if gain != 0.0 {
                for (s, out_val) in channel_data.iter_mut().enumerate() {
                    let sample_val = in_ch.get(s).copied().unwrap_or(0.0);
                    *out_val += sample_val * gain;
                }
            }
        }
        planar_out.push(channel_data);
    }

    AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, planar_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_routing_passthrough() {
        let mapping = AudioChannelMapping::new_pass_through(2);
        let left = vec![0.5; 100];
        let right = vec![-0.5; 100];
        let buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Stereo, 0, vec![left, right]).unwrap();

        let routed = apply_audio_matrix(&mapping, &buf).unwrap();
        assert_eq!(routed.layout, ChannelLayout::Stereo);
        assert_eq!(routed.channel(0).unwrap()[0], 0.5);
        assert_eq!(routed.channel(1).unwrap()[0], -0.5);
    }

    #[test]
    fn test_matrix_routing_swap_and_gain() {
        let mut mapping = AudioChannelMapping::new_pass_through(2);
        // Swap channels: out 0 gets in 1 with 0.5 gain, out 1 gets in 0 with 2.0 gain
        mapping.matrix = vec![vec![0.0, 0.5], vec![2.0, 0.0]];

        let left = vec![0.25; 10];
        let right = vec![0.80; 10];
        let buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Stereo, 0, vec![left, right]).unwrap();

        let routed = apply_audio_matrix(&mapping, &buf).unwrap();
        assert_eq!(routed.channel(0).unwrap()[0], 0.40); // 0.80 * 0.5
        assert_eq!(routed.channel(1).unwrap()[0], 0.50); // 0.25 * 2.0
    }

    #[test]
    fn test_matrix_routing_to_8_channel_discrete() {
        let mapping = AudioChannelMapping::new_discrete_mono(8);
        let mono = vec![0.75; 50];
        let buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Mono, 0, vec![mono]).unwrap();

        let routed = apply_audio_matrix(&mapping, &buf).unwrap();
        assert_eq!(routed.layout, ChannelLayout::Surround71);
        assert_eq!(routed.channels().len(), 8);
        assert_eq!(routed.channel(0).unwrap()[0], 0.75); // Ch 1 gets input
        assert_eq!(routed.channel(1).unwrap()[0], 0.0); // Other channels silent
    }
}
