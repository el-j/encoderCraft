//! Unified video codec registry and factory.
//!
//! Maps target codec names, FourCCs, and sibling ArtCraft encoder pipelines to active encoder instances.

use encodercraft_core::{EncoderError, Rational64};

use crate::av1::Av1EncoderWrapper;
use crate::dnx::DnxEncoderWrapper;
use crate::encoder::VideoEncoder;
use crate::gif::GifEncoder;
use crate::h264::H264EncoderWrapper;
use crate::hevc::HevcEncoderWrapper;
use crate::mjpeg::MjpegEncoder;
use crate::prores::ProResEncoderWrapper;
use crate::vp9::Vp9EncoderWrapper;

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
    bitrate_kbps: Option<u32>,
) -> Result<Box<dyn VideoEncoder + Send>, EncoderError> {
    match kind {
        VideoCodecKind::H264 => {
            let enc = H264EncoderWrapper::new(width, height, fps, bitrate_kbps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Hevc => {
            let enc = HevcEncoderWrapper::new(width, height, fps, bitrate_kbps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::ProRes => {
            let enc = ProResEncoderWrapper::new(width, height, fps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Av1 => {
            let enc = Av1EncoderWrapper::new(width, height, fps, bitrate_kbps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Vp9 => {
            let enc = Vp9EncoderWrapper::new(width, height, fps, bitrate_kbps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Gif => {
            let fps_val = if fps.num > 0 && fps.den > 0 { (fps.num / fps.den) as u32 } else { 10 };
            let enc = GifEncoder::new(width as u16, height as u16, fps_val)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::DnxHd => {
            let enc = DnxEncoderWrapper::new(width, height, fps)?;
            Ok(Box::new(enc))
        }
        VideoCodecKind::Mjpeg => {
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
        let enc_mjpeg = create_video_encoder(VideoCodecKind::Mjpeg, 1920, 1080, Rational64::FPS_24, Some(5000));
        assert!(enc_mjpeg.is_ok());

        let enc_h264 = create_video_encoder(VideoCodecKind::H264, 1280, 720, Rational64::FPS_30, Some(4000));
        assert!(enc_h264.is_ok());

        let enc_prores = create_video_encoder(VideoCodecKind::ProRes, 1920, 1080, Rational64::FPS_24, None);
        assert!(enc_prores.is_ok());

        let enc_hevc = create_video_encoder(VideoCodecKind::Hevc, 1280, 720, Rational64::FPS_30, Some(2000));
        assert!(enc_hevc.is_ok());

        let enc_av1 = create_video_encoder(VideoCodecKind::Av1, 1280, 720, Rational64::FPS_30, Some(1500));
        assert!(enc_av1.is_ok());

        let enc_vp9 = create_video_encoder(VideoCodecKind::Vp9, 1280, 720, Rational64::FPS_30, Some(1500));
        assert!(enc_vp9.is_ok());

        let enc_dnx = create_video_encoder(VideoCodecKind::DnxHd, 1920, 1080, Rational64::FPS_24, None);
        assert!(enc_dnx.is_ok());
    }
}
