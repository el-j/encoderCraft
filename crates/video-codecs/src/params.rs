//! Encoder parameters shared by every codec wrapper.

use encodercraft_core::{ColorTag, EncoderError, RateMode, VideoEncodeSettings};

/// Parameters a preset or target can ask of a video encoder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EncodeParams {
    pub bitrate_kbps: Option<u32>,
    pub keyframe_interval: Option<u32>,
    pub bframes: Option<u8>,
    pub rate_mode: RateMode,
    pub color: Option<ColorTag>,
    pub profile: Option<String>,
    /// Emit start-code (Annex B) NAL units instead of length-prefixed samples; used for MXF and transport streams.
    pub annex_b: bool,
}

impl EncodeParams {
    /// Builds parameters from target settings plus the target bitrate.
    pub fn from_settings(settings: &VideoEncodeSettings, bitrate_kbps: Option<u32>) -> Self {
        Self {
            bitrate_kbps,
            keyframe_interval: settings.keyframe_interval,
            bframes: settings.bframes,
            rate_mode: settings.rate_mode,
            color: settings.color,
            profile: settings.profile.clone(),
            annex_b: false,
        }
    }

    /// Rejects a request the codec cannot honour instead of silently ignoring it.
    pub(crate) fn reject_unsupported(&self, codec: &str, bframes: bool, hdr_color: bool, profile: bool) -> Result<(), EncoderError> {
        if !bframes && self.bframes.is_some_and(|b| b > 0) {
            return Err(EncoderError::UnsupportedCodec(format!("{codec} encoder does not support B-frames")));
        }
        if !hdr_color && self.color.is_some_and(|c| c != ColorTag::Bt709) {
            return Err(EncoderError::UnsupportedCodec(format!("{codec} encoder cannot signal HDR colour tags")));
        }
        if !profile && let Some(p) = &self.profile {
            return Err(EncoderError::UnsupportedCodec(format!("{codec} encoder has no selectable profile (requested `{p}`)")));
        }
        Ok(())
    }
}
