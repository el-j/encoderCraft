//! Rapid magic-byte container sniffer for multi-media files.
//!
//! Inspects only the initial header bytes without decoding or allocating large buffers.

use serde::{Deserialize, Serialize};

/// Identifies the media container or format classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContainerKind {
    // Video containers
    IsoBmffMp4,
    QuickTimeMov,
    Matroska,
    WebM,
    Mxf,
    MpegTs,
    // Audio containers
    Wav,
    Rf64Wav,
    Aiff,
    Flac,
    Mp3,
    Ogg,
    // Still & raster formats
    Png,
    Jpeg,
    WebP,
    Tiff,
    OpenExr,
    RadianceHdr,
    Dds,
    Gif,
    // 3D & spatial assets
    GltfBinary,
    GltfJson,
    UsdBinary,
    UsdAscii,
    Obj,
    StlBinary,
    StlAscii,
    Ply,
    // Documents & vector graphics
    Pdf,
    Svg,
    Docx,
    Pptx,
    Xlsx,
    Idml,
    Indd,
    IllustratorAi,
    Epub,
    Psd,
    Psb,
    Bmp,
    Eps,
    // Adobe Project Containers
    AdobeAfterEffectsAep,
    AdobePremierePrproj,
    AdobeAuditionSesx,
    // CAD & Technical Drafting
    AutocadDxf,
    AutocadDwg,
    HpglPlotter,
    // Native ArtCraft Creative Formats
    ArtcraftDesigncraft,
    ArtcraftPhotocraft,
    ArtcraftVectorcraft,
    ArtcraftEffectcraft,
    ArtcraftFilmcraft,
    ArtcraftSoundcraft,
    ArtcraftCadcraft,
    ArtcraftAnicraft,
}

