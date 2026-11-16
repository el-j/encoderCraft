//! Inputs found by the nightly libFuzzer run that once crashed the WAV decoder.

use encodercraft_demux::{decode_wav_samples, parse_wav_header};

#[test]
fn wav_with_sub_byte_sample_size_is_an_error_not_a_panic() {
    let data = include_bytes!("regressions/wav-zero-sample-size.bin");
    if let Ok(header) = parse_wav_header(data) {
        assert!(decode_wav_samples(&header, data, 1_000_000).is_err());
    }
}
