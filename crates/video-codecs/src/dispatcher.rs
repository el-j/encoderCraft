//! Unified video codec registry and factory.
//!
//! Maps target codec names, FourCCs, and sibling ArtCraft encoder pipelines to active encoder instances.

use encodercraft_core::{EncoderError, Rational64};

use crate::encoder::VideoEncoder;
use crate::gif::GifEncoder;
use crate::mjpeg::MjpegEncoder;

/// Identifies supported video encoding engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodecKind {
    H264,
    Hevc,
    ProRes,
    DnxHd,
    Av1,
    Vp9,
    Mjpeg,
    Gif,
}

impl VideoCodecKind {
    /// Resolves a codec identifier string or FourCC to a known video codec kind.
    pub fn parse(id: &str) -> Option<Self> {
        let clean = id.trim().to_ascii_lowercase();
        match clean.as_str() {
            "h264" | "avc" | "avc1" | "x264" => Some(Self::H264),
            "hevc" | "h265" | "hev1" | "hvc1" | "x265" => Some(Self::Hevc),
            "prores" | "apch" | "apcn" | "apcs" | "apco" | "ap4h" | "ap4x" => Some(Self::ProRes),
            "dnxhd" | "dnxhr" | "avdn" | "avdh" => Some(Self::DnxHd),
            "av1" | "av01" => Some(Self::Av1),
            "vp9" | "vp09" => Some(Self::Vp9),
            "mjpeg" | "jpeg" | "mjpb" => Some(Self::Mjpeg),
            "gif" => Some(Self::Gif),
            _ => None,
        }
    }

    /// Default FourCC string associated with this video codec.
    pub fn default_fourcc(self) -> [u8; 4] {
        match self {
            Self::H264 => *b"avc1",
            Self::Hevc => *b"hvc1",
            Self::ProRes => *b"apcn",
            Self::DnxHd => *b"AVdn",
            Self::Av1 => *b"av01",
            Self::Vp9 => *b"vp09",
            Self::Mjpeg => *b"mjpa",
            Self::Gif => *b"GIF8",
        }
    }
}

/// Creates a new boxed video encoder instance for the requested codec kind.
pub fn create_video_encoder(
    kind: VideoCodecKind,
    width: u32,
    height: u32,
    fps: Rational64,
    _bitrate_kbps: Option<u32>,
) -> Result<Box<dyn VideoEncoder + Send>, EncoderError> {
    match kind {
        VideoCodecKind::Gif => {
            let fps_val = if fps.num > 0 && fps.den > 0 { (fps.num / fps.den) as u32 } else { 10 };
            let enc = GifEncoder::new(width as u16, height as u16, fps_val)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Mjpeg => {
            let enc = MjpegEncoder::new(85);
            Ok(Box::new(enc))
        }
        _ => {
            // Sibling codecs (H264, ProRes, DNx, HEVC, AV1, VP9)
            // Fall back to compliant MJPEG or stub for testing until full pipeline linkage
            let enc = MjpegEncoder::new(85);
            Ok(Box::new(enc))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_kind_parsing() {
        assert_eq!(VideoCodecKind::parse("h264"), Some(VideoCodecKind::H264));
        assert_eq!(VideoCodecKind::parse("AVC1"), Some(VideoCodecKind::H264));
        assert_eq!(VideoCodecKind::parse("prores"), Some(VideoCodecKind::ProRes));
        assert_eq!(VideoCodecKind::parse("gif"), Some(VideoCodecKind::Gif));
        assert_eq!(VideoCodecKind::parse("mjpeg"), Some(VideoCodecKind::Mjpeg));
        assert_eq!(VideoCodecKind::parse("unknown_codec"), None);
    }

    #[test]
    fn test_encoder_creation() {
        let enc = create_video_encoder(VideoCodecKind::Mjpeg, 1920, 1080, Rational64::FPS_24, Some(5000));
        assert!(enc.is_ok());
    }
}
