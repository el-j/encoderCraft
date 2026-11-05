//! Audio decoding and encoding wrappers backed by `craft-audio-io`.
//!
//! Provides decoding for compressed formats (MP3, FLAC, Ogg Vorbis, AAC, CAF, WAV, AIFF)
//! and encoding for FLAC, AIFF, and WAV.

use craft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError};

/// Decodes any supported audio container and format into a planar `AudioSampleBuffer`.
pub fn decode_audio(bytes: &[u8]) -> Result<AudioSampleBuffer, EncoderError> {
    // The third-party demuxers can panic on hostile headers; that must be an error, never a crash.
    let decoded = std::panic::catch_unwind(|| craft_audio_io::decode(bytes, None))
        .map_err(|_| EncoderError::InvalidContainer("audio decoding failed: the file is malformed".into()))?;
    let (_info, buf) = decoded.map_err(|e| EncoderError::Internal(format!("audio decoding failed: {e}")))?;

    let layout = match buf.channels.len() {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        6 => ChannelLayout::Surround51,
        8 => ChannelLayout::Surround71,
        _ => ChannelLayout::Stereo,
    };

    AudioSampleBuffer::from_planar_channels(buf.sample_rate, layout, 0, buf.channels)
}

/// Encodes an `AudioSampleBuffer` to lossless FLAC format.
pub fn encode_audio_flac(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions { format: FileFormat::Flac, bit_depth: BitDepth::Int24, dither: true, bwf: None };

    craft_audio_io::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("FLAC encoding failed: {e}")))
}

/// Encodes an `AudioSampleBuffer` to Apple AIFF format.
pub fn encode_audio_aiff(buffer: &AudioSampleBuffer) -> Result<Vec<u8>, EncoderError> {
    let audio_buf = AudioBuffer { sample_rate: buffer.sample_rate, channels: buffer.clone().into_planar_channels() };
    let opts = EncodeOptions { format: FileFormat::Aiff, bit_depth: BitDepth::Int24, dither: true, bwf: None };

    craft_audio_io::encode(&audio_buf, &opts).map_err(|e| EncoderError::Internal(format!("AIFF encoding failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::ChannelLayout;

    #[test]
    fn test_flac_and_aiff_encode_decode() {
        let buf = AudioSampleBuffer::new_silent(48000, ChannelLayout::Stereo, 1024, 0).unwrap();
        let flac_bytes = encode_audio_flac(&buf).unwrap();
        assert!(!flac_bytes.is_empty());

        let decoded = decode_audio(&flac_bytes).unwrap();
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.channel_count(), 2);

        let aiff_bytes = encode_audio_aiff(&buf).unwrap();
        assert!(!aiff_bytes.is_empty());
    }
}
