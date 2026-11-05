//! Lossy video conversion assessment and format-pair consent records.

use encodercraft_color::{ColorPrimaries, ColorSpaceTag, PixelFormat, TransferFunction};
use encodercraft_core::{ColorTag, ContainerType, CraftJobSpec, EncoderError, SourceDescriptor};
use encodercraft_pipeline::target_pixel_format;
use encodercraft_source::{MediaReader, VideoProbe};
use serde::{Deserialize, Serialize};

/// Exact source-to-target pair used to persist a user's allow or deny choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossyConversionKey {
    pub source_pixel_format: String,
    pub source_bit_depth: u8,
    pub source_color_space: ColorSpaceTag,
    pub source_has_alpha: bool,
    pub target_pixel_format: PixelFormat,
    pub target_color: ColorTag,
}

/// A specific irreversible change required by a conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LossyConversionChange {
    /// The decoder currently supplies 8-bit RGB frames, so higher-precision source samples lose
    /// precision before the selected encoder receives them.
    BitDepthReduction {
        source_bits: u8,
        processing_bits: u8,
        output_bits: u8,
    },
    HdrToSdr,
    AlphaDiscarded,
    GamutReduction {
        source: ColorPrimaries,
        target: ColorPrimaries,
    },
}

impl LossyConversionChange {
    /// Human-readable explanation suitable for a confirmation prompt.
    pub fn description(&self) -> String {
        match self {
            Self::BitDepthReduction { source_bits, processing_bits, output_bits } if processing_bits > source_bits => format!(
                "{source_bits}-bit source precision is preserved through {processing_bits}-bit floating-point processing, then quantized to {output_bits}-bit output"
            ),
            Self::BitDepthReduction { source_bits, processing_bits, output_bits } => format!(
                "{source_bits}-bit source precision is reduced to {processing_bits}-bit processing before the {output_bits}-bit encoder; output bit depth cannot restore it"
            ),
            Self::HdrToSdr => "HDR transfer is tone-mapped to BT.709 SDR".into(),
            Self::AlphaDiscarded => "the source alpha channel is discarded by the selected video encoder".into(),
            Self::GamutReduction { source, target } => format!("color gamut is reduced from {source:?} to {target:?}"),
        }
    }
}

/// A conversion request that needs a user's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossyConversion {
    pub key: LossyConversionKey,
    pub changes: Vec<LossyConversionChange>,
}

impl LossyConversion {
    pub fn description(&self) -> String {
        self.changes.iter().map(LossyConversionChange::description).collect::<Vec<_>>().join("; ")
    }
}

/// Persistent allow/deny choice for one exact source-to-target conversion pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionDecision {
    pub key: LossyConversionKey,
    pub allowed: bool,
}

/// Finds all lossy video conversions in a job. Audio-only and non-video targets need no decision.
pub fn assess_lossy_conversions(spec: &CraftJobSpec) -> Result<Vec<LossyConversion>, EncoderError> {
    let paths = match &spec.source {
        SourceDescriptor::File { path, .. } => vec![path.as_path()],
        SourceDescriptor::StitchedClips { clips } => clips.iter().map(|clip| clip.path.as_path()).collect(),
        SourceDescriptor::Pipe { .. } | SourceDescriptor::ProjectReference { .. } => return Ok(Vec::new()),
    };
    if !spec.targets.iter().any(|target| is_video_target(target.container)) {
        return Ok(Vec::new());
    }

    let mut conversions = Vec::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let reader = MediaReader::open(path)?;
        let Some(source) = reader.probe().video.as_ref() else { continue };
        for target in spec.targets.iter().filter(|target| is_video_target(target.container)) {
            let Some(conversion) = conversion_for_source(source, target)? else {
                continue;
            };
            if !conversions.iter().any(|existing: &LossyConversion| existing.key == conversion.key) {
                conversions.push(conversion);
            }
        }
    }
    Ok(conversions)
}