impl ContainerKind {
    /// Friendly display name for the container.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::IsoBmffMp4 => "MP4 (ISO Base Media)",
            Self::QuickTimeMov => "QuickTime Movie (MOV)",
            Self::Matroska => "Matroska Video (MKV)",
            Self::WebM => "WebM",
            Self::Mxf => "Material Exchange Format (MXF)",
            Self::MpegTs => "MPEG Transport Stream",
            Self::Wav => "Waveform Audio (WAV)",
            Self::Rf64Wav => "RF64 Broadcast Wave (>4GB)",
            Self::Aiff => "Audio Interchange (AIFF)",
            Self::Flac => "Free Lossless Audio Codec (FLAC)",
            Self::Mp3 => "MPEG Audio Layer 3 (MP3)",
            Self::Ogg => "Ogg Container",
            Self::Png => "Portable Network Graphics (PNG)",
            Self::Jpeg => "JPEG Image",
            Self::WebP => "WebP Image",
            Self::Tiff => "Tagged Image File Format (TIFF)",
            Self::OpenExr => "ILM OpenEXR High Dynamic Range",
            Self::RadianceHdr => "Radiance HDR (.hdr)",
            Self::Dds => "DirectDraw Surface (DDS)",
            Self::Gif => "Graphics Interchange Format (GIF)",
            Self::GltfBinary => "glTF 2.0 Binary (.glb)",
            Self::GltfJson => "glTF 2.0 JSON (.gltf)",
            Self::UsdBinary => "USD Crate Binary (.usdc / .usdz)",
            Self::UsdAscii => "USD Ascii (.usda)",
            Self::Obj => "Wavefront OBJ",
            Self::StlBinary => "Stereolithography Binary (STL)",
            Self::StlAscii => "Stereolithography ASCII (STL)",
            Self::Ply => "Stanford Polygon File (PLY)",
            Self::Pdf => "Portable Document Format (PDF)",
            Self::Svg => "Scalable Vector Graphics (SVG)",
            Self::Docx => "Microsoft Word Document (DOCX)",
            Self::Pptx => "Microsoft PowerPoint (PPTX)",
            Self::Xlsx => "Microsoft Excel Workbook (XLSX)",
            Self::Idml => "Adobe InDesign Markup (IDML)",
            Self::Indd => "Adobe InDesign Document (INDD)",
            Self::IllustratorAi => "Adobe Illustrator Artwork (AI)",
            Self::Epub => "Electronic Publication (EPUB)",
            Self::Psd => "Adobe Photoshop Document (PSD)",
            Self::Psb => "Adobe Photoshop Large Document (PSB)",
            Self::Bmp => "Windows Bitmap (BMP)",
            Self::Eps => "Encapsulated PostScript (EPS)",
            Self::AdobeAfterEffectsAep => "Adobe After Effects Project (AEP)",
            Self::AdobePremierePrproj => "Adobe Premiere Pro Project (PRPROJ)",
            Self::AdobeAuditionSesx => "Adobe Audition Session (SESX)",
            Self::AutocadDxf => "AutoCAD Drawing Exchange (DXF)",
            Self::AutocadDwg => "AutoCAD Drawing (DWG)",
            Self::HpglPlotter => "HPGL / PLT Plotter Stream",
            Self::ArtcraftDesigncraft => "DesignCraft Publication (.designcraft)",
            Self::ArtcraftPhotocraft => "PhotoCraft Composite (.photocraft)",
            Self::ArtcraftVectorcraft => "VectorCraft Artwork (.vectorcraft)",
            Self::ArtcraftEffectcraft => "EffectCraft Composition (.effectcraft)",
            Self::ArtcraftFilmcraft => "FilmCraft Timeline (.filmcraft)",
            Self::ArtcraftSoundcraft => "SoundCraft Session (.soundcraft)",
            Self::ArtcraftCadcraft => "CadCraft Blueprint (.cadcraft)",
            Self::ArtcraftAnicraft => "AniCraft Animation (.anicraft)",
        }
    }

    /// Maps this container kind to an `encodercraft_core::ContainerType` if directly transcodable.
    pub fn to_container_type(&self) -> Option<encodercraft_core::ContainerType> {
        use encodercraft_core::ContainerType;
        match self {
            Self::IsoBmffMp4 => Some(ContainerType::IsoBmffMp4),
            Self::QuickTimeMov => Some(ContainerType::QuickTimeMov),
            Self::Matroska => Some(ContainerType::MatroskaMkv),
            Self::WebM => Some(ContainerType::WebM),
            Self::Mxf => Some(ContainerType::MxfOp1a),
            Self::MpegTs => Some(ContainerType::IsoBmffMp4),
            Self::Wav | Self::Rf64Wav => Some(ContainerType::Wave),
            Self::Aiff => Some(ContainerType::Aiff),
            Self::Flac => Some(ContainerType::Flac),
            Self::Mp3 => Some(ContainerType::Mp3),
            Self::Ogg => Some(ContainerType::OggAudio),
            Self::Png => Some(ContainerType::Png),
            Self::Jpeg => Some(ContainerType::Jpeg),
            Self::WebP => Some(ContainerType::WebP),
            Self::Tiff => Some(ContainerType::Tiff),
            Self::OpenExr | Self::RadianceHdr => Some(ContainerType::OpenExr),
            Self::Dds => Some(ContainerType::Tiff),
            Self::Gif => Some(ContainerType::AnimatedGif),
            Self::GltfBinary | Self::GltfJson => Some(ContainerType::Gltf),
            Self::UsdBinary | Self::UsdAscii => Some(ContainerType::Usdz),
            Self::Obj => Some(ContainerType::Obj),
            Self::StlBinary | Self::StlAscii => Some(ContainerType::Stl),
            Self::Ply => Some(ContainerType::Ply),
            Self::Pdf => Some(ContainerType::Pdf),
            Self::Svg => Some(ContainerType::Svg),
            Self::Docx | Self::Pptx | Self::Xlsx | Self::Epub => Some(ContainerType::Docx),
            Self::Idml => Some(ContainerType::Idml),
            Self::Indd => Some(ContainerType::AdobeInDesignIndd),
            Self::IllustratorAi => Some(ContainerType::AdobeIllustratorAi),
            Self::Psd => Some(ContainerType::AdobePhotoshopPsd),
            Self::Psb => Some(ContainerType::AdobePhotoshopPsb),
            Self::Bmp => Some(ContainerType::Bmp),
            Self::Eps => Some(ContainerType::PostScriptEps),
            Self::AdobeAfterEffectsAep => Some(ContainerType::AdobeAfterEffectsAep),
            Self::AdobePremierePrproj => Some(ContainerType::AdobePremierePrproj),
            Self::AdobeAuditionSesx => Some(ContainerType::AdobeAuditionSesx),
            Self::AutocadDxf => Some(ContainerType::AutocadDxf),
            Self::AutocadDwg => Some(ContainerType::AutocadDwg),
            Self::HpglPlotter => Some(ContainerType::HpglPlotter),
            Self::ArtcraftDesigncraft => Some(ContainerType::ArtcraftDesigncraft),
            Self::ArtcraftPhotocraft => Some(ContainerType::ArtcraftPhotocraft),
            Self::ArtcraftVectorcraft => Some(ContainerType::ArtcraftVectorcraft),
            Self::ArtcraftEffectcraft => Some(ContainerType::ArtcraftEffectcraft),
            Self::ArtcraftFilmcraft => Some(ContainerType::ArtcraftFilmcraft),
            Self::ArtcraftSoundcraft => Some(ContainerType::ArtcraftSoundcraft),
            Self::ArtcraftCadcraft => Some(ContainerType::ArtcraftCadcraft),
            Self::ArtcraftAnicraft => Some(ContainerType::ArtcraftAnicraft),
        }
    }

    /// Whether this container is fundamentally a moving picture / video format.
    pub fn is_video(&self) -> bool {
        matches!(
            self,
            Self::IsoBmffMp4
                | Self::QuickTimeMov
                | Self::Matroska
                | Self::WebM
                | Self::Mxf
                | Self::MpegTs
                | Self::Gif
                | Self::AdobeAfterEffectsAep
                | Self::AdobePremierePrproj
                | Self::ArtcraftEffectcraft
                | Self::ArtcraftFilmcraft
        )
    }

    /// Whether this container is purely an audio recording.
    pub fn is_audio(&self) -> bool {
        matches!(
            self,
            Self::Wav | Self::Rf64Wav | Self::Aiff | Self::Flac | Self::Mp3 | Self::Ogg | Self::AdobeAuditionSesx | Self::ArtcraftSoundcraft
        )
    }

    /// Whether this container is a 2D vector asset.
    pub fn is_vector(&self) -> bool {
        matches!(self, Self::Svg | Self::IllustratorAi | Self::Eps | Self::ArtcraftVectorcraft)
    }

    /// Whether this container is a digital document or page layout.
    pub fn is_document(&self) -> bool {
        matches!(self, Self::Pdf | Self::Docx | Self::Pptx | Self::Xlsx | Self::Idml | Self::Indd | Self::Epub | Self::ArtcraftDesigncraft)
    }

    /// Whether this format represents a 3D geometry mesh or scene.
    pub fn is_3d(&self) -> bool {
        matches!(
            self,
            Self::GltfBinary | Self::GltfJson | Self::UsdBinary | Self::UsdAscii | Self::Obj | Self::StlBinary | Self::StlAscii | Self::Ply
        )
    }

    /// Whether this format is a 2D CAD technical drawing.
    pub fn is_cad(&self) -> bool {
        matches!(self, Self::AutocadDxf | Self::AutocadDwg | Self::HpglPlotter | Self::ArtcraftCadcraft)
    }

    /// Whether this format is a multi-layer composite image.
    pub fn is_layered_composite(&self) -> bool {
        matches!(self, Self::Psd | Self::Psb | Self::ArtcraftPhotocraft)
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn sniff_image_and_raw(bytes: &[u8]) -> Option<ContainerKind> {
    // 1. PNG: 89 50 4E 47 0D 0A 1A 0A
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ContainerKind::Png);
    }

    // 2. JPEG: FF D8 FF
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return Some(ContainerKind::Jpeg);
    }

    // 3. GIF: GIF87a or GIF89a
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ContainerKind::Gif);
    }

    // 4. OpenEXR: 76 2F 31 01
    if bytes.starts_with(b"\x76\x2F\x31\x01") {
        return Some(ContainerKind::OpenExr);
    }

    // 5. TIFF: 'II*\x00' (little-endian) or 'MM\x00*' (big-endian)
    if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        return Some(ContainerKind::Tiff);
    }

    // Windows Bitmap: 'BM'
    if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        return Some(ContainerKind::Bmp);
    }

    // DDS: 'DDS '
    if bytes.starts_with(b"DDS ") {
        return Some(ContainerKind::Dds);
    }

    // Radiance HDR: '#?RADIANCE' or '#?RGBE'
    if bytes.starts_with(b"#?RADIANCE") || bytes.starts_with(b"#?RGBE") {
        return Some(ContainerKind::RadianceHdr);
    }

    None
}

