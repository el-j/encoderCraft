//! Broadcast-standard audio channel remixing, downmixing, and upmixing matrices.
//!
//! Complies with ITU-R BS.775 recommendations for surround downmixes.

use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

/// Remixes an audio buffer to a target channel layout.
pub fn remix_channels(source: &AudioSampleBuffer, target_layout: ChannelLayout) -> Result<AudioSampleBuffer, EncoderError> {
    if source.layout == target_layout {
        return Ok(source.clone());
    }

    let samples = source.samples_per_channel();
    let sample_rate = source.sample_rate;
    let start_idx = source.start_sample_index;

    match (source.layout, target_layout) {
        // Mono -> Stereo
        (ChannelLayout::Mono, ChannelLayout::Stereo) => {
            let m = source.channel(0)?;
            let l = m.to_vec();
            let r = m.to_vec();
            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, vec![l, r])
        }

        // Stereo -> Mono
        (ChannelLayout::Stereo, ChannelLayout::Mono) => {
            let l = source.channel(0)?;
            let r = source.channel(1)?;
            let mut m = Vec::with_capacity(samples);
            for i in 0..samples {
                let sl = l.get(i).copied().unwrap_or(0.0);
                let sr = r.get(i).copied().unwrap_or(0.0);
                m.push(0.5 * (sl + sr));
            }
            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, vec![m])
        }

        // 5.1 -> Stereo (ITU-R BS.775 downmix: L = L + 0.7071*C + 0.7071*Ls; R = R + 0.7071*C + 0.7071*Rs)
        (ChannelLayout::Surround51, ChannelLayout::Stereo) => {
            let l_in = source.channel(0)?;
            let r_in = source.channel(1)?;
            let c_in = source.channel(2)?;
            // index 3 is LFE (ignored in downmix per BS.775)
            let ls_in = source.channel(4)?;
            let rs_in = source.channel(5)?;

            let coeff = std::f32::consts::FRAC_1_SQRT_2; // ~0.7071
            let norm = 1.0 / (1.0 + 2.0 * coeff); // prevent clipping

            let mut l_out = Vec::with_capacity(samples);
            let mut r_out = Vec::with_capacity(samples);

            for i in 0..samples {
                let l = l_in.get(i).copied().unwrap_or(0.0);
                let r = r_in.get(i).copied().unwrap_or(0.0);
                let c = c_in.get(i).copied().unwrap_or(0.0);
                let ls = ls_in.get(i).copied().unwrap_or(0.0);
                let rs = rs_in.get(i).copied().unwrap_or(0.0);

                l_out.push((l + coeff * c + coeff * ls) * norm);
                r_out.push((r + coeff * c + coeff * rs) * norm);
            }

            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, vec![l_out, r_out])
        }

        // 5.1 -> Mono
        (ChannelLayout::Surround51, ChannelLayout::Mono) => {
            let stereo = remix_channels(source, ChannelLayout::Stereo)?;
            remix_channels(&stereo, ChannelLayout::Mono)
        }

        // Stereo -> 5.1
        (ChannelLayout::Stereo, ChannelLayout::Surround51) => {
            let l_in = source.channel(0)?;
            let r_in = source.channel(1)?;

            let mut l_out = Vec::with_capacity(samples);
            let mut r_out = Vec::with_capacity(samples);
            let mut c_out = Vec::with_capacity(samples);
            let lfe_out = vec![0.0f32; samples];
            let mut ls_out = Vec::with_capacity(samples);
            let mut rs_out = Vec::with_capacity(samples);

            for i in 0..samples {
                let l = l_in.get(i).copied().unwrap_or(0.0);
                let r = r_in.get(i).copied().unwrap_or(0.0);

                l_out.push(l);
                r_out.push(r);
                c_out.push(0.5 * (l + r));
                ls_out.push(0.5 * (l - r));
                rs_out.push(0.5 * (r - l));
            }

            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, vec![l_out, r_out, c_out, lfe_out, ls_out, rs_out])
        }

        // 7.1 -> 5.1
        (ChannelLayout::Surround71, ChannelLayout::Surround51) => {
            let l = source.channel(0)?.to_vec();
            let r = source.channel(1)?.to_vec();
            let c = source.channel(2)?.to_vec();
            let lfe = source.channel(3)?.to_vec();
            let ls_in = source.channel(4)?;
            let rs_in = source.channel(5)?;
            let lb_in = source.channel(6)?;
            let rb_in = source.channel(7)?;

            let coeff = std::f32::consts::FRAC_1_SQRT_2;
            let mut ls_out = Vec::with_capacity(samples);
            let mut rs_out = Vec::with_capacity(samples);

            for i in 0..samples {
                let ls = ls_in.get(i).copied().unwrap_or(0.0);
                let rs = rs_in.get(i).copied().unwrap_or(0.0);
                let lb = lb_in.get(i).copied().unwrap_or(0.0);
                let rb = rb_in.get(i).copied().unwrap_or(0.0);

                ls_out.push(ls + coeff * lb);
                rs_out.push(rs + coeff * rb);
            }

            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, vec![l, r, c, lfe, ls_out, rs_out])
        }

        // 7.1 -> Stereo
        (ChannelLayout::Surround71, ChannelLayout::Stereo) => {
            let surround51 = remix_channels(source, ChannelLayout::Surround51)?;
            remix_channels(&surround51, ChannelLayout::Stereo)
        }

        // Fallback / identity or silent padding
        _ => {
            let target_channels = target_layout as usize;
            let mut channels = Vec::with_capacity(target_channels);
            for ch in 0..target_channels {
                if let Ok(src_ch) = source.channel(ch) {
                    channels.push(src_ch.to_vec());
                } else {
                    channels.push(vec![0.0f32; samples]);
                }
            }
            AudioSampleBuffer::from_planar_channels(sample_rate, target_layout, start_idx, channels)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stereo_to_mono_downmix() {
        let left = vec![1.0, 0.5, -0.5];
        let right = vec![1.0, -0.5, 0.5];
        let buf = AudioSampleBuffer::from_planar_channels(48000, ChannelLayout::Stereo, 0, vec![left, right]).unwrap();
        let mono = remix_channels(&buf, ChannelLayout::Mono).unwrap();

        assert_eq!(mono.channel_count(), 1);
        let m = mono.channel(0).unwrap();
        assert!((m[0] - 1.0).abs() < 1e-6);
        assert!(m[1].abs() < 1e-6);
        assert!(m[2].abs() < 1e-6);
    }

    #[test]
    fn test_51_to_stereo_downmix() {
        let zeros = vec![0.0; 10];
        let center = vec![1.0; 10];
        let buf = AudioSampleBuffer::from_planar_channels(
            48000,
            ChannelLayout::Surround51,
            0,
            vec![zeros.clone(), zeros.clone(), center, zeros.clone(), zeros.clone(), zeros],
        )
        .unwrap();

        let stereo = remix_channels(&buf, ChannelLayout::Stereo).unwrap();
        assert_eq!(stereo.channel_count(), 2);
        let l = stereo.channel(0).unwrap();
        let r = stereo.channel(1).unwrap();
        assert!((l[0] - r[0]).abs() < 1e-6);
        assert!(l[0] > 0.0);
    }
}
