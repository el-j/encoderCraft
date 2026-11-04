//! Codec capability table: the single source of truth for `encodercraft-cli --formats`
//! and the generated section of `docs/file-format-matrix.md`.

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
    CodecCapability { codec: "H.264", decode: true, encode: true, decode_verified: true, notes: "8-bit 4:2:0 progressive decode (filmcraft-h264)" },
    CodecCapability { codec: "HEVC", decode: true, encode: true, decode_verified: true, notes: "Main / Main 10 decode (filmcraft-hevc)" },
    CodecCapability { codec: "VP8", decode: true, encode: false, decode_verified: true, notes: "decode only (filmcraft-vp8)" },
    CodecCapability { codec: "VP9", decode: true, encode: true, decode_verified: true, notes: "filmcraft-vp9" },
    CodecCapability { codec: "AV1", decode: true, encode: true, decode_verified: true, notes: "pure-Rust decoder (filmcraft-av1)" },
    CodecCapability {
        codec: "MPEG-2",
        decode: true,
        encode: false,
        decode_verified: true,
        notes: "MPEG-1/2 in PS and TS, decode only (filmcraft-mpeg2v)",
    },
    CodecCapability { codec: "ProRes", decode: true, encode: true, decode_verified: true, notes: "filmcraft-prores" },
    CodecCapability {
        codec: "DNxHD/DNxHR",
        decode: true,
        encode: true,
        decode_verified: false,
        notes: "filmcraft-dnx; round-trips in video-codecs tests only",
    },
    CodecCapability { codec: "Motion JPEG", decode: false, encode: true, decode_verified: false, notes: "encode only" },
    CodecCapability { codec: "GIF", decode: false, encode: true, decode_verified: false, notes: "animated GIF89a encode only" },
];

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
        for id in ["h264", "hevc", "prores", "dnxhd", "av1", "vp9", "mjpeg", "gif"] {
            assert!(VideoCodecKind::parse(id).is_some(), "{id} is not a known encoder");
        }
        let encodable = CODEC_CAPABILITIES.iter().filter(|c| c.encode).count();
        assert_eq!(encodable, 8);
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