fn sniff_riff_family(bytes: &[u8]) -> Option<ContainerKind> {
    if (bytes.starts_with(b"RIFX") || bytes.starts_with(b"RIFF")) && bytes.len() >= 12 && matches!(bytes.get(8..12), Some(b"Egg!" | b"EggP")) {
        return Some(ContainerKind::AdobeAfterEffectsAep);
    }

    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        match bytes.get(8..12) {
            Some(b"WAVE") => return Some(ContainerKind::Wav),
            Some(b"WEBP") => return Some(ContainerKind::WebP),
            _ => {}
        }
    }

    if bytes.starts_with(b"RF64") && bytes.len() >= 12 && bytes.get(8..12) == Some(b"WAVE") {
        return Some(ContainerKind::Rf64Wav);
    }

    if bytes.starts_with(b"FORM") && bytes.len() >= 12 && matches!(bytes.get(8..12), Some(b"AIFF" | b"AIFC")) {
        return Some(ContainerKind::Aiff);
    }

    None
}

fn sniff_audio_streams(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.starts_with(b"fLaC") {
        return Some(ContainerKind::Flac);
    }
    if bytes.starts_with(b"OggS") {
        return Some(ContainerKind::Ogg);
    }
    if bytes.starts_with(b"ID3") {
        return Some(ContainerKind::Mp3);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some(ContainerKind::Mp3);
    }
    None
}

