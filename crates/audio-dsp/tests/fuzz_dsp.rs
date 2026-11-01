use encodercraft_audio_dsp::{PeakLimiter, SincResampler, measure_loudness, normalize_loudness, remix_channels};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_resample_no_panic(
        in_rate in 8000u32..96000u32,
        out_rate in 8000u32..96000u32,
        samples in prop::collection::vec(any::<f32>(), 0..256),
    ) {
        // Replace NaNs/Infs with finite values
        let clean_samples: Vec<f32> = samples.into_iter().map(|s| if s.is_finite() { s.clamp(-10.0, 10.0) } else { 0.0 }).collect();

        if let Ok(buf) = AudioSampleBuffer::from_planar_channels(in_rate, ChannelLayout::Mono, 0, vec![clean_samples]) {
            let resampler = SincResampler::default();
            let _ = resampler.resample(&buf, out_rate);
        }
    }

    #[test]
    fn test_remix_channels_no_panic(
        layout_in in prop::sample::select(vec![ChannelLayout::Mono, ChannelLayout::Stereo, ChannelLayout::Surround51]),
        layout_out in prop::sample::select(vec![ChannelLayout::Mono, ChannelLayout::Stereo, ChannelLayout::Surround51]),
        sample_count in 0usize..128,
    ) {
        let ch_count = layout_in as usize;
        let mut channels = Vec::new();
        for _ in 0..ch_count {
            channels.push(vec![0.5f32; sample_count]);
        }

        if let Ok(buf) = AudioSampleBuffer::from_planar_channels(48000, layout_in, 0, channels) {
            let _ = remix_channels(&buf, layout_out);
        }
    }

    #[test]
    fn test_loudness_and_limiter_no_panic(
        samples in prop::collection::vec(any::<f32>(), 0..512),
    ) {
        let clean_samples: Vec<f32> = samples.into_iter().map(|s| if s.is_finite() { s.clamp(-5.0, 5.0) } else { 0.0 }).collect();
        if let Ok(mut buf) = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Mono, 0, vec![clean_samples]) {
            let _ = measure_loudness(&buf);
            let _ = normalize_loudness(&mut buf, -23.0, -1.0);
            let limiter = PeakLimiter::new(-1.0, 0.005, 0.05, 48000);
            let _ = limiter.process(&mut buf);
        }
    }
}
