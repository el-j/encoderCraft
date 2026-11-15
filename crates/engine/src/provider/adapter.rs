//! VideoEncoder trait adapter wrapping out-of-process codec worker processes.

use encodercraft_core::{EncoderError, VideoFrame};
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
