//! Still-image encoding through the ArtCraft `photocraft-codecs` crate.
//!
//! Frames arrive as straight-alpha RGBA8 (or RGB8). Formats without an alpha channel receive the
//! image flattened onto a background colour, never a silently dropped alpha channel.

use encodercraft_color::PixelFormat;
use encodercraft_core::{EncoderError, VideoFrame};
use photocraft_codecs::{
    ChannelLayout, DdsCompression, DdsOptions, DpxBits, EncodeOptions, ExrCompression, Format, Image, JpegFormat, PngCompression, TiffCompression,
};

/// Still formats the encoder can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StillFormat {
    Png,
    Jpeg,
    Tiff,
    WebP,
    Gif,
    Bmp,
    OpenExr,
    Dds,
    Dpx,
    Jxl,
    RadianceHdr,
    Dicom,
    Fits,
}

impl StillFormat {
    /// Every writable format.
    pub const ALL: [StillFormat; 13] = [
        Self::Png,
        Self::Jpeg,
        Self::Tiff,
        Self::WebP,
        Self::Gif,
        Self::Bmp,
        Self::OpenExr,
        Self::Dds,
        Self::Dpx,
        Self::Jxl,
        Self::RadianceHdr,
        Self::Dicom,
        Self::Fits,
    ];

    /// Canonical file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::OpenExr => "exr",
            Self::Dds => "dds",
            Self::Dpx => "dpx",
            Self::Jxl => "jxl",
            Self::RadianceHdr => "hdr",
            Self::Dicom => "dcm",
            Self::Fits => "fits",
        }
    }

    /// Whether the format can carry transparency.
    pub fn has_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Tiff | Self::WebP | Self::Gif | Self::OpenExr | Self::Dds | Self::Dpx | Self::Jxl)
    }

    fn codec_format(self) -> Format {
        match self {
            Self::Png => Format::Png,
            Self::Jpeg => Format::Jpeg,
            Self::Tiff => Format::Tiff,
            Self::WebP => Format::WebP,
            Self::Gif => Format::Gif,
            Self::Bmp => Format::Bmp,
            Self::OpenExr => Format::OpenExr,
            Self::Dds => Format::Dds,
            Self::Dpx => Format::Dpx,
            Self::Jxl => Format::Jxl,
            Self::RadianceHdr => Format::Hdr,
            Self::Dicom => Format::Dicom,
            Self::Fits => Format::Fits,
        }
    }
}

/// Tunable encoder parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct StillOptions {
    /// JPEG and lossy WebP quality, 1..=100.
    pub quality: u8,
    /// JPEG 4:2:0 chroma subsampling.
    pub jpeg_subsampling: bool,
    pub jpeg_progressive: bool,
    /// WebP lossless (VP8L) instead of lossy.
    pub webp_lossless: bool,
    pub png_compression: PngCompression,
    pub tiff_compression: TiffCompression,
    pub exr_compression: ExrCompression,
    pub dds_compression: DdsCompression,
    pub dpx_bits: Option<DpxBits>,
    /// Colour that transparent pixels are composited onto for formats without alpha.
    pub background: [u8; 3],
    /// Drop EXIF/XMP/ICC instead of embedding them.
    pub strip_metadata: bool,
}

impl Default for StillOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            jpeg_subsampling: true,
            jpeg_progressive: false,
            webp_lossless: true,
            png_compression: PngCompression::Default,
            tiff_compression: TiffCompression::Deflate,
            exr_compression: ExrCompression::Zip16,
            dds_compression: DdsCompression::None,
            dpx_bits: None,
            background: [255, 255, 255],
            strip_metadata: false,
        }
    }
}

fn codec_err(e: photocraft_codecs::CodecError) -> EncoderError {
    EncoderError::InvalidContainer(format!("image encode failed: {e}"))
}

/// Converts a pipeline frame into straight RGBA8 pixels, or RGB8 when `drop_alpha` composites it
/// onto `background`.
fn frame_pixels(frame: &VideoFrame, drop_alpha: bool, background: [u8; 3]) -> Result<(u32, u32, Vec<u8>, bool), EncoderError> {
    let (w, h) = (frame.dimensions.width, frame.dimensions.height);
    if w == 0 || h == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode an image with zero dimensions".into()));
    }
    let target_format = if drop_alpha { PixelFormat::Rgb8 } else { PixelFormat::Rgba8 };
    let data = frame.data();
    let opts = encodercraft_color::ConvertOptions { background, ..Default::default() };
    let pixels = encodercraft_color::convert_pixels_with_options(data, frame.format, target_format, w, h, &opts)
        .map_err(|e| EncoderError::Internal(format!("pixel conversion failed: {e}")))?;
    Ok((w, h, pixels, !drop_alpha))
}

