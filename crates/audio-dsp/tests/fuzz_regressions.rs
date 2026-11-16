//! Inputs found by the nightly libFuzzer run that once crashed audio decoding.

use encodercraft_audio_dsp::decode_audio;

#[test]
fn riff_chunk_size_overflow_is_an_error_not_a_panic() {
    let data = include_bytes!("regressions/riff-chunk-overflow.bin");
    let _ = decode_audio(data);
}
