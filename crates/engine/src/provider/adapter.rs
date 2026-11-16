//! VideoEncoder trait adapter wrapping out-of-process codec worker processes.

use encodercraft_color::ColorSpaceTag;
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, EncoderError, Rational64, VideoFrame};
use encodercraft_geom::Dimensions;
use encodercraft_video_codecs::{EncodedPacket, VideoEncoder};

use super::worker::CodecWorkerProcess;

/// VideoEncoder trait implementation backed by a supervised out-of-process provider child.
pub struct ProviderVideoEncoder {
    worker: CodecWorkerProcess,
}

impl ProviderVideoEncoder {
    pub fn new(worker: CodecWorkerProcess) -> Self {
        Self { worker }
    }

    pub fn worker(&self) -> &CodecWorkerProcess {
        &self.worker
    }

    pub fn worker_mut(&mut self) -> &mut CodecWorkerProcess {
        &mut self.worker
    }
}

impl VideoEncoder for ProviderVideoEncoder {
    fn encode_frame(&mut self, frame: &VideoFrame) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.worker.encode_video_frame(frame.pts, frame.pts, frame.duration as u32, frame.data())
    }

    fn flush(&mut self) -> Result<Vec<EncodedPacket>, EncoderError> {
        self.worker.flush_video()
    }

    fn config_record(&self) -> Vec<u8> {
        self.worker.config_record().to_vec()
    }

    fn fourcc(&self) -> Option<[u8; 4]> {
        self.worker.fourcc()
    }
}

/// Audio encoder backed by a supervised out-of-process provider child.
pub struct ProviderAudioEncoder {
    worker: CodecWorkerProcess,
}

impl ProviderAudioEncoder {
    pub fn new(worker: CodecWorkerProcess) -> Self {
        Self { worker }
    }

    pub fn encode_audio_buffer(&mut self, buffer: &AudioSampleBuffer) -> Result<Vec<(i64, Vec<u8>)>, EncoderError> {
        let pcm_bytes = buffer.to_interleaved_f32le()?;
        self.worker.encode_audio_chunk(0, buffer.samples_per_channel() as u32, &pcm_bytes)
    }

    pub fn worker(&self) -> &CodecWorkerProcess {
        &self.worker
    }

    pub fn worker_mut(&mut self) -> &mut CodecWorkerProcess {
        &mut self.worker
    }
}

/// Video decoder backed by a supervised out-of-process provider child.
pub struct ProviderVideoDecoder {
    worker: CodecWorkerProcess,
    timebase: Rational64,
}

impl ProviderVideoDecoder {
    pub fn new(worker: CodecWorkerProcess, timebase: Rational64) -> Self {
        Self { worker, timebase }
    }

    pub fn decode_packet(&mut self, pts: i64, dts: i64, packet_bytes: &[u8]) -> Result<Option<VideoFrame>, EncoderError> {
        let res = self.worker.decode_video_packet(pts, dts, packet_bytes)?;
        if let Some((pts, width, height, pixel_format, data)) = res {
            let dims = Dimensions::new(width, height)?;
            let frame = VideoFrame::from_raw_bytes(dims, pixel_format, ColorSpaceTag::REC709, pts, 1, self.timebase, data)?;
            Ok(Some(frame))
        } else {
            Ok(None)
        }
    }

    pub fn worker(&self) -> &CodecWorkerProcess {
        &self.worker
    }

    pub fn worker_mut(&mut self) -> &mut CodecWorkerProcess {
        &mut self.worker
    }
}

/// Audio decoder backed by a supervised out-of-process provider child.
pub struct ProviderAudioDecoder {
    worker: CodecWorkerProcess,
}

impl ProviderAudioDecoder {
    pub fn new(worker: CodecWorkerProcess) -> Self {
        Self { worker }
    }

    pub fn decode_packet(&mut self, pts: i64, packet_bytes: &[u8]) -> Result<Option<AudioSampleBuffer>, EncoderError> {
        let res = self.worker.decode_audio_packet(pts, packet_bytes)?;
        if let Some((_pts, sample_rate, channels, frames, data)) = res {
            let layout = match channels {
                1 => ChannelLayout::Mono,
                2 => ChannelLayout::Stereo,
                6 => ChannelLayout::Surround51,
                8 => ChannelLayout::Surround71,
                _ => return Err(EncoderError::InvalidJobSpec(format!("unsupported provider audio channel count: {channels}"))),
            };
            // Read interleaved f32 samples
            let float_count = data.len() / 4;
            if float_count < (frames as usize * channels as usize) {
                return Err(EncoderError::InvalidJobSpec("decoded audio buffer truncated".into()));
            }
            let mut planar = vec![Vec::with_capacity(frames as usize); channels as usize];
            for f in 0..frames as usize {
                for (ch, plan) in planar.iter_mut().enumerate() {
                    let offset = (f * channels as usize + ch) * 4;
                    let sample = f32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                    plan.push(sample);
                }
            }
            let buffer = AudioSampleBuffer::from_planar_channels(sample_rate, layout, 0, planar)?;
            Ok(Some(buffer))
        } else {
            Ok(None)
        }
    }

    pub fn worker(&self) -> &CodecWorkerProcess {
        &self.worker
    }

    pub fn worker_mut(&mut self) -> &mut CodecWorkerProcess {
        &mut self.worker
    }
}