/// Encodes `frame` in `format`.
pub fn encode_still(frame: &VideoFrame, format: StillFormat, options: &StillOptions) -> Result<Vec<u8>, EncoderError> {
    let (w, h) = (frame.dimensions.width, frame.dimensions.height);
    if w == 0 || h == 0 {
        return Err(EncoderError::InvalidJobSpec("cannot encode an image with zero dimensions".into()));
    }

    if format == StillFormat::Dicom {
        return crate::dicom::encode_dicom(frame);
    }

    let opts = EncodeOptions {
        jpeg_quality: options.quality.clamp(1, 100),
        jpeg_chroma_subsampling: options.jpeg_subsampling,
        jpeg_format: if options.jpeg_progressive { JpegFormat::Progressive } else { JpegFormat::Baseline },
        png_compression: options.png_compression,
        webp_lossless: options.webp_lossless,
        webp_quality: options.quality.clamp(1, 100),
        tiff_compression: options.tiff_compression,
        exr_compression: options.exr_compression,
        dpx_bits: options.dpx_bits,
        dds: DdsOptions { compression: options.dds_compression, mipmaps: false },
        embed_icc: !options.strip_metadata,
        embed_metadata: !options.strip_metadata,
        ..EncodeOptions::default()
    };

    if frame.format == PixelFormat::Rgba16 && format == StillFormat::Tiff {
        let (plane, _) = frame.plane(0)?;
        let mut u16_samples = Vec::with_capacity((w as usize).saturating_mul(h as usize).saturating_mul(4));
        for px in plane.as_chunks::<8>().0 {
            u16_samples.push(u16::from_le_bytes([px[0], px[1]]));
            u16_samples.push(u16::from_le_bytes([px[2], px[3]]));
            u16_samples.push(u16::from_le_bytes([px[4], px[5]]));
            u16_samples.push(u16::from_le_bytes([px[6], px[7]]));
        }
        let image = Image::from_u16(w, h, ChannelLayout::Rgba, &u16_samples).map_err(codec_err)?;
        return photocraft_codecs::encode(&image, format.codec_format(), &opts).map_err(codec_err);
    }

    let (w, h, pixels, has_alpha) = frame_pixels(frame, !format.has_alpha(), options.background)?;
    let layout = if has_alpha { ChannelLayout::Rgba } else { ChannelLayout::Rgb };
    let image = Image::from_u8(w, h, layout, pixels).map_err(codec_err)?;
    photocraft_codecs::encode(&image, format.codec_format(), &opts).map_err(codec_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::ColorSpaceTag;
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;
    use photocraft_codecs::DecodeOptions;

    const W: u32 = 12;
    const H: u32 = 8;

    /// Left half red, right half blue, top row half transparent.
    fn fixture() -> VideoFrame {
        let mut rgba = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let (r, b) = if x < W / 2 { (255, 0) } else { (0, 255) };
                let a = if y == 0 { 128 } else { 255 };
                rgba.extend_from_slice(&[r, 40, b, a]);
            }
        }
        VideoFrame::from_raw_bytes(
            Dimensions { width: W, height: H },
            PixelFormat::Rgba8,
            ColorSpaceTag::SRGB,
            0,
            1,
            Rational64 { num: 1, den: 1 },
            rgba,
        )
        .unwrap()
    }

    fn decode(bytes: &[u8]) -> Vec<u8> {
        let img = photocraft_codecs::decode_with(bytes, &DecodeOptions::default()).unwrap();
        assert_eq!(img.dimensions(), (W, H));
        img.to_rgba8()
    }

    fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn lossless_formats_round_trip_exactly_including_alpha() {
        let source = fixture();
        let expected = source.data().to_vec();
        for format in [StillFormat::Png, StillFormat::Tiff, StillFormat::OpenExr, StillFormat::WebP] {
            let bytes = encode_still(&source, format, &StillOptions::default()).unwrap();
            let got = decode(&bytes);
            if format == StillFormat::OpenExr {
                // Half float keeps 8-bit values within one step.
                assert!(got.iter().zip(&expected).all(|(a, b)| a.abs_diff(*b) <= 1), "{format:?}");
            } else {
                assert_eq!(got, expected, "{format:?} must be lossless");
            }
        }
    }

    #[test]
    fn bmp_flattens_transparency_onto_the_background() {
        let opts = StillOptions { background: [0, 0, 0], ..StillOptions::default() };
        let bytes = encode_still(&fixture(), StillFormat::Bmp, &opts).unwrap();
        let got = decode(&bytes);
        assert_eq!(pixel(&got, 0, 4), [255, 40, 0, 255]);
        // 50% red over black is about half red.
        let top = pixel(&got, 0, 0);
        assert!(top[0].abs_diff(128) <= 1 && top[3] == 255, "{top:?}");
    }

    #[test]
    fn jpeg_is_close_and_opaque() {
        let opts = StillOptions { quality: 100, jpeg_subsampling: false, ..StillOptions::default() };
        let bytes = encode_still(&fixture(), StillFormat::Jpeg, &opts).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        let got = decode(&bytes);
        let p = pixel(&got, 2, 4);
        assert!(p[0] > 240 && p[2] < 20 && p[3] == 255, "{p:?}");
    }

    #[test]
    fn gif_keeps_the_two_flat_colours() {
        let bytes = encode_still(&fixture(), StillFormat::Gif, &StillOptions::default()).unwrap();
        assert_eq!(&bytes[..3], b"GIF");
        let got = decode(&bytes);
        let (l, r) = (pixel(&got, 1, 4), pixel(&got, 10, 4));
        assert!(l[0] > 200 && l[2] < 60, "{l:?}");
        assert!(r[2] > 200 && r[0] < 60, "{r:?}");
    }

    #[test]
    fn every_format_has_a_matching_signature() {
        let sigs: [(StillFormat, &[u8]); 12] = [
            (StillFormat::Png, &[0x89, b'P', b'N', b'G']),
            (StillFormat::Jpeg, &[0xFF, 0xD8]),
            (StillFormat::Tiff, b"II"),
            (StillFormat::WebP, b"RIFF"),
            (StillFormat::Gif, b"GIF8"),
            (StillFormat::Bmp, b"BM"),
            (StillFormat::OpenExr, &[0x76, 0x2F, 0x31, 0x01]),
            (StillFormat::Dds, b"DDS "),
            (StillFormat::Dpx, b"SDPX"),
            (StillFormat::Jxl, &[0xFF, 0x0A]),
            (StillFormat::RadianceHdr, b"#?RADIANCE"),
            (StillFormat::Fits, b"SIMPLE  ="),
        ];
        for (format, sig) in sigs {
            let bytes = encode_still(&fixture(), format, &StillOptions::default()).unwrap();
            assert!(
                bytes.starts_with(sig)
                    || (format == StillFormat::Tiff && bytes.starts_with(b"MM"))
                    || (format == StillFormat::Dpx && bytes.starts_with(b"XPDS"))
                    || (format == StillFormat::RadianceHdr && bytes.starts_with(b"#?RGBE")),
                "{format:?}"
            );
        }

        // DICOM preamble check
        let dcm = encode_still(&fixture(), StillFormat::Dicom, &StillOptions::default()).unwrap();
        assert_eq!(&dcm[128..132], b"DICM");
    }

    #[test]
    fn dds_and_dpx_roundtrip() {
        let source = fixture();
        let expected = source.data().to_vec();

        // DPX lossless roundtrip
        let dpx_bytes = encode_still(&source, StillFormat::Dpx, &StillOptions::default()).unwrap();
        assert!(dpx_bytes.starts_with(b"SDPX") || dpx_bytes.starts_with(b"XPDS"));
        let got_dpx = decode(&dpx_bytes);
        assert_eq!(got_dpx, expected);

        // DDS uncompressed roundtrip
        let dds_bytes = encode_still(&source, StillFormat::Dds, &StillOptions::default()).unwrap();
        assert!(dds_bytes.starts_with(b"DDS "));
        let got_dds = decode(&dds_bytes);
        assert_eq!(got_dds, expected);

        // JXL bare codestream signature check
        let jxl_bytes = encode_still(&source, StillFormat::Jxl, &StillOptions::default()).unwrap();
        assert_eq!(&jxl_bytes[..2], &[0xFF, 0x0A]);

        // FITS roundtrip
        let fits_bytes = encode_still(&source, StillFormat::Fits, &StillOptions::default()).unwrap();
        assert!(fits_bytes.starts_with(b"SIMPLE  ="));
        let got_fits = decode(&fits_bytes);
        assert_eq!(got_fits.len(), expected.len());

        // DICOM roundtrip
        let dcm_bytes = encode_still(&source, StillFormat::Dicom, &StillOptions::default()).unwrap();
        assert_eq!(&dcm_bytes[128..132], b"DICM");
        let got_dcm = decode(&dcm_bytes);
        assert_eq!(got_dcm.len(), expected.len());
    }

    #[test]
    fn accepts_canonical_pixel_formats() {
        let frame = VideoFrame::new_allocated(
            Dimensions { width: 2, height: 2 },
            PixelFormat::Yuv420p,
            ColorSpaceTag::REC709,
            0,
            1,
            Rational64 { num: 1, den: 1 },
        )
        .unwrap();
        assert!(encode_still(&frame, StillFormat::Png, &StillOptions::default()).is_ok());

        let frame16 = VideoFrame::new_allocated(
            Dimensions { width: 2, height: 2 },
            PixelFormat::Rgba16,
            ColorSpaceTag::SRGB,
            0,
            1,
            Rational64 { num: 1, den: 1 },
        )
        .unwrap();
        assert!(encode_still(&frame16, StillFormat::Tiff, &StillOptions::default()).is_ok());
    }
}
