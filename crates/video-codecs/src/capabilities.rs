//! Codec capability table: the single source of truth for `encodercraft-cli --formats`
//! and the generated section of `docs/file-format-matrix.md`, plus the container x codec matrix.

use encodercraft_core::ContainerType;

use crate::VideoCodecKind;

/// What encoderCraft can do with one video codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecCapability {
    pub codec: &'static str,
    pub decode: bool,
    pub encode: bool,
    /// Decoding is exercised by a conformance test against a stream from an independent encoder.
    pub decode_verified: bool,
    pub notes: &'static str,
}

/// Every video codec known to the engine, in display order.
pub const CODEC_CAPABILITIES: &[CodecCapability] = &[
    CodecCapability { codec: "H.264", decode: true, encode: true, decode_verified: true, notes: "8-bit 4:2:0 progressive (craft-video)" },
    CodecCapability { codec: "HEVC", decode: true, encode: true, decode_verified: true, notes: "Main / Main 10 (craft-video)" },
    CodecCapability { codec: "VP8", decode: true, encode: false, decode_verified: true, notes: "decode (craft-video)" },
    CodecCapability { codec: "VP9", decode: true, encode: true, decode_verified: true, notes: "craft-video" },
    CodecCapability { codec: "AV1", decode: true, encode: true, decode_verified: true, notes: "craft-video" },
    CodecCapability { codec: "APV", decode: true, encode: true, decode_verified: true, notes: "RFC 9924 10/12-bit 422/444 (craft-video)" },
    CodecCapability { codec: "MPEG-2", decode: true, encode: true, decode_verified: true, notes: "H.262 Main/422 Profile (craft-video)" },
    CodecCapability { codec: "MPEG-4 Part 2", decode: false, encode: true, decode_verified: false, notes: "Simple Profile mp4v/XVID (craft-video)" },
    CodecCapability { codec: "FFV1", decode: true, encode: true, decode_verified: true, notes: "RFC 9043 v3 lossless (craft-video)" },
    CodecCapability { codec: "ProRes", decode: true, encode: true, decode_verified: true, notes: "craft-video" },
    CodecCapability {
        codec: "DNxHD/DNxHR",
        decode: true,
        encode: true,
        decode_verified: false,
        notes: "craft-video; round-trips in video-codecs tests",
    },
    CodecCapability { codec: "Motion JPEG", decode: false, encode: true, decode_verified: false, notes: "encode only" },
    CodecCapability { codec: "GIF", decode: false, encode: true, decode_verified: false, notes: "animated GIF89a encode only" },
];

/// Video codecs the muxer of `container` can store; empty for non-video containers.
pub fn codecs_for_container(container: ContainerType) -> &'static [VideoCodecKind] {
    use VideoCodecKind::{Apv, Av1, DnxHd, Ffv1, Gif, H264, Hevc, Mpeg2, Mpeg4Sp, ProRes, Vp9};
    match container {
        ContainerType::IsoBmffMp4 => &[H264, Hevc, Av1, Vp9, Apv],
        ContainerType::QuickTimeMov => &[H264, Hevc, ProRes, DnxHd, Apv],
        ContainerType::MatroskaMkv => &[H264, Hevc, Av1, Vp9, Ffv1, Mpeg2, Apv],
        ContainerType::WebM => &[Vp9, Av1],
        ContainerType::MxfOp1a | ContainerType::MxfOpAtom => &[DnxHd, ProRes, H264, Mpeg2],
        ContainerType::MpegTs => &[H264, Hevc, Mpeg2],
        ContainerType::Avi => &[Mpeg4Sp, H264, Ffv1, Mpeg2],
        ContainerType::AnimatedGif => &[Gif],
        _ => &[],
    }
}

/// Whether `container` can store `codec`.
pub fn container_accepts(container: ContainerType, codec: VideoCodecKind) -> bool {
    codecs_for_container(container).contains(&codec)
}