fn sniff_video_containers(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.len() >= 14 && bytes.starts_with(b"\x06\x0E\x2B\x34\x02\x05\x01\x01\x0D\x01\x02\x01\x01") {
        return Some(ContainerKind::Mxf);
    }

    if bytes.starts_with(b"\x1A\x45\xDF\xA3") {
        let slice_len = bytes.len().min(128);
        if bytes.get(0..slice_len).is_some_and(|sub| sub.windows(4).any(|w| w == b"webm")) {
            return Some(ContainerKind::WebM);
        }
        return Some(ContainerKind::Matroska);
    }

    if bytes.len() >= 12 && bytes.get(4..8) == Some(b"ftyp") {
        if bytes.get(8..12) == Some(b"qt  ") {
            return Some(ContainerKind::QuickTimeMov);
        }
        return Some(ContainerKind::IsoBmffMp4);
    }

    if bytes.len() >= 8 && matches!(bytes.get(4..8), Some(b"moov" | b"wide")) {
        return Some(ContainerKind::QuickTimeMov);
    }

    if bytes.len() >= 377 && bytes.first() == Some(&0x47) && bytes.get(188) == Some(&0x47) && bytes.get(376) == Some(&0x47) {
        return Some(ContainerKind::MpegTs);
    }

    None
}

