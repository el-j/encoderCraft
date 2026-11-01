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
        }
    }

    /// Whether this container is fundamentally a moving picture / video format.
    pub fn is_video(&self) -> bool {
        matches!(self, Self::IsoBmffMp4 | Self::QuickTimeMov | Self::Matroska | Self::WebM | Self::Mxf | Self::Gif)
    }

    /// Whether this container is purely an audio recording.
    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Wav | Self::Rf64Wav | Self::Aiff | Self::Flac | Self::Mp3 | Self::Ogg)
    }

    /// Whether this format represents a 3D geometry mesh or scene.
    pub fn is_3d(&self) -> bool {
        matches!(
            self,
            Self::GltfBinary | Self::GltfJson | Self::UsdBinary | Self::UsdAscii | Self::Obj | Self::StlBinary | Self::StlAscii | Self::Ply
        )
    }
}

/// Sniffs the container kind by inspecting up to the first 512 bytes of a file or stream.
pub fn sniff_container(bytes: &[u8]) -> Option<ContainerKind> {
    if bytes.len() < 4 {
        return None;
    }

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

    // 6. FLAC: 'fLaC'
    if bytes.starts_with(b"fLaC") {
        return Some(ContainerKind::Flac);
    }

    // 7. Ogg: 'OggS'
    if bytes.starts_with(b"OggS") {
        return Some(ContainerKind::Ogg);
    }

    // 8. PDF: '%PDF-'
    if bytes.starts_with(b"%PDF-") {
        return Some(ContainerKind::Pdf);
    }

    // 9. DDS: 'DDS '
    if bytes.starts_with(b"DDS ") {
        return Some(ContainerKind::Dds);
    }

    // 10. glTF Binary (.glb): 'glTF' followed by version 2
    if bytes.starts_with(b"glTF") {
        return Some(ContainerKind::GltfBinary);
    }

    // 11. USD Crate Binary: 'PXR-USDC'
    if bytes.starts_with(b"PXR-USDC") {
        return Some(ContainerKind::UsdBinary);
    }

    // 12. USD Ascii: '#usda '
    if bytes.starts_with(b"#usda") {
        return Some(ContainerKind::UsdAscii);
    }

    // 13. PLY: 'ply\n' or 'ply\r\n'
    if bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n") {
        return Some(ContainerKind::Ply);
    }

    // 14. Radiance HDR: '#?RADIANCE' or '#?RGBE'
    if bytes.starts_with(b"#?RADIANCE") || bytes.starts_with(b"#?RGBE") {
        return Some(ContainerKind::RadianceHdr);
    }

    // 15. MXF: SMPTE 377M Partition pack key (06 0E 2B 34 02 05 01 01 0D 01 02 01 01)
    if bytes.len() >= 14 && bytes.starts_with(b"\x06\x0E\x2B\x34\x02\x05\x01\x01\x0D\x01\x02\x01\x01") {
        return Some(ContainerKind::Mxf);
    }

    // 16. Matroska / WebM: EBML header 1A 45 DF A3
    if bytes.starts_with(b"\x1A\x45\xDF\xA3") {
        // Inspect the DocType string if present within the first 128 bytes
        let slice_len = bytes.len().min(128);
        if bytes.get(0..slice_len).is_some_and(|sub| sub.windows(4).any(|w| w == b"webm")) {
            return Some(ContainerKind::WebM);
        }
        return Some(ContainerKind::Matroska);
    }

    // 17. RIFF family: WAV, WebP, AVI
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        match bytes.get(8..12) {
            Some(b"WAVE") => return Some(ContainerKind::Wav),
            Some(b"WEBP") => return Some(ContainerKind::WebP),
            _ => {}
        }
    }

    // 18. RF64 (Broadcast Wave 64-bit >4GB): 'RF64....WAVE'
    if bytes.starts_with(b"RF64") && bytes.len() >= 12 && bytes.get(8..12) == Some(b"WAVE") {
        return Some(ContainerKind::Rf64Wav);
    }

    // 19. AIFF: 'FORM....AIFF' or 'FORM....AIFC'
    if bytes.starts_with(b"FORM") && bytes.len() >= 12 && matches!(bytes.get(8..12), Some(b"AIFF" | b"AIFC")) {
        return Some(ContainerKind::Aiff);
    }

    // 20. ISO Base Media File Format (MP4 / MOV): box structure with 'ftyp'
    // Usually starts with [size: 4 bytes] 'ftyp' [major_brand: 4 bytes]
    if bytes.len() >= 12 && bytes.get(4..8) == Some(b"ftyp") {
        if bytes.get(8..12) == Some(b"qt  ") {
            return Some(ContainerKind::QuickTimeMov);
        }
        return Some(ContainerKind::IsoBmffMp4);
    }

    // Also check for QuickTime files starting with 'moov', 'wide', or 'mdat' directly
    if bytes.len() >= 8 && matches!(bytes.get(4..8), Some(b"moov" | b"wide")) {
        return Some(ContainerKind::QuickTimeMov);
    }

    // 21. MP3 ID3v2 tag: 'ID3' or MPEG sync word (11 bits set: 0xFF 0xFB/0xF3/0xF2)
    if bytes.starts_with(b"ID3") {
        return Some(ContainerKind::Mp3);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some(ContainerKind::Mp3);
    }

    // 22. Text formats (SVG, glTF JSON, STL ASCII, Wavefront OBJ)
    let sample_len = bytes.len().min(512);
    if let Ok(text) = std::str::from_utf8(&bytes[0..sample_len]) {
        let trimmed = text.trim_start();
        if trimmed.starts_with("<?xml") && trimmed.contains("<svg") || trimmed.starts_with("<svg") {
            return Some(ContainerKind::Svg);
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
}
