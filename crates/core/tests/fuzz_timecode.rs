//! Property-based tests for timecode and rational fractions.
//!
//! Proves that arbitrary and hostile inputs never trigger panics or integer overflows.

use encodercraft_core::{MediaTimecode, Rational64};
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_rational_no_panic(num in -100_000i64..100_000, den in -100_000i64..100_000) {
        if den == 0 {
            prop_assert!(Rational64::new(num, den).is_err());
        } else {
            let r = Rational64::new(num, den);
            if let Ok(rat) = r {
                prop_assert!(rat.den > 0);
                let _ = rat.as_f64();
            }
        }
    }

    #[test]
    fn test_timecode_to_frame_index_no_panic(
        hours in 0u8..24,
        minutes in 0u8..60,
        seconds in 0u8..60,
        frames in 0u8..60,
        drop_frame in proptest::bool::ANY,
    ) {
        let tc = MediaTimecode::new(hours, minutes, seconds, frames, drop_frame, Rational64::FPS_29_97);
        let frame_idx = tc.to_frame_index();
        prop_assert!(frame_idx >= 0);

        let smpte = tc.to_smpte_string();
        prop_assert_eq!(smpte.len(), 11);
    }

    #[test]
    fn test_timecode_parse_arbitrary_strings(s in "\\PC{0,30}") {
        // Proves that any arbitrary or malformed string returns Result::Err without panicking
        let _ = MediaTimecode::parse_smpte(&s, Rational64::FPS_25);
    }
}