fn sniff_artcraft_binary(bytes: &[u8]) -> Option<ContainerKind> {
    if !bytes.starts_with(b"\x89CRAFT\x00\x01") {
        return None;
    }
    match bytes.get(8) {
        Some(b'D') => Some(ContainerKind::ArtcraftDesigncraft),
        Some(b'P') => Some(ContainerKind::ArtcraftPhotocraft),
        Some(b'V') => Some(ContainerKind::ArtcraftVectorcraft),
        Some(b'E') => Some(ContainerKind::ArtcraftEffectcraft),
        Some(b'F') => Some(ContainerKind::ArtcraftFilmcraft),
        Some(b'S') => Some(ContainerKind::ArtcraftSoundcraft),
        Some(b'C') => Some(ContainerKind::ArtcraftCadcraft),
        Some(b'A') => Some(ContainerKind::ArtcraftAnicraft),
        _ => Some(ContainerKind::ArtcraftDesigncraft),
    }
}

fn sniff_zip_package(sub: &[u8]) -> ContainerKind {
    if contains_subslice(sub, b"designmap.xml") || contains_subslice(sub, b"idml-package") {
        ContainerKind::Idml
    } else if contains_subslice(sub, b"application/x-designcraft") {
        ContainerKind::ArtcraftDesigncraft
    } else if contains_subslice(sub, b"application/x-photocraft") {
        ContainerKind::ArtcraftPhotocraft
    } else if contains_subslice(sub, b"application/x-vectorcraft") {
        ContainerKind::ArtcraftVectorcraft
    } else if contains_subslice(sub, b"application/x-effectcraft") {
        ContainerKind::ArtcraftEffectcraft
    } else if contains_subslice(sub, b"application/x-filmcraft") {
        ContainerKind::ArtcraftFilmcraft
    } else if contains_subslice(sub, b"application/x-soundcraft") {
        ContainerKind::ArtcraftSoundcraft
    } else if contains_subslice(sub, b"application/x-cadcraft") {
        ContainerKind::ArtcraftCadcraft
    } else if contains_subslice(sub, b"application/x-anicraft") {
        ContainerKind::ArtcraftAnicraft
    } else if contains_subslice(sub, b"application/epub") {
        ContainerKind::Epub
    } else if contains_subslice(sub, b"word/") {
        ContainerKind::Docx
    } else if contains_subslice(sub, b"ppt/") {
        ContainerKind::Pptx
    } else if contains_subslice(sub, b"xl/") {
        ContainerKind::Xlsx
    } else {
        ContainerKind::Docx
    }
}

fn sniff_adobe_cad_and_packages(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.starts_with(b"%PDF-") {
        let inspect_len = bytes.len().min(4096);
        if let Some(sub) = bytes.get(0..inspect_len)
            && (contains_subslice(sub, b"/AIPrivateData") || contains_subslice(sub, b"AdobeIllustrat"))
        {
            return Some(ContainerKind::IllustratorAi);
        }
        return Some(ContainerKind::Pdf);
    }

    if bytes.starts_with(b"%!PS-Adobe") {
        return Some(ContainerKind::Eps);
    }

    if bytes.starts_with(b"8BPS") {
        if bytes.len() >= 6 {
            let version = u16::from_be_bytes([bytes[4], bytes[5]]);
            if version == 2 {
                return Some(ContainerKind::Psb);
            }
        }
        return Some(ContainerKind::Psd);
    }

    if bytes.starts_with(b"\x06\x06\xED\xF5\xD8\x1D\x46\xE5\xBD\x31\xEF\xE7\xFE\x74\xB7\x1D") {
        return Some(ContainerKind::Indd);
    }

    if let Some(artcraft) = sniff_artcraft_binary(bytes) {
        return Some(artcraft);
    }

    if bytes.starts_with(b"PK\x03\x04") {
        let inspect_len = bytes.len().min(1024);
        if let Some(sub) = bytes.get(0..inspect_len) {
            return Some(sniff_zip_package(sub));
        }
        return Some(ContainerKind::Docx);
    }

    if bytes.starts_with(b"AC10") {
        return Some(ContainerKind::AutocadDwg);
    }

    if bytes.starts_with(b"AutoCAD DXF\r\n\x1a\x00") {
        return Some(ContainerKind::AutocadDxf);
    }

    if bytes.starts_with(b"IN;") || bytes.starts_with(b"\x1B%-1B") || bytes.starts_with(b"SP;") || bytes.starts_with(b"PA;") {
        return Some(ContainerKind::HpglPlotter);
    }

    if bytes.starts_with(b"\x1F\x8B\x08") && bytes.len() >= 10 {
        let flg = bytes[3];
        if (flg & 0x08) != 0 {
            let name_slice = &bytes[10..bytes.len().min(128)];
            if let Some(name) = name_slice.iter().position(|&b| b == 0).and_then(|pos| std::str::from_utf8(&name_slice[..pos]).ok()) {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".prproj") || lower.contains("premiere") {
                    return Some(ContainerKind::AdobePremierePrproj);
                }
            }
        }
    }

    None
}