fn conversion_for_source(source: &VideoProbe, target: &encodercraft_core::OutputTarget) -> Result<Option<LossyConversion>, EncoderError> {
    let target_pixel_format = target_pixel_format(target)?;
    let target_color = target.video.color.unwrap_or(ColorTag::Bt709);
    let output_bits = target_pixel_format.bits_per_component();
    let processing_bits = if source.bit_depth > 8 { 32 } else { 8 };
    let target_primaries = match target_color {
        ColorTag::Bt709 => ColorPrimaries::Bt709,
        ColorTag::Pq | ColorTag::Hlg => ColorPrimaries::Bt2020,
    };
    let mut changes = Vec::new();
    if source.bit_depth > output_bits {
        changes.push(LossyConversionChange::BitDepthReduction { source_bits: source.bit_depth, processing_bits, output_bits });
    }
    if source.has_alpha {
        changes.push(LossyConversionChange::AlphaDiscarded);
    }
    if matches!(source.color_space.transfer, TransferFunction::PerceptualQuantizer | TransferFunction::HybridLogGamma)
        && target_color == ColorTag::Bt709
    {
        changes.push(LossyConversionChange::HdrToSdr);
    }
    if source.color_space.primaries != target_primaries
        && target_primaries == ColorPrimaries::Bt709
        && matches!(source.color_space.primaries, ColorPrimaries::Bt2020 | ColorPrimaries::DisplayP3 | ColorPrimaries::DciP3)
    {
        changes.push(LossyConversionChange::GamutReduction { source: source.color_space.primaries, target: target_primaries });
    }
    if changes.is_empty() {
        return Ok(None);
    }
    Ok(Some(LossyConversion {
        key: LossyConversionKey {
            source_pixel_format: source.pixel_format.clone(),
            source_bit_depth: source.bit_depth,
            source_color_space: source.color_space,
            source_has_alpha: source.has_alpha,
            target_pixel_format,
            target_color,
        },
        changes,
    }))
}

fn is_video_target(container: ContainerType) -> bool {
    matches!(
        container,
        ContainerType::IsoBmffMp4
            | ContainerType::QuickTimeMov
            | ContainerType::MatroskaMkv
            | ContainerType::WebM
            | ContainerType::MxfOp1a
            | ContainerType::MxfOpAtom
            | ContainerType::AnimatedGif
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::OutputTarget;

    fn source(bits: u8, color_space: ColorSpaceTag, has_alpha: bool) -> VideoProbe {
        VideoProbe {
            width: 1920,
            height: 1080,
            frame_rate: encodercraft_core::Rational64::FPS_24,
            codec: "test".into(),
            pixel_format: "yuv422p10le".into(),
            bit_depth: bits,
            color_space,
            has_alpha,
            bitrate: None,
        }
    }

    fn target(pixel_format: PixelFormat, color: Option<ColorTag>) -> OutputTarget {
        OutputTarget {
            preset_name: "test".into(),
            container: ContainerType::IsoBmffMp4,
            custom_width: Some(1920),
            custom_height: Some(1080),
            video: encodercraft_core::VideoEncodeSettings { pixel_format: Some(pixel_format), color, ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn reports_all_confirmable_loss_types_for_high_bit_depth_hdr_alpha_source() {
        let source = source(10, ColorSpaceTag::HDR10, true);
        let conversion = conversion_for_source(&source, &target(PixelFormat::Yuv420p, None)).unwrap().unwrap();
        assert_eq!(conversion.changes.len(), 4);
        assert!(conversion.changes.contains(&LossyConversionChange::BitDepthReduction { source_bits: 10, processing_bits: 32, output_bits: 8 }));
        assert!(conversion.changes.contains(&LossyConversionChange::HdrToSdr));
        assert!(conversion.changes.contains(&LossyConversionChange::AlphaDiscarded));
        assert!(
            conversion.changes.contains(&LossyConversionChange::GamutReduction { source: ColorPrimaries::Bt2020, target: ColorPrimaries::Bt709 })
        );
    }

    #[test]
    fn equivalent_eight_bit_bt709_conversion_needs_no_confirmation() {
        let source = source(8, ColorSpaceTag::REC709, false);
        assert!(conversion_for_source(&source, &target(PixelFormat::Yuv420p, None)).unwrap().is_none());
    }

    #[test]
    fn ten_bit_encoder_preserves_ten_bit_source_precision() {
        let source = source(10, ColorSpaceTag::REC709, false);
        let target = OutputTarget {
            preset_name: "ProRes HQ".into(),
            container: ContainerType::QuickTimeMov,
            video: encodercraft_core::VideoEncodeSettings {
                codec: Some("prores".into()),
                profile: Some("hq".into()),
                pixel_format: Some(PixelFormat::Yuv422p10),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(conversion_for_source(&source, &target).unwrap().is_none());
    }

    #[test]
    fn settings_pair_changes_the_persisted_key() {
        let source = source(10, ColorSpaceTag::REC709, false);
        let a = conversion_for_source(&source, &target(PixelFormat::Yuv420p, None)).unwrap().unwrap();
        let b = conversion_for_source(&source, &target(PixelFormat::Yuv420p, Some(ColorTag::Pq))).unwrap().unwrap();
        assert_ne!(a.key, b.key);
    }
}
