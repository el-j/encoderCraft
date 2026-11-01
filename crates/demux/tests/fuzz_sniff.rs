//! Property-based fuzz tests for container sniffing and header probing.
//!
//! Proves that arbitrary and hostile byte inputs never trigger panics or out-of-bounds indexing.

use encodercraft_demux::{parse_aiff_header, parse_wav_header, probe_bytes, probe_jpeg, probe_png, probe_webp, sniff_container};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_sniff_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // Must never panic on random input
        let _ = sniff_container(&bytes);
    }

    #[test]
    fn test_probe_bytes_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..8192)) {
        // Must return Ok or Err, never panic
        let _ = probe_bytes(&bytes);
    }

    #[test]
    fn test_parse_wav_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = parse_wav_header(&bytes);
    }

    #[test]
    fn test_parse_aiff_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = parse_aiff_header(&bytes);
    }

    #[test]
    fn test_probe_png_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = probe_png(&bytes);
    }

    #[test]
    fn test_probe_jpeg_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = probe_jpeg(&bytes);
    }

    #[test]
    fn test_probe_webp_arbitrary(bytes in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let _ = probe_webp(&bytes);
    }
}