fn sniff_3d(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.starts_with(b"glTF") {
        return Some(ContainerKind::GltfBinary);
    }
    if bytes.starts_with(b"PXR-USDC") {
        return Some(ContainerKind::UsdBinary);
    }
    if bytes.starts_with(b"#usda") {
        return Some(ContainerKind::UsdAscii);
    }
    if bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n") {
        return Some(ContainerKind::Ply);
    }
    None
}

fn sniff_text_formats(bytes: &[u8]) -> Option<ContainerKind> {
    let sample_len = bytes.len().min(512);
    if let Ok(text) = std::str::from_utf8(&bytes[0..sample_len]) {
        let trimmed = text.trim_start();
        if trimmed.starts_with("<?xml") && trimmed.contains("<svg") || trimmed.starts_with("<svg") {
            return Some(ContainerKind::Svg);
        }
        if trimmed.starts_with("<?xml") && (trimmed.contains("<PremiereData") || trimmed.contains("<Project")) || trimmed.starts_with("<PremiereData")
        {
            return Some(ContainerKind::AdobePremierePrproj);
        }
        if trimmed.starts_with("<sesx") || ((trimmed.starts_with("<?xml") || trimmed.starts_with("<!DOCTYPE sesx")) && trimmed.contains("<sesx")) {
            return Some(ContainerKind::AdobeAuditionSesx);
        }
        if (trimmed.starts_with("<?xml") || trimmed.starts_with("<session")) && trimmed.contains("<session") {
            return Some(ContainerKind::AdobeAuditionSesx);
        }
        if (trimmed.starts_with("0\nSECTION") || trimmed.starts_with("0\r\nSECTION") || trimmed.starts_with("  0\r\nSECTION"))
            && (trimmed.contains("HEADER") || trimmed.contains("ENTITIES") || trimmed.contains("BLOCKS"))
        {
            return Some(ContainerKind::AutocadDxf);
        }
        if trimmed.starts_with('{') && trimmed.contains("\"asset\"") {
            return Some(ContainerKind::GltfJson);
        }
        if trimmed.starts_with("solid ") {
            return Some(ContainerKind::StlAscii);
        }
        if trimmed.lines().any(|l| l.starts_with("v ") || l.starts_with("vn ") || l.starts_with("f ")) {
            return Some(ContainerKind::Obj);
        }
    }
    None
}