/// Renders the container x codec matrix as a Markdown table for the docs.
pub fn render_container_matrix_markdown() -> String {
    let containers = [
        ("MP4", ContainerType::IsoBmffMp4),
        ("MOV", ContainerType::QuickTimeMov),
        ("MKV", ContainerType::MatroskaMkv),
        ("WebM", ContainerType::WebM),
        ("MXF OP1a", ContainerType::MxfOp1a),
        ("MXF OP-Atom", ContainerType::MxfOpAtom),
        ("MPEG-TS", ContainerType::MpegTs),
        ("AVI", ContainerType::Avi),
        ("GIF", ContainerType::AnimatedGif),
    ];
    let codecs = [
        ("H.264", VideoCodecKind::H264),
        ("HEVC", VideoCodecKind::Hevc),
        ("AV1", VideoCodecKind::Av1),
        ("VP9", VideoCodecKind::Vp9),
        ("ProRes", VideoCodecKind::ProRes),
        ("DNxHR", VideoCodecKind::DnxHd),
        ("GIF", VideoCodecKind::Gif),
    ];
    let mut out = String::from("| Container |");
    for (name, _) in codecs {
        out.push_str(&format!(" {name} |"));
    }
    out.push_str("\n|---|");
    out.push_str(&"---|".repeat(codecs.len()));
    out.push('\n');
    for (cname, container) in containers {
        out.push_str(&format!("| {cname} |"));
        for (_, codec) in codecs {
            out.push_str(if container_accepts(container, codec) { " yes |" } else { " - |" });
        }
        out.push('\n');
    }
    out
}

/// Renders the table as aligned plain text for the CLI.
pub fn render_text() -> String {
    let mut out = String::from("CODEC         DECODE  ENCODE  VERIFIED  NOTES\n");
    for c in CODEC_CAPABILITIES {
        out.push_str(&format!(
            "{:<13} {:<7} {:<7} {:<9} {}\n",
            c.codec,
            yes_no(c.decode),
            yes_no(c.encode),
            if c.decode { yes_no(c.decode_verified) } else { "-" },
            c.notes
        ));
    }
    out
}

/// Renders the table as a Markdown table for the docs.
pub fn render_markdown() -> String {
    let mut out = String::from("| Codec | Decode | Encode | Decode verified | Notes |\n|---|---|---|---|---|\n");
    for c in CODEC_CAPABILITIES {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            c.codec,
            yes_no(c.decode),
            yes_no(c.encode),
            if c.decode { yes_no(c.decode_verified) } else { "-" },
            c.notes
        ));
    }
    out
}

fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoCodecKind;

    #[test]
    fn every_encoder_kind_is_listed_as_encodable() {
        for id in ["h264", "hevc", "prores", "dnxhd", "av1", "vp9", "apv", "mpeg2", "mpeg4", "ffv1", "mjpeg", "gif"] {
            assert!(VideoCodecKind::parse(id).is_some(), "{id} is not a known encoder");
        }
        let encodable = CODEC_CAPABILITIES.iter().filter(|c| c.encode).count();
        assert_eq!(encodable, 12);
    }

    #[test]
    fn matrix_matches_executors() {
        assert!(container_accepts(ContainerType::QuickTimeMov, VideoCodecKind::ProRes));
        assert!(!container_accepts(ContainerType::IsoBmffMp4, VideoCodecKind::ProRes));
        assert!(!container_accepts(ContainerType::WebM, VideoCodecKind::H264));
        assert!(container_accepts(ContainerType::Avi, VideoCodecKind::Mpeg4Sp));
        assert!(codecs_for_container(ContainerType::Wave).is_empty());
        assert_eq!(render_container_matrix_markdown().lines().count(), 11);
    }

    #[test]
    fn verified_implies_decodable() {
        assert!(CODEC_CAPABILITIES.iter().all(|c| !c.decode_verified || c.decode));
    }

    #[test]
    fn markdown_has_a_row_per_codec() {
        assert_eq!(render_markdown().lines().count(), CODEC_CAPABILITIES.len() + 2);
        assert!(render_text().contains("MPEG-2"));
    }
}
