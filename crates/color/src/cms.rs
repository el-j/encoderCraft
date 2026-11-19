//! Color management and HDR gain mapping wrappers backed by `craft-cms` and `craft-gainmap`.

use std::path::Path;
use std::sync::Arc;

use craft_cms::{Intent, Profile, Transform};
use thiserror::Error;

/// Color management errors.
#[derive(Debug, Clone, Error)]
pub enum CmsError {
    #[error("failed to read or parse ICC profile: {0}")]
    Profile(String),

    #[error("CMS transform error: {0}")]
    Transform(String),

    #[error("HDR gain map error: {0}")]
    GainMap(String),
}

/// Managed ICC color transform between source and destination profiles.
#[derive(Clone)]
pub struct IccColorTransform {
    transform: Arc<Transform>,
}

impl IccColorTransform {
    /// Creates a transform from source ICC profile bytes to destination profile bytes.
    pub fn new(src_bytes: &[u8], dst_bytes: &[u8]) -> Result<Self, CmsError> {
        let src = Profile::parse(src_bytes).map_err(|e| CmsError::Profile(format!("{e:?}")))?;
        let dst = Profile::parse(dst_bytes).map_err(|e| CmsError::Profile(format!("{e:?}")))?;
        let t = Transform::new(&src, &dst, Intent::Perceptual, true).map_err(|e| CmsError::Transform(format!("{e:?}")))?;
        Ok(Self { transform: Arc::new(t) })
    }

    /// Creates a transform from an ICC profile file on disk to standard sRGB.
    pub fn from_profile_file_to_srgb(path: &Path) -> Result<Self, CmsError> {
        let bytes = std::fs::read(path).map_err(|e| CmsError::Profile(format!("{e}")))?;
        let src = Profile::parse(&bytes).map_err(|e| CmsError::Profile(format!("{e:?}")))?;
        let dst = craft_cms::builtin::srgb();
        let t = Transform::new(&src, dst, Intent::Perceptual, true).map_err(|e| CmsError::Transform(format!("{e:?}")))?;
        Ok(Self { transform: Arc::new(t) })
    }

    /// Transforms packed 8-bit RGBA pixel buffer in-place.
    pub fn transform_rgba8(&self, data: &mut [u8]) -> Result<(), CmsError> {
        if data.is_empty() {
            return Ok(());
        }
        let tmp = data.to_vec();
        self.transform.convert_u8(&tmp, 4, data, 4, true).map_err(|e| CmsError::Transform(format!("{e:?}")))
    }
}

/// Applies an ISO 21496-1 HDR gain map to an RGBA8 base buffer.
pub fn apply_hdr_gainmap_to_rgba8(
    base: &mut [u8],
    width: usize,
    height: usize,
    gainmap: &[u8],
    gm_width: usize,
    gm_height: usize,
    headroom: f32,
) -> Result<(), CmsError> {
    if base.is_empty() || gainmap.is_empty() || width == 0 || height == 0 || gm_width == 0 || gm_height == 0 {
        return Ok(());
    }
    let meta = craft_gainmap::GainMapMeta {
        multichannel: false,
        use_base_colour_space: true,
        min: [0.0; 3],
        max: [headroom.max(1.0).log2(); 3],
        gamma: [1.0; 3],
        base_offset: [0.015625; 3],
        alternate_offset: [0.015625; 3],
        base_headroom: 0.0,
        alternate_headroom: headroom.max(1.0).log2(),
    };

    let weight = 1.0f32.min((headroom - 1.0) / headroom.max(1.0));
    let channels = 4;
    for y in 0..height {
        let gm_y = (y * gm_height) / height;
        for x in 0..width {
            let gm_x = (x * gm_width) / width;
            let gm_idx = (gm_y * gm_width + gm_x) * channels;
            let gm_val = f32::from(*gainmap.get(gm_idx).unwrap_or(&128)) / 255.0;
            let gain = (gm_val * weight * (meta.max[0] - meta.min[0])).exp2();

            let base_idx = (y * width + x) * channels;
            if let Some(pixel) = base.get_mut(base_idx..base_idx + 3) {
                for comp in pixel.iter_mut() {
                    let linear = f32::from(*comp) / 255.0;
                    let boosted = (linear * gain).clamp(0.0, 1.0);
                    *comp = (boosted * 255.0).round() as u8;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cms_builtin_transforms() {
        let srgb_bytes = craft_cms::builtin::srgb_v2();
        let display_p3 = craft_cms::builtin::Builtin::DisplayP3.profile();
        let p3_bytes = display_p3.to_bytes();

        let transform = IccColorTransform::new(&srgb_bytes, &p3_bytes).unwrap();
        let mut pixels = [128u8, 64, 32, 255];
        transform.transform_rgba8(&mut pixels).unwrap();
        assert_eq!(pixels[3], 255); // Alpha preserved
    }

    #[test]
    fn test_gainmap_apply_boosts_highlights() {
        let mut base = [100u8, 100, 100, 255];
        let gainmap = [255u8, 255, 255, 255]; // Max gain
        apply_hdr_gainmap_to_rgba8(&mut base, 1, 1, &gainmap, 1, 1, 4.0).unwrap();
        assert!(base[0] > 100);
    }
}