/// Sniffs the container kind by inspecting up to the first 512 bytes of a file or stream.
pub fn sniff_container(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.len() < 2 {
        return None;
    }

    if let Some(kind) = sniff_image_and_raw(bytes) {
        return Some(kind);
    }
    if let Some(kind) = sniff_riff_family(bytes) {
        return Some(kind);
    }
    if let Some(kind) = sniff_audio_streams(bytes) {
        return Some(kind);
    }
    if let Some(kind) = sniff_video_containers(bytes) {
        return Some(kind);
    }
    if let Some(kind) = sniff_adobe_cad_and_packages(bytes) {
        return Some(kind);
    }
    if let Some(kind) = sniff_3d(bytes) {
        return Some(kind);
    }
    sniff_text_formats(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sniff_png() {
        let header = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(sniff_container(header), Some(ContainerKind::Png));
    }

    #[test]
    fn test_sniff_jpeg() {
        let header = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
        assert_eq!(sniff_container(header), Some(ContainerKind::Jpeg));
    }

    #[test]
    fn test_sniff_mp4() {
        let mut header = vec![0x00, 0x00, 0x00, 0x20];
        header.extend_from_slice(b"ftypisom\x00\x00\x02\x00");
        assert_eq!(sniff_container(&header), Some(ContainerKind::IsoBmffMp4));
    }

    #[test]
    fn test_sniff_wav() {
        let header = b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec();
        assert_eq!(sniff_container(&header), Some(ContainerKind::Wav));
    }

    #[test]
    fn test_sniff_openexr() {
        let header = b"\x76\x2F\x31\x01\x02\x00\x00\x00";
        assert_eq!(sniff_container(header), Some(ContainerKind::OpenExr));
    }

    #[test]
    fn test_sniff_glb() {
        let header = b"glTF\x02\x00\x00\x00\x40\x00\x00\x00";
        assert_eq!(sniff_container(header), Some(ContainerKind::GltfBinary));
    }

    #[test]
    fn test_sniff_unknown_or_short() {
        assert_eq!(sniff_container(b""), None);
        assert_eq!(sniff_container(b"xyz"), None);
        assert_eq!(sniff_container(b"some random junk data that has no header"), None);
    }

    #[test]
    fn test_sniff_mpeg_ts_needs_three_sync_bytes() {
        let mut ts = vec![0u8; 188 * 3];
        for i in 0..3 {
            ts[i * 188] = 0x47;
        }
        assert_eq!(sniff_container(&ts), Some(ContainerKind::MpegTs));
        ts[376] = 0;
        assert_eq!(sniff_container(&ts), None);
    }

    #[test]
    fn test_sniff_audio_formats() {
        assert_eq!(sniff_container(b"fLaC\x00\x00\x00\x22"), Some(ContainerKind::Flac));
        assert_eq!(sniff_container(b"OggS\x00\x02\x00\x00"), Some(ContainerKind::Ogg));
        assert_eq!(sniff_container(b"ID3\x04\x00\x00\x00\x00"), Some(ContainerKind::Mp3));
        assert_eq!(sniff_container(b"\xFF\xFB\x90\x64\x00\x00"), Some(ContainerKind::Mp3));
    }

    #[test]
    fn test_sniff_documents_and_vector() {
        assert_eq!(sniff_container(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n"), Some(ContainerKind::Pdf));
        assert_eq!(sniff_container(b"%PDF-1.5\n/AIPrivateData 123 0 obj\n"), Some(ContainerKind::IllustratorAi));
        assert_eq!(sniff_container(b"%!PS-Adobe-3.0 EPSF-3.0\n"), Some(ContainerKind::Eps));
        assert_eq!(sniff_container(b"8BPS\x00\x01\x00\x00"), Some(ContainerKind::Psd));
        assert_eq!(sniff_container(b"8BPS\x00\x02\x00\x00"), Some(ContainerKind::Psb));
        assert_eq!(sniff_container(b"BM\x36\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00"), Some(ContainerKind::Bmp));
        assert_eq!(sniff_container(b"\x06\x06\xED\xF5\xD8\x1D\x46\xE5\xBD\x31\xEF\xE7\xFE\x74\xB7\x1D\x00\x00"), Some(ContainerKind::Indd));
        assert_eq!(sniff_container(b"PK\x03\x04\x14\x00\x00\x00\x08\x00word/document.xml"), Some(ContainerKind::Docx));
        assert_eq!(sniff_container(b"PK\x03\x04\x14\x00\x00\x00\x08\x00designmap.xml"), Some(ContainerKind::Idml));
        assert_eq!(sniff_container(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"), Some(ContainerKind::Svg));
    }

    #[test]
    fn test_sniff_adobe_and_cad_and_artcraft() {
        // After Effects AEP
        let mut aep = b"RIFX\x00\x00\x10\x00Egg!LIST".to_vec();
        assert_eq!(sniff_container(&aep), Some(ContainerKind::AdobeAfterEffectsAep));
        aep[8..12].copy_from_slice(b"EggP");
        assert_eq!(sniff_container(&aep), Some(ContainerKind::AdobeAfterEffectsAep));

        // Premiere Pro PRPROJ (XML)
        let prproj_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PremiereData Version=\"4\"></PremiereData>";
        assert_eq!(sniff_container(prproj_xml), Some(ContainerKind::AdobePremierePrproj));

        // Premiere Pro PRPROJ (Gzip with FNAME)
        let mut prproj_gz = vec![0x1F, 0x8B, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];
        prproj_gz.extend_from_slice(b"sequence.prproj\x00contentdata");
        assert_eq!(sniff_container(&prproj_gz), Some(ContainerKind::AdobePremierePrproj));

        // Audition SESX
        let sesx_root = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<sesx version=\"1.9\"><session/></sesx>";
        assert_eq!(sniff_container(sesx_root), Some(ContainerKind::AdobeAuditionSesx));
        assert_eq!(sniff_container(b"<sesx audioRate=\"48000\"></sesx>"), Some(ContainerKind::AdobeAuditionSesx));
        let sesx_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<session Version=\"1.0\"></session>";
        assert_eq!(sniff_container(sesx_xml), Some(ContainerKind::AdobeAuditionSesx));

        // AutoCAD DWG
        assert_eq!(sniff_container(b"AC1032\x00\x00\x00"), Some(ContainerKind::AutocadDwg));

        // AutoCAD DXF
        assert_eq!(sniff_container(b"AutoCAD DXF\r\n\x1a\x00binarydata"), Some(ContainerKind::AutocadDxf));
        assert_eq!(sniff_container(b"0\nSECTION\n2\nHEADER\n0\nENDSEC"), Some(ContainerKind::AutocadDxf));

        // HPGL Plotter
        assert_eq!(sniff_container(b"IN;SP1;PA100,100;"), Some(ContainerKind::HpglPlotter));

        // ArtCraft Native Binary
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01D\x00\x00"), Some(ContainerKind::ArtcraftDesigncraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01P\x00\x00"), Some(ContainerKind::ArtcraftPhotocraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01V\x00\x00"), Some(ContainerKind::ArtcraftVectorcraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01E\x00\x00"), Some(ContainerKind::ArtcraftEffectcraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01F\x00\x00"), Some(ContainerKind::ArtcraftFilmcraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01S\x00\x00"), Some(ContainerKind::ArtcraftSoundcraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01C\x00\x00"), Some(ContainerKind::ArtcraftCadcraft));
        assert_eq!(sniff_container(b"\x89CRAFT\x00\x01A\x00\x00"), Some(ContainerKind::ArtcraftAnicraft));

        // ArtCraft Native Zip package
        let mut craft_zip = b"PK\x03\x04\x14\x00\x00\x00\x08\x00mimetypeapplication/x-cadcraft".to_vec();
        assert_eq!(sniff_container(&craft_zip), Some(ContainerKind::ArtcraftCadcraft));
        craft_zip = b"PK\x03\x04\x14\x00\x00\x00\x08\x00mimetypeapplication/x-designcraft".to_vec();
        assert_eq!(sniff_container(&craft_zip), Some(ContainerKind::ArtcraftDesigncraft));
    }
}
